# OCN Milestone MS-Compute-Loopback: Implementation Plan

## 1. Scope & Design
- **Goal**: End-to-end task execution loopback between Client and Server over QUIC after B-2 auth.
- **Protocol**: Add `TaskRequest` (MsgType 10) and `TaskResult` (MsgType 11) to `OCNMsgType` in `ocn-core/src/proto.rs`.
- **Execution**: Server receives `TaskRequest`, invokes `ocn_compute::runtime::ComputeExecutor::execute(&task)`, and returns `TaskResult`.
- **Validation**: Client sends task, awaits response, validates `result_hash` and payload content.
- **Safety**: B-2 production auth logic unchanged. B-3 untouched. No Android/Git/backup changes.

## 2. Implementation Steps
1. **Extend `OCNMsgType`** in `ocn-core/src/proto.rs` with `TaskRequest` and `TaskResult` variants (or serializable payload structs).
2. **Add integration test** in `ocn-core/tests/compute_loopback.rs` (or `ocn-compute` integration tests) that spins up a QUIC server, authenticates via B-2, sends a BatchHash task, and verifies the result.
3. **Run tests** and verify pass.
