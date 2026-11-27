use crate::task::{TaskDescriptor, ExecutionResult, TaskStatus};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub enum TaskExecutionState {
    Running { started_at: Instant },
    Completed { result: ExecutionResult, completed_at: Instant },
    Failed { error: String, failed_at: Instant },
}

pub struct TaskDeduplicationCache {
    cache: Arc<Mutex<HashMap<String, TaskExecutionState>>>,
    ttl: Duration,
    max_capacity: usize,
}

impl TaskDeduplicationCache {
    pub fn new(ttl: Duration, max_capacity: usize) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            ttl,
            max_capacity,
        }
    }

    /// Try to register a task as running. 
    /// Returns Ok(true) if newly registered, Ok(false) if already running/completed, 
    /// or returns the cached result if already completed.
    pub async fn check_or_register(&self, task: &TaskDescriptor) -> Result<CacheCheckResult, DeduplicationError> {
        let mut cache = self.cache.lock().await;

        // Purge expired entries first
        self.purge_expired_locked(&mut cache);

        // Check by idempotency_key or task_id
        let key = task.idempotency_key.clone();

        if let Some(state) = cache.get(&key) {
            match state {
                TaskExecutionState::Running { .. } => {
                    return Ok(CacheCheckResult::AlreadyRunning);
                }
                TaskExecutionState::Completed { result, .. } => {
                    return Ok(CacheCheckResult::Completed(result.clone()));
                }
                TaskExecutionState::Failed { error, .. } => {
                    return Ok(CacheCheckResult::Failed(error.clone()));
                }
            }
        }

        // Check capacity limit
        if cache.len() >= self.max_capacity {
            return Err(DeduplicationError::CacheCapacityExceeded);
        }

        // Register as running
        cache.insert(
            key,
            TaskExecutionState::Running {
                started_at: Instant::now(),
            },
        );

        Ok(CacheCheckResult::NewTask)
    }

    /// Store completed result
    pub async fn store_completed(&self, idempotency_key: &str, result: ExecutionResult) {
        let mut cache = self.cache.lock().await;
        cache.insert(
            idempotency_key.to_string(),
            TaskExecutionState::Completed {
                result,
                completed_at: Instant::now(),
            },
        );
    }

    /// Store failure
    pub async fn store_failed(&self, idempotency_key: &str, error: String) {
        let mut cache = self.cache.lock().await;
        cache.insert(
            idempotency_key.to_string(),
            TaskExecutionState::Failed {
                error,
                failed_at: Instant::now(),
            },
        );
    }

    fn purge_expired_locked(&self, cache: &mut HashMap<String, TaskExecutionState>) {
        let now = Instant::now();
        cache.retain(|_, state| {
            match state {
                TaskExecutionState::Running { started_at } => {
                    now.duration_since(*started_at) < self.ttl * 2 // Running tasks get longer TTL
                }
                TaskExecutionState::Completed { completed_at, .. } => {
                    now.duration_since(*completed_at) < self.ttl
                }
                TaskExecutionState::Failed { failed_at, .. } => {
                    now.duration_since(*failed_at) < self.ttl
                }
            }
        });
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CacheCheckResult {
    NewTask,
    AlreadyRunning,
    Completed(ExecutionResult),
    Failed(String),
}

#[derive(Debug, thiserror::Error)]
pub enum DeduplicationError {
    #[error("Cache capacity exceeded")]
    CacheCapacityExceeded,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{TaskDescriptor, ResourceRequirements, TaskType, TaskStatus};
    use sha2::{Sha256, Digest};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn create_task_with_idem(idempotency_key: &str) -> TaskDescriptor {
        let payload = b"dedup test data";
        let mut hasher = Sha256::new();
        hasher.update(payload);
        let input_hash: [u8; 32] = hasher.finalize().into();

        TaskDescriptor {
            task_id: format!("task-{}", idempotency_key),
            task_type: TaskType::BatchHash,
            protocol_version: 1,
            input_hash,
            payload: payload.to_vec(),
            resource_requirements: ResourceRequirements {
                cpu_cores: 1,
                memory_mb: 64,
                gpu_required: false,
            },
            priority: 50,
            deadline_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64 + 60000,
            timeout_ms: 5000,
            idempotency_key: idempotency_key.to_string(),
        }
    }

    #[tokio::test]
    async fn test_deduplication_new_task() {
        let cache = TaskDeduplicationCache::new(Duration::from_secs(10), 100);
        let task = create_task_with_idem("idem-1");

        let res = cache.check_or_register(&task).await.unwrap();
        assert_eq!(res, CacheCheckResult::NewTask);
    }

    #[tokio::test]
    async fn test_deduplication_already_running() {
        let cache = TaskDeduplicationCache::new(Duration::from_secs(10), 100);
        let task = create_task_with_idem("idem-1");

        // First registration -> New
        assert_eq!(cache.check_or_register(&task).await.unwrap(), CacheCheckResult::NewTask);

        // Second registration -> AlreadyRunning
        assert_eq!(cache.check_or_register(&task).await.unwrap(), CacheCheckResult::AlreadyRunning);
    }

    #[tokio::test]
    async fn test_deduplication_completed_cached() {
        let cache = TaskDeduplicationCache::new(Duration::from_secs(10), 100);
        let task = create_task_with_idem("idem-1");

        // Register
        assert_eq!(cache.check_or_register(&task).await.unwrap(), CacheCheckResult::NewTask);

        // Store completed
        let exec_result = ExecutionResult {
            task_id: task.task_id.clone(),
            node_id: "node-1".to_string(),
            result: vec![1, 2, 3],
            result_hash: [0u8; 32],
            execution_time_ms: 50,
            status: TaskStatus::Success,
            error_code: None,
        };
        cache.store_completed(&task.idempotency_key, exec_result.clone()).await;

        // Next check should return Completed result
        let res = cache.check_or_register(&task).await.unwrap();
        match res {
            CacheCheckResult::Completed(r) => {
                assert_eq!(r.task_id, exec_result.task_id);
                assert_eq!(r.result, vec![1, 2, 3]);
            }
            _ => panic!("Expected Completed result"),
        }
    }

    #[tokio::test]
    async fn test_deduplication_capacity_limit() {
        let cache = TaskDeduplicationCache::new(Duration::from_secs(10), 1); // Capacity 1
        let task1 = create_task_with_idem("idem-1");
        let task2 = create_task_with_idem("idem-2");

        assert_eq!(cache.check_or_register(&task1).await.unwrap(), CacheCheckResult::NewTask);
        
        // Second task exceeds capacity 1
        let res = cache.check_or_register(&task2).await;
        assert!(res.is_err());
    }
}