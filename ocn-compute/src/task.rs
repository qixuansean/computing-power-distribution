use serde::{Serialize, Deserialize};
use sha2::{Sha256, Digest};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_PAYLOAD_SIZE: usize = 10 * 1024 * 1024; // 10MB
pub const MAX_RESULT_SIZE: usize = 10 * 1024 * 1024; // 10MB
pub const MAX_TASK_ID_LEN: usize = 128;
pub const MAX_IDEMPOTENCY_KEY_LEN: usize = 128;
pub const MIN_TIMEOUT_MS: u64 = 1000;
pub const MAX_TIMEOUT_MS: u64 = 3600_000; // 1 hour
pub const MAX_DEADLINE_OFFSET_MS: u64 = 86400_000; // 24 hours from now

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum TaskType {
    BatchHash,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TaskDescriptor {
    pub task_id: String,
    pub task_type: TaskType,
    pub protocol_version: u32,
    pub input_hash: [u8; 32],
    pub payload: Vec<u8>,
    pub resource_requirements: ResourceRequirements,
    pub priority: u32,
    pub deadline_unix_ms: u64,
    pub timeout_ms: u64,
    pub idempotency_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ResourceRequirements {
    pub cpu_cores: u32,
    pub memory_mb: u64,
    pub gpu_required: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ExecutionResult {
    pub task_id: String,
    pub node_id: String,
    pub result: Vec<u8>,
    pub result_hash: [u8; 32],
    pub execution_time_ms: u64,
    pub status: TaskStatus,
    pub error_code: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Success,
    Failed,
    Rejected,
}

impl TaskDescriptor {
    /// Validates all fields of the task descriptor
    pub fn validate(&self) -> Result<(), TaskValidationError> {
        // Check protocol version
        if self.protocol_version != 1 {
            return Err(TaskValidationError::InvalidProtocolVersion);
        }

        // Check task_id length
        if self.task_id.is_empty() || self.task_id.len() > MAX_TASK_ID_LEN {
            return Err(TaskValidationError::InvalidTaskIdLength);
        }

        // Check idempotency_key length
        if self.idempotency_key.is_empty() || self.idempotency_key.len() > MAX_IDEMPOTENCY_KEY_LEN {
            return Err(TaskValidationError::InvalidIdempotencyKeyLength);
        }

        // Check priority range [0, 100]
        if self.priority > 100 {
            return Err(TaskValidationError::InvalidPriority);
        }

        // Check timeout range
        if self.timeout_ms < MIN_TIMEOUT_MS || self.timeout_ms > MAX_TIMEOUT_MS {
            return Err(TaskValidationError::InvalidTimeout);
        }

        // Check deadline - must be in the future but not too far
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| TaskValidationError::TimeError)?
            .as_millis() as u64;

        if self.deadline_unix_ms <= now_ms {
            return Err(TaskValidationError::DeadlineInPast);
        }

        if self.deadline_unix_ms > now_ms + MAX_DEADLINE_OFFSET_MS {
            return Err(TaskValidationError::DeadlineTooFar);
        }

        // Check payload size
        if self.payload.len() > MAX_PAYLOAD_SIZE {
            return Err(TaskValidationError::PayloadTooLarge);
        }

        // Verify input hash
        if !self.validate_input_hash() {
            return Err(TaskValidationError::InputHashMismatch);
        }

        // Validate resource requirements
        if self.resource_requirements.cpu_cores == 0 || self.resource_requirements.cpu_cores > 256 {
            return Err(TaskValidationError::InvalidResourceRequirements);
        }

        if self.resource_requirements.memory_mb == 0 || self.resource_requirements.memory_mb > 1024 * 1024 {
            return Err(TaskValidationError::InvalidResourceRequirements);
        }

        Ok(())
    }

    /// Validates that the input_hash matches the SHA-256 of payload
    pub fn validate_input_hash(&self) -> bool {
        let mut hasher = Sha256::new();
        hasher.update(&self.payload);
        let hash: [u8; 32] = hasher.finalize().into();
        hash == self.input_hash
    }
}

impl ExecutionResult {
    /// Validates the execution result
    pub fn validate(&self) -> Result<(), TaskValidationError> {
        // Check result size
        if self.result.len() > MAX_RESULT_SIZE {
            return Err(TaskValidationError::ResultTooLarge);
        }

        // Verify result hash
        if !self.validate_result_hash() {
            return Err(TaskValidationError::ResultHashMismatch);
        }

        // Status must be valid (enforced by enum)
        Ok(())
    }

    /// Validates that the result_hash matches the SHA-256 of result
    pub fn validate_result_hash(&self) -> bool {
        let mut hasher = Sha256::new();
        hasher.update(&self.result);
        let hash: [u8; 32] = hasher.finalize().into();
        hash == self.result_hash
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TaskValidationError {
    #[error("Invalid protocol version: expected 1")]
    InvalidProtocolVersion,
    #[error("Task ID length must be between 1 and {MAX_TASK_ID_LEN}")]
    InvalidTaskIdLength,
    #[error("Idempotency key length must be between 1 and {MAX_IDEMPOTENCY_KEY_LEN}")]
    InvalidIdempotencyKeyLength,
    #[error("Priority must be in range [0, 100]")]
    InvalidPriority,
    #[error("Timeout must be in range [{MIN_TIMEOUT_MS}, {MAX_TIMEOUT_MS}] ms")]
    InvalidTimeout,
    #[error("Deadline must be in the future")]
    DeadlineInPast,
    #[error("Deadline too far in the future (max 24h)")]
    DeadlineTooFar,
    #[error("Payload size exceeds maximum of {MAX_PAYLOAD_SIZE} bytes")]
    PayloadTooLarge,
    #[error("Input hash mismatch")]
    InputHashMismatch,
    #[error("Result size exceeds maximum of {MAX_RESULT_SIZE} bytes")]
    ResultTooLarge,
    #[error("Result hash mismatch")]
    ResultHashMismatch,
    #[error("Invalid resource requirements")]
    InvalidResourceRequirements,
    #[error("Time error")]
    TimeError,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Sha256, Digest};

    fn create_valid_task() -> TaskDescriptor {
        let payload = b"test data for hashing";
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
    fn test_valid_task_descriptor() {
        let task = create_valid_task();
        assert!(task.validate().is_ok());
    }

    #[test]
    fn test_invalid_input_hash() {
        let mut task = create_valid_task();
        task.input_hash = [0u8; 32]; // Wrong hash
        assert!(matches!(task.validate(), Err(TaskValidationError::InputHashMismatch)));
    }

    #[test]
    fn test_invalid_protocol_version() {
        let mut task = create_valid_task();
        task.protocol_version = 2;
        assert!(matches!(task.validate(), Err(TaskValidationError::InvalidProtocolVersion)));
    }

    #[test]
    fn test_priority_out_of_range() {
        let mut task = create_valid_task();
        task.priority = 101;
        assert!(matches!(task.validate(), Err(TaskValidationError::InvalidPriority)));
    }

    #[test]
    fn test_timeout_too_small() {
        let mut task = create_valid_task();
        task.timeout_ms = 500;
        assert!(matches!(task.validate(), Err(TaskValidationError::InvalidTimeout)));
    }

    #[test]
    fn test_timeout_too_large() {
        let mut task = create_valid_task();
        task.timeout_ms = MAX_TIMEOUT_MS + 1;
        assert!(matches!(task.validate(), Err(TaskValidationError::InvalidTimeout)));
    }

    #[test]
    fn test_deadline_in_past() {
        let mut task = create_valid_task();
        task.deadline_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64 - 1000;
        assert!(matches!(task.validate(), Err(TaskValidationError::DeadlineInPast)));
    }

    #[test]
    fn test_payload_too_large() {
        let mut task = create_valid_task();
        task.payload = vec![0u8; MAX_PAYLOAD_SIZE + 1];
        // Recompute hash for new payload
        let mut hasher = Sha256::new();
        hasher.update(&task.payload);
        task.input_hash = hasher.finalize().into();
        assert!(matches!(task.validate(), Err(TaskValidationError::PayloadTooLarge)));
    }

    #[test]
    fn test_empty_task_id() {
        let mut task = create_valid_task();
        task.task_id = "".to_string();
        assert!(matches!(task.validate(), Err(TaskValidationError::InvalidTaskIdLength)));
    }

    #[test]
    fn test_empty_idempotency_key() {
        let mut task = create_valid_task();
        task.idempotency_key = "".to_string();
        assert!(matches!(task.validate(), Err(TaskValidationError::InvalidIdempotencyKeyLength)));
    }

    #[test]
    fn test_execution_result_validation() {
        let result = b"hash result";
        let mut hasher = Sha256::new();
        hasher.update(result);
        let result_hash: [u8; 32] = hasher.finalize().into();

        let exec_result = ExecutionResult {
            task_id: "test-task-1".to_string(),
            node_id: "node-1".to_string(),
            result: result.to_vec(),
            result_hash,
            execution_time_ms: 100,
            status: TaskStatus::Success,
            error_code: None,
        };

        assert!(exec_result.validate().is_ok());
    }

    #[test]
    fn test_execution_result_hash_mismatch() {
        let result = b"hash result";
        let result_hash = [0u8; 32]; // Wrong hash

        let exec_result = ExecutionResult {
            task_id: "test-task-1".to_string(),
            node_id: "node-1".to_string(),
            result: result.to_vec(),
            result_hash,
            execution_time_ms: 100,
            status: TaskStatus::Success,
            error_code: None,
        };

        assert!(matches!(exec_result.validate(), Err(TaskValidationError::ResultHashMismatch)));
    }

    #[test]
    fn test_result_too_large() {
        let result = vec![0u8; MAX_RESULT_SIZE + 1];
        let mut hasher = Sha256::new();
        hasher.update(&result);
        let result_hash: [u8; 32] = hasher.finalize().into();

        let exec_result = ExecutionResult {
            task_id: "test-task-1".to_string(),
            node_id: "node-1".to_string(),
            result,
            result_hash,
            execution_time_ms: 100,
            status: TaskStatus::Success,
            error_code: None,
        };

        assert!(matches!(exec_result.validate(), Err(TaskValidationError::ResultTooLarge)));
    }
}