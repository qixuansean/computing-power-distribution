use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct NodeMetrics {
    pub rtt_ms: f64,
    pub throughput_mbps: f64,
    pub cpu_usage_pct: f32,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}
