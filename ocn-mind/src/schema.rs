use serde::{Serialize, Deserialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DataSource {
    SIMULATED,
    REAL_DEVICE,
    HISTORICAL,
    SYNTHETIC,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDescriptor {
    pub task_id: Uuid,
    pub task_type: String,
    pub priority: u32,
    pub deadline: DateTime<Utc>,
    pub input_size: u64,
    pub required_capabilities: Vec<String>,
    pub allowed_execution_modes: Vec<String>,
    pub schema_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSnapshot {
    pub node_id: String,
    pub platform: String,
    pub cpu_available: f32, // 0.0 to 1.0
    pub memory_available: u64, // in bytes
    pub gpu_available: Option<f32>,
    pub network_latency: f32, // ms
    pub network_bandwidth: f32, // Mbps
    pub thermal_state: String,
    pub power_state: String,
    pub timestamp: DateTime<Utc>,
    pub schema_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcePredictionLabel {
    pub actual_cpu_usage: f32,
    pub actual_memory_usage: u64,
    pub actual_gpu_usage: Option<f32>,
    pub actual_execution_time: f32, // seconds
    pub success: bool,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulingRecord {
    pub selected_node: String,
    pub alternative_nodes: Vec<String>,
    pub scheduling_reason: String,
    pub predicted_cost: f32,
    pub actual_cost: f32,
    pub completion_status: String,
    pub data_source: DataSource,
}
