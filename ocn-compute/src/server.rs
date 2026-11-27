//! QUIC task server: executes compute tasks received over authenticated QUIC streams.
//!
//! The server reuses the existing [`ComputeExecutor`] and [`TaskDeduplicationCache`].
//! Each inbound QUIC bidirectional stream carries exactly one task request and one task
//! result; no results are fabricated, every successful result is produced by the real
//! executor and re-hashed independently by the client.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::{broadcast, OwnedSemaphorePermit, Semaphore};

use ocn_core::task_wire::error_code;
use ocn_core::{QuicConnection, TaskRequestFrame, TaskResultFrame, TaskStream, TaskWireError};

use crate::deduplication::{CacheCheckResult, TaskDeduplicationCache, TaskExecutionState};
use crate::runtime::{ComputeExecutor, RuntimeError};
use crate::task::{
    ExecutionResult, ResourceRequirements, TaskDescriptor, TaskStatus, TaskType,
    MAX_DEADLINE_OFFSET_MS,
};

/// Server side handler for the OCN task wire protocol.
pub struct TaskServer {
    node_id: String,
    cache: TaskDeduplicationCache,
    idle_timeout: Duration,
    request_timeout: Duration,
    execution_count: AtomicU64,
    /// Bounds how many task streams may be admitted at once, across all connections
    /// served by this [`TaskServer`].
    inflight: Arc<Semaphore>,
    max_inflight: usize,
    inflight_now: Arc<AtomicUsize>,
    peak_inflight: AtomicUsize,
}

impl TaskServer {
    /// Create a server that identifies itself as `node_id` in every result frame.
    pub fn new(node_id: impl Into<String>) -> Self {
        let max_inflight = default_max_inflight();
        Self {
            node_id: node_id.into(),
            cache: TaskDeduplicationCache::new(Duration::from_secs(300), 1024),
            idle_timeout: Duration::from_secs(1),
            request_timeout: Duration::from_secs(15),
            execution_count: AtomicU64::new(0),
            inflight: Arc::new(Semaphore::new(max_inflight)),
            max_inflight,
            inflight_now: Arc::new(AtomicUsize::new(0)),
            peak_inflight: AtomicUsize::new(0),
        }
    }

    /// Override the stream idle and per-request read timeouts.
    pub fn with_timeouts(mut self, idle_timeout: Duration, request_timeout: Duration) -> Self {
        self.idle_timeout = idle_timeout;
        self.request_timeout = request_timeout;
        self
    }

    /// Bound how many task streams are admitted concurrently.
    ///
    /// At capacity the accept loop stops reading new streams, which applies QUIC
    /// backpressure instead of buffering unbounded work. Values below 1 are clamped to 1.
    pub fn with_max_inflight(mut self, permits: usize) -> Self {
        let permits = permits.max(1);
        self.inflight = Arc::new(Semaphore::new(permits));
        self.max_inflight = permits;
        self
    }

    /// Configured concurrent-admission limit.
    pub fn max_inflight(&self) -> usize {
        self.max_inflight
    }

    /// Highest number of task streams admitted at the same time since startup.
    pub fn peak_inflight(&self) -> usize {
        self.peak_inflight.load(Ordering::SeqCst)
    }

    /// Number of tasks actually executed (deduplicated replays are not counted).
    pub fn execution_count(&self) -> u64 {
        self.execution_count.load(Ordering::SeqCst)
    }

    /// Serve task streams on one authenticated connection until it closes.
    ///
    /// Each accepted stream is handled in its own task, so a slow or stalled stream can
    /// never delay a sibling stream on the same connection. Admission is bounded by
    /// [`Self::with_max_inflight`]; while at capacity the loop stops accepting, which is
    /// the intended backpressure. Separate connections are served by their own loops.
    pub async fn serve_connection(self: &Arc<Self>, connection: &QuicConnection) {
        loop {
            match connection.accept_task(self.idle_timeout).await {
                Ok(Some(stream)) => {
                    let permit = match self.inflight.clone().acquire_owned().await {
                        Ok(permit) => permit,
                        Err(_) => break,
                    };
                    let guard = InflightGuard::new(permit, self.inflight_now.clone());
                    let now = guard.observed();
                    self.peak_inflight.fetch_max(now, Ordering::SeqCst);

                    let server = Arc::clone(self);
                    tokio::spawn(async move {
                        server.handle_stream(stream, guard).await;
                    });
                }
                Ok(None) => continue,
                Err(_) => break,
            }
        }
    }

    async fn handle_stream(&self, mut stream: TaskStream, permit: InflightGuard) {
        let request = match stream.recv_request(self.request_timeout).await {
            Ok(request) => request,
            Err(error) => {
                let code = error
                    .downcast_ref::<TaskWireError>()
                    .map(TaskWireError::code)
                    .unwrap_or(error_code::BAD_FRAME);
                let _ = stream
                    .send_result(&TaskResultFrame::failure(
                        unknown_task_id(),
                        &self.node_id,
                        code,
                        format!("invalid task frame: {error:#}"),
                    ))
                    .await;
                return;
            }
        };

        let result = match request.validate() {
            Ok(()) => self.execute_request(&request, permit).await,
            Err(error) => TaskResultFrame::failure(
                request.task_id.clone(),
                &self.node_id,
                error.code(),
                error.to_string(),
            ),
        };

        let _ = stream.send_result(&result).await;
    }

    async fn execute_request(
        &self,
        request: &TaskRequestFrame,
        permit: InflightGuard,
    ) -> TaskResultFrame {
        let descriptor = match descriptor_from_request(request) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                return TaskResultFrame::failure(
                    request.task_id.clone(),
                    &self.node_id,
                    error_code::INVALID_TASK,
                    error,
                );
            }
        };

        // Subscribe before checking, so a completion that races with this call is still
        // observed. Only concurrent duplicates actually wait on this receiver.
        let completions = self.cache.subscribe_completions();

        match self.cache.check_or_register(&descriptor).await {
            Ok(CacheCheckResult::NewTask) => self.execute_new(descriptor, permit).await,
            Ok(CacheCheckResult::Completed(result)) => {
                result_to_frame(result, &self.node_id, true)
            }
            Ok(CacheCheckResult::AlreadyRunning) => {
                self.await_duplicate(&descriptor, completions, permit).await
            }
            Ok(CacheCheckResult::Failed(error)) => TaskResultFrame::failure(
                request.task_id.clone(),
                &self.node_id,
                error_code::INVALID_TASK,
                error,
            ),
            Err(error) => TaskResultFrame::failure(
                request.task_id.clone(),
                &self.node_id,
                error_code::INTERNAL,
                error.to_string(),
            ),
        }
    }

    /// Single-flight: wait for the in-flight original, then serve its stored result.
    ///
    /// The waiter's own `timeout_ms` is the wait budget. The permit is held while waiting
    /// so a flood of duplicate requests cannot bypass the admission limit. The loop always
    /// re-reads the cache before waiting again, so a lagged broadcast cannot end the wait
    /// while the original is still running.
    async fn await_duplicate(
        &self,
        descriptor: &TaskDescriptor,
        mut completions: broadcast::Receiver<String>,
        _permit: InflightGuard,
    ) -> TaskResultFrame {
        let key = descriptor.idempotency_key.clone();
        let deadline = Instant::now() + Duration::from_millis(descriptor.timeout_ms);

        loop {
            match self.cache.lookup(&key).await {
                Some(TaskExecutionState::Completed { result, .. }) => {
                    return result_to_frame(result, &self.node_id, true);
                }
                Some(TaskExecutionState::Failed { error, .. }) => {
                    return TaskResultFrame::failure(
                        descriptor.task_id.clone(),
                        &self.node_id,
                        error_code::INVALID_TASK,
                        error,
                    );
                }
                Some(TaskExecutionState::Running { .. }) => {}
                None => {
                    return TaskResultFrame::failure(
                        descriptor.task_id.clone(),
                        &self.node_id,
                        error_code::INVALID_TASK,
                        "the identical task is no longer tracked by the deduplication cache",
                    );
                }
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return TaskResultFrame::failure(
                    descriptor.task_id.clone(),
                    &self.node_id,
                    error_code::TIMEOUT,
                    "an identical task is still running and did not finish within the request timeout",
                );
            }

            tokio::time::timeout(remaining, async {
                loop {
                    match completions.recv().await {
                        Ok(completed_key) if completed_key == key => return,
                        // A different task finished; keep waiting for ours.
                        Ok(_) => continue,
                        // Buffered completions were dropped; re-read the cache instead.
                        Err(broadcast::error::RecvError::Lagged(_)) => return,
                        Err(broadcast::error::RecvError::Closed) => return,
                    }
                }
            })
            .await;
        }
    }

    async fn execute_new(
        &self,
        descriptor: TaskDescriptor,
        permit: InflightGuard,
    ) -> TaskResultFrame {
        let idempotency_key = descriptor.idempotency_key.clone();
        let task_id = descriptor.task_id.clone();
        let timeout = Duration::from_millis(descriptor.timeout_ms);

        let mut execution = tokio::task::spawn_blocking(move || ComputeExecutor::execute(&descriptor));

        match tokio::time::timeout(timeout, &mut execution).await {
            Err(_) => {
                // The blocking closure cannot be cancelled. Keep the admission permit
                // accounted for in a reaper until the real work actually ends, so a
                // timed-out task cannot free capacity it is still consuming.
                tokio::spawn(async move {
                    let _permit = permit;
                    let _ = execution.await;
                });
                self.cache
                    .store_failed(&idempotency_key, "task execution timed out".to_string())
                    .await;
                TaskResultFrame::failure(
                    task_id,
                    &self.node_id,
                    error_code::TIMEOUT,
                    "task execution timed out",
                )
            }
            Ok(Err(join_error)) => {
                let message = format!("executor task failed: {join_error}");
                self.cache
                    .store_failed(&idempotency_key, message.clone())
                    .await;
                TaskResultFrame::failure(task_id, &self.node_id, error_code::INTERNAL, message)
            }
            Ok(Ok(Err(runtime_error))) => {
                let code = runtime_error_code(&runtime_error);
                let message = runtime_error.to_string();
                self.cache
                    .store_failed(&idempotency_key, message.clone())
                    .await;
                TaskResultFrame::failure(task_id, &self.node_id, code, message)
            }
            Ok(Ok(Ok(result))) => {
                self.execution_count.fetch_add(1, Ordering::SeqCst);
                self.cache
                    .store_completed(&idempotency_key, result.clone())
                    .await;
                result_to_frame(result, &self.node_id, false)
            }
        }
    }
}

/// Admitted-stream permit that keeps the in-flight gauge accurate.
///
/// Moving this into the timeout reaper is what keeps a detached `spawn_blocking`
/// execution accounted for after its request future has returned.
struct InflightGuard {
    _permit: OwnedSemaphorePermit,
    current: Arc<AtomicUsize>,
    observed: usize,
}

impl InflightGuard {
    fn new(permit: OwnedSemaphorePermit, current: Arc<AtomicUsize>) -> Self {
        let observed = current.fetch_add(1, Ordering::SeqCst) + 1;
        Self {
            _permit: permit,
            current,
            observed,
        }
    }

    fn observed(&self) -> usize {
        self.observed
    }
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        self.current.fetch_sub(1, Ordering::SeqCst);
    }
}

fn default_max_inflight() -> usize {
    std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(4)
        .max(1)
}

/// Client helper that issues a task over an authenticated QUIC connection and verifies
/// the returned result hash independently before handing it to the caller.
pub struct TaskClient<'a> {
    connection: &'a QuicConnection,
}

impl<'a> TaskClient<'a> {
    pub fn new(connection: &'a QuicConnection) -> Self {
        Self { connection }
    }

    /// Execute `request`, waiting at most `request.timeout_ms`.
    pub async fn execute(&self, request: &TaskRequestFrame) -> anyhow::Result<TaskResultFrame> {
        self.execute_with_timeout(request, Duration::from_millis(request.timeout_ms))
            .await
    }

    /// Execute `request` with an explicit client-side wait budget.
    pub async fn execute_with_timeout(
        &self,
        request: &TaskRequestFrame,
        timeout: Duration,
    ) -> anyhow::Result<TaskResultFrame> {
        let mut exchange = self.connection.open_task(request).await?;
        let result = exchange.recv_result_with_timeout(timeout).await?;
        result.verify_for_request(request)?;
        Ok(result)
    }
}

fn descriptor_from_request(request: &TaskRequestFrame) -> Result<TaskDescriptor, String> {
    if !request.verify_input_hash() {
        return Err("input hash does not match payload".to_string());
    }

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock before unix epoch: {error}"))?
        .as_millis() as u64;

    Ok(TaskDescriptor {
        task_id: request.task_id.clone(),
        task_type: TaskType::BatchHash,
        protocol_version: 1,
        input_hash: request.input_hash,
        payload: request.payload.clone(),
        resource_requirements: ResourceRequirements {
            cpu_cores: 1,
            memory_mb: 64,
            gpu_required: false,
        },
        priority: 50,
        deadline_unix_ms: now_ms.saturating_add(request.timeout_ms.min(MAX_DEADLINE_OFFSET_MS)),
        timeout_ms: request.timeout_ms,
        idempotency_key: request.task_id.clone(),
    })
}

fn result_to_frame(result: ExecutionResult, node_id: &str, duplicate: bool) -> TaskResultFrame {
    if result.status == TaskStatus::Success {
        TaskResultFrame::success(
            result.task_id,
            node_id,
            result.result,
            result.result_hash,
            result.execution_time_ms,
            duplicate,
        )
    } else {
        TaskResultFrame::failure(
            result.task_id,
            node_id,
            result.error_code.unwrap_or(error_code::INTERNAL),
            "task execution failed",
        )
    }
}

fn runtime_error_code(error: &RuntimeError) -> u32 {
    match error {
        RuntimeError::ValidationError(_) => error_code::INVALID_TASK,
        RuntimeError::UnsupportedTaskType => error_code::UNSUPPORTED_TASK,
        RuntimeError::ExecutionFailed(_) => error_code::INTERNAL,
    }
}

fn unknown_task_id() -> String {
    "unknown".to_string()
}
