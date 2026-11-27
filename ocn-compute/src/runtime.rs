use crate::task::{TaskDescriptor, ExecutionResult, TaskStatus, TaskValidationError};
use sha2::{Sha256, Digest};
use std::time::Instant;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("Task validation failed: {0}")]
    ValidationError(#[from] TaskValidationError),
    #[error("Unsupported task type")]
    UnsupportedTaskType,
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
}

pub struct ComputeExecutor;

impl ComputeExecutor {
    /// Execute a task and return the result
    pub fn execute(task: &TaskDescriptor) -> Result<ExecutionResult, RuntimeError> {
        // Validate task before execution
        task.validate()?;

        let start = Instant::now();

        // Only BatchHash is supported
        match task.task_type {
            crate::task::TaskType::BatchHash => Self::execute_batch_hash(task, start),
        }
    }

    fn execute_batch_hash(task: &TaskDescriptor, start: Instant) -> Result<ExecutionResult, RuntimeError> {
        // Compute SHA-256 of the payload
        let mut hasher = Sha256::new();
        hasher.update(&task.payload);
        let result_hash: [u8; 32] = hasher.finalize().into();

        let elapsed = start.elapsed().as_millis() as u64;
        let execution_time_ms = if elapsed == 0 { 1 } else { elapsed };

        let res = ExecutionResult {
            task_id: task.task_id.clone(),
            node_id: "local".to_string(),
            result: task.payload.clone(), // Result is the processed data (or hash)
            result_hash,
            execution_time_ms,
            status: TaskStatus::Success,
            error_code: None,
        };

        // Ensure result validates correctly
        res.validate().map_err(|e| RuntimeError::ExecutionFailed(format!("Result validation failed: {:?}", e)))?;

        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{TaskDescriptor, ResourceRequirements, TaskType};
    use sha2::{Sha256, Digest};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn create_valid_task(payload: &[u8]) -> TaskDescriptor {
        let mut hasher = Sha256::new();
        hasher.update(payload);
        let input_hash: [u8; 32] = hasher.finalize().into();

        TaskDescriptor {
            task_id: "test-task-1".to_string(),
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
            idempotency_key: "idem-key-1".to_string(),
        }
    }

    #[test]
    fn test_batch_hash_execution() {
        let payload = b"hello world test data";
        let task = create_valid_task(payload);

        let result = ComputeExecutor::execute(&task).unwrap();

        assert_eq!(result.task_id, "test-task-1");
        assert_eq!(result.status, TaskStatus::Success);
        assert!(result.execution_time_ms > 0);
        assert!(result.validate().is_ok());

        // Verify result matches expected SHA-256 of payload
        let mut hasher = Sha256::new();
        hasher.update(payload);
        let expected: [u8; 32] = hasher.finalize().into();
        assert_eq!(result.result_hash, expected);
        assert_eq!(result.result, payload.to_vec());
    }

    #[test]
    fn test_empty_payload() {
        let payload = b"";
        let task = create_valid_task(payload);

        let result = ComputeExecutor::execute(&task).unwrap();
        assert_eq!(result.status, TaskStatus::Success);
        assert!(result.validate().is_ok());
    }

    #[test]
    fn test_large_payload() {
        let payload = vec![0x42u8; 1024 * 1024]; // 1MB
        let task = create_valid_task(&payload);

        let result = ComputeExecutor::execute(&task).unwrap();
        assert_eq!(result.status, TaskStatus::Success);
        assert!(result.validate().is_ok());
    }

    #[test]
    fn test_execution_time_recorded() {
        let task = create_valid_task(b"test data for timing");

        let result = ComputeExecutor::execute(&task).unwrap();
        assert!(result.execution_time_ms > 0);
    }
}