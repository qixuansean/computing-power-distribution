use crate::schema::{TaskDescriptor, NodeSnapshot, SchedulingRecord};
use async_trait::async_trait;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MindError {
    #[error("Model load failed: {0}")]
    LoadFailed(String),
    #[error("Inference timeout")]
    Timeout,
    #[error("Model execution failed: {0}")]
    ExecutionError(String),
    #[error("Insufficient resources for mode {0:?}")]
    ResourceConstraint(RuntimeMode),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    RuleOnly,
    Compact,
    Full,
    Unloaded,
}

#[async_trait]
pub trait MindProvider: Send + Sync {
    async fn predict_scheduling(&self, task: &TaskDescriptor, nodes: &[NodeSnapshot]) -> Result<SchedulingRecord, MindError>;
    fn name(&self) -> &str;
}

pub struct RuntimeManager {
    current_mode: RuntimeMode,
    // Other states like loaded models...
}

impl RuntimeManager {
    pub fn new() -> Self {
        Self {
            current_mode: RuntimeMode::Unloaded,
        }
    }

    pub fn set_mode(&mut self, mode: RuntimeMode) -> Result<(), MindError> {
        // Implementation for mode switching logic
        self.current_mode = mode;
        Ok(())
    }

    pub fn current_mode(&self) -> RuntimeMode {
        self.current_mode
    }
}
