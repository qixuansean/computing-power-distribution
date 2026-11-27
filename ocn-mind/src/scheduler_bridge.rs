use crate::interfaces::*;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

pub struct PolicyEngine {
    max_cpu_usage: f32,
    max_memory_mb: u64,
    min_confidence: f32,
}

impl PolicyEngine {
    pub fn new(max_cpu_usage: f32, max_memory_mb: u64, min_confidence: f32) -> Self {
        Self {
            max_cpu_usage,
            max_memory_mb,
            min_confidence,
        }
    }

    pub fn validate_recommendation(
        &self,
        request: &SchedulingRequest,
        recommendation: &SchedulingRecommendation,
    ) -> Result<(), MindError> {
        // Validate confidence
        if request.analysis.confidence < self.min_confidence {
            return Err(MindError::ValidationFailed(format!(
                "Task analysis confidence {} is below minimum required {}",
                request.analysis.confidence, self.min_confidence
            )));
        }

        // Validate resource estimates against hard limits
        if request.analysis.estimate.cpu_usage > 1.0 || request.analysis.estimate.cpu_usage < 0.0 {
            return Err(MindError::ValidationFailed(
                "CPU usage out of bounds [0.0, 1.0]".into(),
            ));
        }

        // Validate action consistency
        match &recommendation.action {
            SchedulingAction::LocalExecute => {
                if request.analysis.estimate.cpu_usage > request.budget.max_cpu {
                    return Err(MindError::ValidationFailed(
                        "Local execution recommended but CPU usage exceeds budget".into(),
                    ));
                }
                if request.analysis.estimate.memory_mb > request.budget.max_memory_mb {
                    return Err(MindError::ValidationFailed(
                        "Local execution recommended but memory usage exceeds budget".into(),
                    ));
                }
            }
            SchedulingAction::RemoteOverflow { target_node } => {
                if !request.available_nodes.contains(target_node) {
                    return Err(MindError::ValidationFailed(format!(
                        "Recommended target node {} is not in available nodes list",
                        target_node
                    )));
                }
            }
            SchedulingAction::PauseBackground => {
                // Allowed for high-priority tasks
            }
            SchedulingAction::RejectTask { reason } => {
                if reason.is_empty() {
                    return Err(MindError::ValidationFailed(
                        "Rejection reason cannot be empty".into(),
                    ));
                }
            }
        }

        Ok(())
    }
}

pub struct OcnSchedulerBridge {
    provider: Arc<dyn MindProvider>,
    policy_engine: Arc<PolicyEngine>,
    timeout_duration: Duration,
}

impl OcnSchedulerBridge {
    pub fn new(
        provider: Arc<dyn MindProvider>,
        policy_engine: Arc<PolicyEngine>,
        timeout_ms: u64,
    ) -> Self {
        Self {
            provider,
            policy_engine,
            timeout_duration: Duration::from_millis(timeout_ms),
        }
    }

    pub async fn analyze_task(
        &self,
        request: TaskAnalysisRequest,
    ) -> Result<TaskAnalysisResult, MindError> {
        let provider = self.provider.clone();
        let fut = async move { provider.analyze_task(request).await };

        match timeout(self.timeout_duration, fut).await {
            Ok(res) => res,
            Err(_) => Err(MindError::Timeout),
        }
    }

    pub async fn recommend_schedule(
        &self,
        request: SchedulingRequest,
    ) -> Result<SchedulingRecommendation, MindError> {
        let provider = self.provider.clone();
        let req_clone = request.clone();
        let fut = async move { provider.recommend_schedule(req_clone).await };

        let recommendation = match timeout(self.timeout_duration, fut).await {
            Ok(res) => res?,
            Err(_) => return Err(MindError::Timeout),
        };

        // Enforce Policy Engine validation
        self.policy_engine
            .validate_recommendation(&request, &recommendation)?;

        Ok(recommendation)
    }

    pub fn update_provider(&mut self, provider: Arc<dyn MindProvider>) {
        self.provider = provider;
    }
}
