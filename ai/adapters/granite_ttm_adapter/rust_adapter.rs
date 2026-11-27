use serde::{Serialize, Deserialize};
use std::collections::HashMap;

/// Represents the resource prediction output from IBM Granite TTM
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TtmResourcePrediction {
    pub predicted_cpu_usage: f32,      // 0.0 to 1.0
    pub predicted_memory_mb: u64,
    pub predicted_gpu_usage: Option<f32>,
    pub confidence: f32,               // 0.0 to 1.0
    pub forecast_horizon_seconds: u32,
}

/// Represents the task metadata extracted from OCN task description
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TtmTaskContext {
    pub task_type: String,
    pub task_size_bytes: u64,
    pub previous_cpu_history: Vec<f32>,
    pub previous_memory_history: Vec<u64>,
    pub current_network_latency_ms: f32,
    pub current_network_bandwidth_mbps: f32,
}

/// TTM Model Adapter for OCN Mind Provider Interface
pub struct GraniteTTMProvider {
    model_loaded: bool,
    task_context_cache: HashMap<String, TtmTaskContext>,
    base_cpu_prediction: f32,
    base_memory_prediction: u64,
}

impl GraniteTTMProvider {
    pub fn new() -> Self {
        Self {
            model_loaded: false,
            task_context_cache: HashMap::new(),
            base_cpu_prediction: 0.3,
            base_memory_prediction: 512,
        }
    }

    /// Simulates loading the TTM model (in production, this would load safetensors)
    pub async fn load_model(&mut self) -> Result<(), String> {
        // Simulate model loading time
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // In production: Load from ai/models/granite-ttm/model/model.safetensors
        self.model_loaded = true;
        Ok(())
    }

    /// Simulates unloading the model
    pub async fn unload_model(&mut self) -> Result<(), String> {
        self.model_loaded = false;
        self.task_context_cache.clear();
        Ok(())
    }

    /// Core TTM inference simulation (uses historical patterns)
    pub fn predict_resources(&self, context: &TtmTaskContext) -> TtmResourcePrediction {
        if !self.model_loaded {
            return TtmResourcePrediction {
                predicted_cpu_usage: self.base_cpu_prediction,
                predicted_memory_mb: self.base_memory_prediction,
                predicted_gpu_usage: None,
                confidence: 0.7,
                forecast_horizon_seconds: 60,
            };
        }

        // Simulate TTM prediction based on historical patterns
        let avg_cpu = if !context.previous_cpu_history.is_empty() {
            context.previous_cpu_history.iter().sum::<f32>() / context.previous_cpu_history.len() as f32
        } else {
            0.3
        };

        // Scale prediction based on task size
        let cpu_multiplier = 1.0 + (context.task_size_bytes as f32 / 1024.0 / 1024.0 / 100.0);
        let predicted_cpu = (avg_cpu * cpu_multiplier).min(0.95);

        // Memory prediction based on task type and network conditions
        let mut memory_mb = self.base_memory_prediction;
        if context.task_type.contains("compute") {
            memory_mb = 1024;
        } else if context.task_type.contains("storage") {
            memory_mb = 512;
        } else if context.task_type.contains("gaming") {
            memory_mb = 4096;
        }

        // Network congestion increases memory pressure
        if context.current_network_latency_ms > 100.0 {
            memory_mb = memory_mb * 2;
        }

        let confidence = if self.model_loaded { 0.85 } else { 0.7 };

        TtmResourcePrediction {
            predicted_cpu_usage: predicted_cpu,
            predicted_memory_mb: memory_mb,
            predicted_gpu_usage: Some(0.5),
            confidence,
            forecast_horizon_seconds: 60,
        }
    }

    /// Convert TTM prediction to OCN TaskAnalysisResult
    pub fn to_task_analysis_result(
        &self,
        task_id: String,
        task_type: String,
        description: String,
        prediction: TtmResourcePrediction,
    ) -> crate::interfaces::TaskAnalysisResult {
        let mut metadata = HashMap::new();
        metadata.insert("provider".into(), "GraniteTTM".into());
        metadata.insert("model".into(), "TTM-R2".into());
        metadata.insert("forecast_horizon".into(), prediction.forecast_horizon_seconds.to_string());
        metadata.insert("confidence_source".into(), "TTM".into());

        let estimate = crate::interfaces::ResourceEstimate {
            cpu_usage: prediction.predicted_cpu_usage,
            memory_mb: prediction.predicted_memory_mb,
            gpu_required: prediction.predicted_gpu_usage.unwrap_or(0.0) > 0.4,
            confidence: prediction.confidence,
        };

        crate::interfaces::TaskAnalysisResult {
            estimate,
            metadata,
            confidence: prediction.confidence,
        }
    }

    /// Convert OCN SchedulingRequest to TTM task context
    pub fn from_scheduling_request(
        &self,
        request: &crate::interfaces::SchedulingRequest,
    ) -> TtmTaskContext {
        let task_type_str = format!("{:?}", request.analysis.estimate.cpu_usage);
        let task_size_bytes = (request.analysis.estimate.cpu_usage * 1000000.0) as u64;

        // Extract task type from metadata or use default
        let task_type = request.analysis.metadata
            .get("task_type")
            .cloned()
            .unwrap_or_else(|| "compute".to_string());

        let mut previous_cpu = vec![0.3; 10]; // Simulated history
        if let Some(cpu_history) = request.analysis.metadata.get("cpu_history") {
            if let Ok(hist) = serde_json::from_str::<Vec<f32>>(cpu_history) {
                previous_cpu = hist;
            }
        }

        let mut previous_memory = vec![512; 10];
        if let Some(mem_history) = request.analysis.metadata.get("memory_history") {
            if let Ok(hist) = serde_json::from_str::<Vec<u64>>(mem_history) {
                previous_memory = hist;
            }
        }

        TtmTaskContext {
            task_type,
            task_size_bytes,
            previous_cpu_history: previous_cpu,
            previous_memory_history: previous_memory,
            current_network_latency_ms: request.analysis.metadata
                .get("network_latency_ms")
                .and_then(|v| v.parse().ok())
                .unwrap_or(10.0),
            current_network_bandwidth_mbps: request.analysis.metadata
                .get("network_bandwidth_mbps")
                .and_then(|v| v.parse().ok())
                .unwrap_or(100.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ttm_model_load() {
        let mut provider = GraniteTTMProvider::new();
        let result = provider.load_model().await;
        assert!(result.is_ok());
        assert!(provider.model_loaded);
    }

    #[tokio::test]
    async fn test_ttm_model_unload() {
        let mut provider = GraniteTTMProvider::new();
        provider.load_model().await.unwrap();
        let result = provider.unload_model().await;
        assert!(result.is_ok());
        assert!(!provider.model_loaded);
    }

    #[tokio::test]
    async fn test_ttm_resource_prediction() {
        let mut provider = GraniteTTMProvider::new();
        provider.load_model().await.unwrap();

        let context = TtmTaskContext {
            task_type: "compute".to_string(),
            task_size_bytes: 5242880, // 5MB
            previous_cpu_history: vec![0.3; 10],
            previous_memory_history: vec![512; 10],
            current_network_latency_ms: 50.0,
            current_network_bandwidth_mbps: 100.0,
        };

        let prediction = provider.predict_resources(&context);
        assert!(prediction.predicted_cpu_usage > 0.0);
        assert!(prediction.predicted_memory_mb >= 512);
        assert!(prediction.confidence > 0.0);
    }

    #[tokio::test]
    async fn test_ttm_to_task_analysis_result() {
        let mut provider = GraniteTTMProvider::new();
        provider.load_model().await.unwrap();

        let prediction = TtmResourcePrediction {
            predicted_cpu_usage: 0.5,
            predicted_memory_mb: 1024,
            predicted_gpu_usage: Some(0.6),
            confidence: 0.85,
            forecast_horizon_seconds: 60,
        };

        let result = provider.to_task_analysis_result(
            "task-1".to_string(),
            "compute".to_string(),
            "Test task".to_string(),
            prediction,
        );

        assert_eq!(result.estimate.cpu_usage, 0.5);
        assert_eq!(result.estimate.memory_mb, 1024);
        assert_eq!(result.estimate.gpu_required, true);
        assert_eq!(result.confidence, 0.85);
    }
}
