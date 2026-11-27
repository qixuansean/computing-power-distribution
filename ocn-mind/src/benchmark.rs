use crate::runtime::MindProvider;
use crate::schema::{TaskDescriptor, NodeSnapshot, ResourcePredictionLabel};
use std::time::Instant;

pub struct BenchmarkResult {
    pub provider_name: String,
    pub task_id: uuid::Uuid,
    pub inference_latency_ms: f64,
    pub success: bool,
}

pub struct BenchmarkFramework {
    providers: Vec<Box<dyn MindProvider>>,
}

impl BenchmarkFramework {
    pub fn new() -> Self {
        Self {
            providers: vec![],
        }
    }

    pub fn add_provider(&mut self, provider: Box<dyn MindProvider>) {
        self.providers.push(provider);
    }

    pub async fn run_benchmark(&self, task: &TaskDescriptor, nodes: &[NodeSnapshot]) -> Vec<BenchmarkResult> {
        let mut results = vec![];
        for provider in &self.providers {
            let start = Instant::now();
            let res = provider.predict_scheduling(task, nodes).await;
            let latency = start.elapsed().as_secs_f64() * 1000.0;
            
            results.push(BenchmarkResult {
                provider_name: provider.name().to_string(),
                task_id: task.task_id,
                inference_latency_ms: latency,
                success: res.is_ok(),
            });
        }
        results
    }
}
