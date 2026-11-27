# OCN Mind 1B Architecture Specification

## 1. Proposed Hybrid Architecture Prototype
- **Shared Feature Encoder (400M)**: Extracts common features from task and node states.
- **Task Understanding Head (150M)**: Classifies and decomposes complex tasks.
- **Resource Prediction Head (150M)**: Predicts CPU/Memory/GPU/Network usage.
- **Device Matching Head (120M)**: Ranks nodes based on capability and constraints.
- **Scheduling Policy Head (100M)**: Decides the final scheduling action.
- **Decision Evaluation Head (80M)**: Self-evaluates the confidence of the decision.

## 2. Parameter Budgeting
- Total Budget: ~1B Parameters.
- Configurable modules to allow experimentation with smaller/larger heads.
