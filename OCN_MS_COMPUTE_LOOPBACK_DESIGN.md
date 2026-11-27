# OCN Milestone MS-Compute-Loopback: Implementation Plan & Minimal Change Design

**Milestone**: `MS-Compute-Loopback`  
**Goal**: Establish end-to-end task execution loopback between Client (Windows A) and Server (Windows B) over QUIC after successful B-2 authentication.  
**Flow**: Client sends `TaskDescriptor` (BatchHash) over QUIC bidirectional stream → Server receives, validates, executes via `ComputeExecutor`, and returns `ExecutionResult` → Client receives and validates `result_hash` and computation correctness.

---

## 1. Minimal Change Design & Architecture

### 1.1 Protocol Extension (`ocn-core/src/proto.rs`)
- Add explicit MsgType variants for task execution with stable wire IDs or reuse existing extension patterns:
  - `TaskRequest { task_id: String, payload: Vec<u8>, ... }` or directly serialize `ocn_compute::task::TaskDescriptor` / `ExecutionResult` wrapped in `OCNMsgType`.
  - To keep `ocn-core` independent of heavy compute logic if desired, or add `ocn-compute` as a dependency (or define light DTOs in `ocn-core` / serialize bincode).
  - Let's check dependency relationships between `ocn-core` and `ocn-compute`.

Let's check Cargo.toml dependencies across workspace:
