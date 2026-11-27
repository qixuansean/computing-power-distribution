//! QUIC task server: executes compute tasks received over authenticated QUIC streams.
//!
//! The server reuses the existing [`ComputeExecutor`] and [`TaskDeduplicationCache`].
//! Each inbound QUIC bidirectional stream carries exactly one task request and one task
//! result; no results are fabricated, every successful result is produced by the real
//! executor and re-hashed independently by the client.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ocn_core::task_wire::error_code;
use ocn_core::{QuicConnection, TaskRequestFrame, TaskResultFrame, TaskStream, TaskWireError};

use crate::deduplication::{CacheCheckResult, TaskDeduplicationCache};
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
}

impl TaskServer {
    /// Create a server that identifies itself as `node_id` in every result frame.
    pub fn new(node_id: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            cache: TaskDeduplicationCache::new(Duration::from_secs(300), 1024),
            idle_timeout: Duration::from_secs(1),
            request_timeout: Duration::from_secs(15),
            execution_count: AtomicU64::new(0),
        }
    }

    /// Override the stream idle and per-request read timeouts.
    pub fn with_timeouts(mut self, idle_timeout: Duration, request_timeout: Duration) -> Self {
        self.idle_timeout = idle_timeout;
        self.request_timeout = request_timeout;
        self
    }

    /// Number of tasks actually executed (deduplicated replays are not counted).
    pub fn execution_count(&self) -> u64 {
        self.execution_count.load(Ordering::SeqCst)
    }

    /// Serve task streams on one authenticated connection until it closes.
    ///
    /// Streams are processed one at a time per connection; separate connections are
    /// served concurrently by their own tasks. Every task still gets its own QUIC
    /// bidirectional stream.
    pub async fn serve_connection(&self, connection: &QuicConnection) {
        loop {
            match connection.accept_task(self.idle_timeout).await {
                Ok(Some(mut stream)) => self.handle_stream(&mut stream).await,
                Ok(None) => continue,
                Err(_) => break,
            }
        }
    }

    async fn handle_stream(&self, stream: &mut TaskStream) {
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
            Ok(()) => self.execute_request(&request).await,
            Err(error) => TaskResultFrame::failure(
                request.task_id.clone(),
                &self.node_id,
                error.code(),
                error.to_string(),
            ),
        };

        let _ = stream.send_result(&result).await;
    }

    async fn execute_request(&self, request: &TaskRequestFrame) -> TaskResultFrame {
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

        match self.cache.check_or_register(&descriptor).await {
            Ok(CacheCheckResult::NewTask) => self.execute_new(descriptor).await,
            Ok(CacheCheckResult::Completed(result)) => {
                result_to_frame(result, &self.node_id, true)
            }
            Ok(CacheCheckResult::AlreadyRunning) => TaskResultFrame::failure(
                request.task_id.clone(),
                &self.node_id,
                error_code::ALREADY_RUNNING,
                "an identical task is already running",
            ),
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

    async fn execute_new(&self, descriptor: TaskDescriptor) -> TaskResultFrame {
        let idempotency_key = descriptor.idempotency_key.clone();
        let task_id = descriptor.task_id.clone();
        let timeout = Duration::from_millis(descriptor.timeout_ms);

        let execution = tokio::task::spawn_blocking(move || ComputeExecutor::execute(&descriptor));

        match tokio::time::timeout(timeout, execution).await {
            Err(_) => {
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
