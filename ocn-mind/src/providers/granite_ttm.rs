use crate::interfaces::*;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::process::{Command, Child};
use tokio::io::{AsyncWriteExt, AsyncBufReadExt, BufReader};
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PythonRequest {
    command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    history: Option<Vec<f32>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PythonResponse {
    status: String,
    #[serde(default)]
    prediction: Vec<f32>,
    #[serde(default)]
    latency_ms: f64,
    #[serde(default)]
    message: String,
}

pub struct GraniteTTMProvider {
    node_id: NodeId,
    config: ProviderConfig,
    model_loaded: AtomicBool,
    python_process: Arc<Mutex<Option<Child>>>,
}

impl GraniteTTMProvider {
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            config: ProviderConfig {
                name: "GraniteTTM".into(),
                version: "2.0.0".into(),
                timeout_ms: 5000,
                max_retries: 3,
                enable_cache: true,
            },
            model_loaded: AtomicBool::new(false),
            python_process: Arc::new(Mutex::new(None)),
        }
    }

    async fn ensure_python_runtime(&self) -> Result<(), MindError> {
        let mut proc_guard = self.python_process.lock().await;
        if proc_guard.is_some() {
            return Ok(());
        }

        let script_path = if std::path::Path::new("ai/adapters/granite_ttm_adapter/runtime_service.py").exists() {
            "ai/adapters/granite_ttm_adapter/runtime_service.py"
        } else if std::path::Path::new("../ai/adapters/granite_ttm_adapter/runtime_service.py").exists() {
            "../ai/adapters/granite_ttm_adapter/runtime_service.py"
        } else {
            "C:/Users/qixua/Desktop/OCN/ai/adapters/granite_ttm_adapter/runtime_service.py"
        };

        let child = Command::new("python")
            .arg(script_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .map_err(|e| MindError::Internal(format!("Failed to start Python runtime: {}", e)))?;

        *proc_guard = Some(child);
        self.model_loaded.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn call_python(&self, req: PythonRequest) -> Result<PythonResponse, MindError> {
        self.ensure_python_runtime().await?;
        
        let mut proc_guard = self.python_process.lock().await;
        let child = proc_guard.as_mut().unwrap();
        
        let stdin = child.stdin.as_mut().ok_or_else(|| MindError::Internal("No stdin".into()))?;
        let stdout = child.stdout.as_mut().ok_or_else(|| MindError::Internal("No stdout".into()))?;
        let mut reader = BufReader::new(stdout).lines();

        let req_json = serde_json::to_string(&req).map_err(|e| MindError::Internal(e.to_string()))?;
        stdin.write_all(format!("{}\n", req_json).as_bytes()).await
            .map_err(|e| MindError::Internal(format!("Pipe write error: {}", e)))?;
        stdin.flush().await.map_err(|e| MindError::Internal(e.to_string()))?;

        if let Some(line) = reader.next_line().await.map_err(|e| MindError::Internal(e.to_string()))? {
            let resp: PythonResponse = serde_json::from_str(&line).map_err(|e| MindError::Internal(e.to_string()))?;
            if resp.status == "success" {
                Ok(resp)
            } else {
                Err(MindError::Internal(resp.message))
            }
        } else {
            Err(MindError::Internal("Python runtime exited prematurely".into()))
        }
    }
}

#[async_trait]
impl MindProvider for GraniteTTMProvider {
    async fn analyze_task(&self, request: TaskAnalysisRequest) -> Result<TaskAnalysisResult, MindError> {
        // Use a dummy history for analysis if none provided in metadata
        let history = if let Some(h_str) = request.metadata.get("history") {
            serde_json::from_str(h_str).unwrap_or_else(|_| vec![0.3; 512])
        } else {
            vec![0.3; 512]
        };

        let py_resp = self.call_python(PythonRequest {
            command: "predict".into(),
            history: Some(history),
        }).await?;

        let avg_predicted = py_resp.prediction.iter().sum::<f32>() / 96.0;

        let mut metadata = HashMap::new();
        metadata.insert("provider".into(), "GraniteTTM".into());
        metadata.insert("inference_latency_ms".into(), py_resp.latency_ms.to_string());
        metadata.insert("predicted_points".into(), py_resp.prediction.len().to_string());

        Ok(TaskAnalysisResult {
            estimate: ResourceEstimate {
                cpu_usage: avg_predicted,
                memory_mb: 1024, // Simplified
                gpu_required: false,
                confidence: 0.88,
            },
            metadata,
            confidence: 0.88,
        })
    }

    async fn recommend_schedule(&self, request: SchedulingRequest) -> Result<SchedulingRecommendation, MindError> {
        if request.analysis.estimate.cpu_usage <= request.budget.max_cpu {
            Ok(SchedulingRecommendation {
                action: SchedulingAction::LocalExecute,
                score: 0.9,
            })
        } else if !request.available_nodes.is_empty() {
            Ok(SchedulingRecommendation {
                action: SchedulingAction::RemoteOverflow {
                    target_node: request.available_nodes[0].clone(),
                },
                score: 0.7,
            })
        } else {
            Ok(SchedulingRecommendation {
                action: SchedulingAction::RejectTask { reason: "Overloaded".into() },
                score: 0.0,
            })
        }
    }

    async fn load_model(&self, _path: &str) -> Result<(), MindError> {
        self.ensure_python_runtime().await
    }

    async fn unload_model(&self) -> Result<(), MindError> {
        let mut proc_guard = self.python_process.lock().await;
        if let Some(mut child) = proc_guard.take() {
            let _ = child.kill().await;
        }
        self.model_loaded.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn status(&self) -> String {
        format!("GraniteTTM (Loaded: {})", self.model_loaded.load(Ordering::SeqCst))
    }

    fn provider_name(&self) -> String { "GraniteTTM".into() }
    fn provider_version(&self) -> String { "2.0.0".into() }
    fn is_loaded(&self) -> bool { self.model_loaded.load(Ordering::SeqCst) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_granite_ttm_full_chain() {
        let provider = GraniteTTMProvider::new(NodeId("node-1".into()));
        
        // Test Task Analysis (should trigger Python start)
        let request = TaskAnalysisRequest {
            task_id: TaskId("task-real".into()),
            task_type: TaskType::Compute,
            description: "Real TTM Test".into(),
            metadata: HashMap::new(),
        };

        let result = provider.analyze_task(request).await;
        
        match result {
            Ok(res) => {
                println!("TTM Analysis Result: {:?}", res);
                assert!(res.estimate.cpu_usage >= 0.0);
                assert_eq!(res.metadata.get("provider"), Some(&"GraniteTTM".to_string()));
                assert!(res.metadata.contains_key("inference_latency_ms"));
            },
            Err(e) => {
                panic!("TTM chain failed: {}", e);
            }
        }

        // Test Unload
        provider.unload_model().await.unwrap();
        assert!(!provider.is_loaded());
    }
}


