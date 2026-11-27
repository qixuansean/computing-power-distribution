use crate::runtime::{MindProvider, MindError};
use crate::schema::{TaskDescriptor, NodeSnapshot, SchedulingRecord, DataSource};
use async_trait::async_trait;

pub struct RuleBasedScheduler;

#[async_trait]
impl MindProvider for RuleBasedScheduler {
    async fn predict_scheduling(&self, _task: &TaskDescriptor, nodes: &[NodeSnapshot]) -> Result<SchedulingRecord, MindError> {
        // Simple heuristic: pick node with most available CPU
        let best_node = nodes.iter()
            .max_by(|a, b| a.cpu_available.partial_cmp(&b.cpu_available).unwrap())
            .ok_or_else(|| MindError::ExecutionError("No nodes available".into()))?;

        Ok(SchedulingRecord {
            selected_node: best_node.node_id.clone(),
            alternative_nodes: vec![],
            scheduling_reason: "Maximum CPU availability".into(),
            predicted_cost: 0.0,
            actual_cost: 0.0,
            completion_status: "PENDING".into(),
            data_source: DataSource::SIMULATED,
        })
    }

    fn name(&self) -> &str {
        "RuleBasedBaseline"
    }
}
