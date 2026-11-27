use crate::interfaces::*;
use async_trait::async_trait;
use std::collections::HashMap;

pub struct RuleBasedProvider {
    node_id: NodeId,
    config: ProviderConfig,
}

impl RuleBasedProvider {
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            config: ProviderConfig {
                name: "RuleBased".into(),
                version: "1.0.0".into(),
                timeout_ms: 5000,
                max_retries: 3,
                enable_cache: true,
            },
        }
    }

    pub fn with_config(node_id: NodeId, config: ProviderConfig) -> Self {
        Self { node_id, config }
    }

    fn validate_output(&self, result: &TaskAnalysisResult) -> Result<(), MindError> {
        // Validate confidence is in range [0.0, 1.0]
        if result.confidence < 0.0 || result.confidence > 1.0 {
            return Err(MindError::ValidationFailed(
                "Confidence must be in range [0.0, 1.0]".into(),
            ));
        }

        // Validate CPU usage is in range [0.0, 1.0]
        if result.estimate.cpu_usage < 0.0 || result.estimate.cpu_usage > 1.0 {
            return Err(MindError::ValidationFailed(
                "CPU usage must be in range [0.0, 1.0]".into(),
            ));
        }

        Ok(())
    }
}

#[async_trait]
impl MindProvider for RuleBasedProvider {
    async fn analyze_task(&self, request: TaskAnalysisRequest) -> Result<TaskAnalysisResult, MindError> {
        // Simple heuristic: Compute tasks need 0.5 CPU, Storage needs 100MB
        let (cpu, mem) = match request.task_type {
            TaskType::Compute => (0.5, 512),
            TaskType::Storage => (0.1, 1024),
            TaskType::MemoryOverflow => (0.05, 2048),
            TaskType::Inference => (0.3, 1024),
            TaskType::Gaming => (0.8, 4096),
        };

        let estimate = ResourceEstimate {
            cpu_usage: cpu,
            memory_mb: mem,
            gpu_required: request.task_type == TaskType::Gaming,
            confidence: 0.9,
        };

        let mut metadata = HashMap::new();
        metadata.insert("provider".into(), self.config.name.clone());
        metadata.insert("task_type".into(), format!("{:?}", request.task_type));
        metadata.insert("description".into(), request.description.clone());

        let result = TaskAnalysisResult {
            estimate,
            metadata,
            confidence: 0.9,
        };

        self.validate_output(&result)?;
        Ok(result)
    }

    async fn recommend_schedule(&self, request: SchedulingRequest) -> Result<SchedulingRecommendation, MindError> {
        // If local budget is enough, execute locally
        if request.analysis.estimate.cpu_usage <= request.budget.max_cpu &&
           request.analysis.estimate.memory_mb <= request.budget.max_memory_mb {
            Ok(SchedulingRecommendation {
                action: SchedulingAction::LocalExecute,
                score: 1.0,
            })
        } else if !request.available_nodes.is_empty() {
            // Otherwise, overflow to first available node
            Ok(SchedulingRecommendation {
                action: SchedulingAction::RemoteOverflow {
                    target_node: request.available_nodes[0].clone(),
                },
                score: 0.8,
            })
        } else {
            Ok(SchedulingRecommendation {
                action: SchedulingAction::RejectTask {
                    reason: "Insufficient local resources and no remote nodes".into(),
                },
                score: 0.0,
            })
        }
    }

    async fn load_model(&self, _path: &str) -> Result<(), MindError> {
        Ok(()) // Static rules don't need loading
    }

    async fn unload_model(&self) -> Result<(), MindError> {
        Ok(())
    }

    fn status(&self) -> String {
        format!("{} v{} (Active)", self.config.name, self.config.version)
    }

    fn provider_name(&self) -> String {
        self.config.name.clone()
    }

    fn provider_version(&self) -> String {
        self.config.version.clone()
    }

    fn is_loaded(&self) -> bool {
        true
    }
}
