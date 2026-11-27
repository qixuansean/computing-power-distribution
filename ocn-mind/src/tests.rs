#[cfg(test)]
mod tests {
    use crate::{
        NodeId, TaskId, TaskType, ResourceEstimate, ResourceBudget, SchedulingAction,
        MindProvider, RuleBasedProvider, TaskAnalysisRequest, TaskAnalysisResult, SchedulingRequest, SchedulingRecommendation,
        PolicyEngine, OcnSchedulerBridge, MindError,
    };
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;
    use async_trait::async_trait;

    #[tokio::test]
    async fn test_rule_based_analyze_task() {
        let provider = RuleBasedProvider::new(NodeId("node-1".into()));
        let request = TaskAnalysisRequest {
            task_id: TaskId("task-1".into()),
            task_type: TaskType::Compute,
            description: "Heavy computation".into(),
            metadata: HashMap::new(),
        };

        let result = provider.analyze_task(request).await.unwrap();

        assert_eq!(result.estimate.cpu_usage, 0.5);
        assert_eq!(result.estimate.memory_mb, 512);
        assert_eq!(result.confidence, 0.9);
    }

    #[tokio::test]
    async fn test_rule_based_recommend_schedule_local() {
        let provider = RuleBasedProvider::new(NodeId("node-1".into()));
        let request = TaskAnalysisRequest {
            task_id: TaskId("task-1".into()),
            task_type: TaskType::Compute,
            description: "Light task".into(),
            metadata: HashMap::new(),
        };
        let analysis = provider.analyze_task(request).await.unwrap();

        let sched_req = SchedulingRequest {
            task_id: TaskId("task-1".into()),
            analysis,
            budget: ResourceBudget {
                max_cpu: 0.8,
                max_memory_mb: 1024,
                allow_gpu: false,
            },
            available_nodes: vec![NodeId("node-2".into())],
        };

        let recommendation = provider.recommend_schedule(sched_req).await.unwrap();
        assert_eq!(recommendation.action, SchedulingAction::LocalExecute);
        assert_eq!(recommendation.score, 1.0);
    }

    #[tokio::test]
    async fn test_rule_based_recommend_schedule_overflow() {
        let provider = RuleBasedProvider::new(NodeId("node-1".into()));
        let request = TaskAnalysisRequest {
            task_id: TaskId("task-1".into()),
            task_type: TaskType::Gaming,
            description: "Gaming task".into(),
            metadata: HashMap::new(),
        };
        let analysis = provider.analyze_task(request).await.unwrap();

        let sched_req = SchedulingRequest {
            task_id: TaskId("task-1".into()),
            analysis,
            budget: ResourceBudget {
                max_cpu: 0.5,
                max_memory_mb: 1024,
                allow_gpu: false,
            },
            available_nodes: vec![NodeId("node-2".into())],
        };

        let recommendation = provider.recommend_schedule(sched_req).await.unwrap();
        assert_eq!(
            recommendation.action,
            SchedulingAction::RemoteOverflow {
                target_node: NodeId("node-2".into())
            }
        );
        assert_eq!(recommendation.score, 0.8);
    }

    #[tokio::test]
    async fn test_policy_engine_validation() {
        let policy = PolicyEngine::new(0.8, 2048, 0.8);
        let analysis = TaskAnalysisResult {
            estimate: ResourceEstimate {
                cpu_usage: 0.5,
                memory_mb: 512,
                gpu_required: false,
                confidence: 0.9,
            },
            metadata: HashMap::new(),
            confidence: 0.9,
        };

        let sched_req = SchedulingRequest {
            task_id: TaskId("task-1".into()),
            analysis,
            budget: ResourceBudget {
                max_cpu: 0.8,
                max_memory_mb: 1024,
                allow_gpu: false,
            },
            available_nodes: vec![NodeId("node-2".into())],
        };

        let rec = SchedulingRecommendation {
            action: SchedulingAction::LocalExecute,
            score: 1.0,
        };

        assert!(policy.validate_recommendation(&sched_req, &rec).is_ok());
    }

    #[tokio::test]
    async fn test_policy_engine_rejection_invalid_target() {
        let policy = PolicyEngine::new(0.8, 2048, 0.8);
        let analysis = TaskAnalysisResult {
            estimate: ResourceEstimate {
                cpu_usage: 0.9,
                memory_mb: 512,
                gpu_required: false,
                confidence: 0.9,
            },
            metadata: HashMap::new(),
            confidence: 0.9,
        };

        let sched_req = SchedulingRequest {
            task_id: TaskId("task-1".into()),
            analysis,
            budget: ResourceBudget {
                max_cpu: 0.8,
                max_memory_mb: 1024,
                allow_gpu: false,
            },
            available_nodes: vec![NodeId("node-2".into())],
        };

        let rec = SchedulingRecommendation {
            action: SchedulingAction::RemoteOverflow {
                target_node: NodeId("node-99".into()), // Not in available nodes
            },
            score: 0.8,
        };

        assert!(policy.validate_recommendation(&sched_req, &rec).is_err());
    }

    #[tokio::test]
    async fn test_scheduler_bridge_timeout() {
        struct SlowProvider;
        #[async_trait]
        impl MindProvider for SlowProvider {
            async fn analyze_task(&self, _: TaskAnalysisRequest) -> Result<TaskAnalysisResult, MindError> {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Err(MindError::Internal("Should not reach".into()))
            }
            async fn recommend_schedule(&self, _: SchedulingRequest) -> Result<SchedulingRecommendation, MindError> {
                Ok(SchedulingRecommendation { action: SchedulingAction::LocalExecute, score: 1.0 })
            }
            async fn load_model(&self, _: &str) -> Result<(), MindError> { Ok(()) }
            async fn unload_model(&self) -> Result<(), MindError> { Ok(()) }
            fn status(&self) -> String { "Slow".into() }
            fn provider_name(&self) -> String { "Slow".into() }
            fn provider_version(&self) -> String { "1.0".into() }
            fn is_loaded(&self) -> bool { true }
        }

        let provider = Arc::new(SlowProvider);
        let policy = Arc::new(PolicyEngine::new(1.0, 4096, 0.0));
        let bridge = OcnSchedulerBridge::new(provider, policy, 10); // 10ms timeout

        let request = TaskAnalysisRequest {
            task_id: TaskId("task-1".into()),
            task_type: TaskType::Compute,
            description: "Slow task".into(),
            metadata: HashMap::new(),
        };

        let res = bridge.analyze_task(request).await;
        assert!(matches!(res, Err(MindError::Timeout)));
    }
}
