use serde::{Serialize, Deserialize};
use async_trait::async_trait;
use std::fmt;
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct NodeId(pub String);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TaskId(pub String);

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum TaskType {
    Compute,
    Storage,
    MemoryOverflow,
    Inference,
    Gaming,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ResourceEstimate {
    pub cpu_usage: f32,       // 0.0 to 1.0
    pub memory_mb: u64,
    pub gpu_required: bool,
    pub confidence: f32,      // 0.0 to 1.0
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ResourceBudget {
    pub max_cpu: f32,
    pub max_memory_mb: u64,
    pub allow_gpu: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum SchedulingAction {
    LocalExecute,
    RemoteOverflow { target_node: NodeId },
    PauseBackground,
    RejectTask { reason: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TaskAnalysisRequest {
    pub task_id: TaskId,
    pub task_type: TaskType,
    pub description: String,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TaskAnalysisResult {
    pub estimate: ResourceEstimate,
    pub metadata: std::collections::HashMap<String, String>,
    pub confidence: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SchedulingRequest {
    pub task_id: TaskId,
    pub analysis: TaskAnalysisResult,
    pub budget: ResourceBudget,
    pub available_nodes: Vec<NodeId>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SchedulingRecommendation {
    pub action: SchedulingAction,
    pub score: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum MindError {
    #[error("Inference timeout")]
    Timeout,
    #[error("Model not loaded")]
    ModelNotLoaded,
    #[error("Invalid output: {0}")]
    InvalidOutput(String),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Validation failed: {0}")]
    ValidationFailed(String),
}

#[async_trait]
pub trait MindProvider: Send + Sync {
    /// Analyze task characteristics and resource requirements
    /// Returns a ResourceEstimate with confidence score
    async fn analyze_task(&self, request: TaskAnalysisRequest)
        -> Result<TaskAnalysisResult, MindError>;

    /// Recommend scheduling decision based on analysis and available resources
    /// Returns a SchedulingRecommendation with a confidence score
    async fn recommend_schedule(&self, request: SchedulingRequest)
        -> Result<SchedulingRecommendation, MindError>;

    /// Load or reload model weights
    async fn load_model(&self, model_path: &str) -> Result<(), MindError>;

    /// Unload model weights to free resources
    async fn unload_model(&self) -> Result<(), MindError>;

    /// Get current provider status
    fn status(&self) -> String;

    /// Get provider name
    fn provider_name(&self) -> String;

    /// Get provider version
    fn provider_version(&self) -> String;

    /// Check if provider is loaded
    fn is_loaded(&self) -> bool;
}

/// Configuration for MindProvider
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub version: String,
    pub timeout_ms: u64,
    pub max_retries: u32,
    pub enable_cache: bool,
}

/// Output validation rules
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutputValidation {
    pub min_confidence: f32,
    pub max_cpu_usage: f32,
    pub min_memory_mb: u64,
    pub allowed_actions: Vec<SchedulingAction>,
}
