# OCN AI Interface Specification (OCN Mind 2.0)

## 1. Executive Summary

OCN Mind 2.0 defines a robust, decoupled, and extensible AI provider interface (`MindProvider`) and scheduler bridge (`OcnSchedulerBridge`) for OCN nodes. This specification enables pluggable AI backends—ranging from deterministic rule-based heuristics (`RuleBasedProvider`) to lightweight and full pre-trained models—without requiring any modifications to OCN Core's QUIC, TLS 1.3, or authentication layers.

---

## 2. Core AI Interface (`MindProvider`)

All AI providers implement the `MindProvider` trait using `async_trait` for asynchronous inference and model management:

```rust
#[async_trait]
pub trait MindProvider: Send + Sync {
    /// Analyze task characteristics and resource requirements
    async fn analyze_task(&self, request: TaskAnalysisRequest)
        -> Result<TaskAnalysisResult, MindError>;

    /// Recommend scheduling decision based on analysis and available resources
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
```

---

## 3. Data Structures & Protocols

### 3.1 Identifiers
- **`NodeId(pub String)`**: Unique node identifier.
- **`TaskId(pub String)`**: Unique task identifier.

### 3.2 Task Analysis
- **`TaskAnalysisRequest`**: Contains `task_id`, `task_type` (`Compute`, `Storage`, `MemoryOverflow`, `Inference`, `Gaming`), `description`, and `metadata`.
- **`TaskAnalysisResult`**: Contains `estimate` (`ResourceEstimate`: `cpu_usage`, `memory_mb`, `gpu_required`, `confidence`), `metadata`, and overall `confidence`.

### 3.3 Scheduling
- **`SchedulingRequest`**: Contains `task_id`, `analysis`, `budget` (`ResourceBudget`: `max_cpu`, `max_memory_mb`, `allow_gpu`), and `available_nodes`.
- **`SchedulingRecommendation`**: Contains `action` (`SchedulingAction`: `LocalExecute`, `RemoteOverflow { target_node }`, `PauseBackground`, `RejectTask { reason }`) and `score`.

---

## 4. Safety & Policy Engine

To prevent unvalidated AI decisions from compromising node stability, **all AI recommendations must pass through the `PolicyEngine`**:

1. **Confidence Thresholding**: Task analysis confidence must meet or exceed the minimum threshold (`min_confidence`).
2. **Resource Budget Validation**: Local execution recommendations are checked against local CPU and memory budgets.
3. **Target Node Validation**: Overflow recommendations are verified to ensure the target node exists in the active `available_nodes` list.
4. **Action Consistency**: Rejection reasons and background pause actions are structurally validated.

---

## 5. Resilience & Fallbacks

- **Timeout Control**: All AI provider invocations are wrapped in strict asynchronous timeouts (`OcnSchedulerBridge`, default 5000ms). If a model hangs or takes too long, a `MindError::Timeout` is returned.
- **Rule-Based Fallback**: When models are unavailable, unloads, or timeouts occur, nodes seamlessly fall back to `RuleBasedProvider` for deterministic heuristic scheduling.
- **Error Handling**: Comprehensive `MindError` variants (`Timeout`, `ModelNotLoaded`, `InvalidOutput`, `Internal`, `ValidationFailed`) prevent model failures from crashing the OCN node.
