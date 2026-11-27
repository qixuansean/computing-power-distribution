# OCN Extended Work Session — Progress Log

**Session start:** 2026-11-23 19:19:56 (local)  
**Session end:** 2026-11-23 19:26:30 (local)  
**Mode:** Single worker; read-only audit + planning. No business-code edits, no Git operations, no deletions/moves, no report overwrites.  
**Checkpoint policy:** one checkpoint per completed phase (cadence was phase-based, not wall-clock 20 min; actual timestamps below are exact).

---

## Checkpoint 01 — 19:19:56 — Baseline

### Completed
- Recorded environment before any work.
- Confirmed `git` / `git-lfs` availability and disk headroom.
- Verified the model checksum chain.

### Commands and results
| Command | Output | Exit |
|---|---|---|
| `Get-Date -Format ...` | `2026-11-23 19:19:56` | 0 |
| `Get-PSDrive C` | Free `116,953,866,240` B (108.9 GB) | 0 |
| `git lfs version` | `git-lfs/3.8.1` | 0 |
| `git --version` | `2.56.1.windows.1` | 0 |
| `Get-FileHash ai/models/granite-ttm/model/model.safetensors` | `A706726A7EB01BBCB42994B7DCB3C06EA9557898DBAE8D480EB04FE8CCB89710` | 0 |

### Findings
- `model.safetensors` (3,240,592 B) checksum matches both `ai/manifests/model_checksums.json` and `ai/models/granite-ttm/config/model_config.json`.
- `android/local.properties` does not exist.
- `gradle-wrapper.jar` = 63,375 B.
- `build_arm64.ps1` rebuilds the `.so` via `cargo build --target aarch64-linux-android --features android-jni --release -p ocn-core`.

### Failures
- None.

### Next
- Phase A artifact matrix; Phase B pipeline trace.

---

## Checkpoint 02 — 19:21:40 — Phase A (version control) and Phase B (static trace)

### Completed
- Re-read `OCN_SOURCE_CONTROL_PLAN.md` (199 lines) and validated its facts against the live tree.
- Read every Phase 3.1 source file: `task.rs`, `runtime.rs`, `resource_manager.rs`, `deduplication.rs`, `dispatcher.rs`, `engine.rs`, `messages.rs`, `proto.rs`, `quic.rs`, `transport.rs`, `auth.rs`, `identity.rs`, `config.rs`, `metrics.rs`, `discovery.rs`, `ocn-node/src/main.rs` (1-304), `jni_api.rs`, `OcnNode.java`, `MainActivity.java`, Android Gradle files.
- Built the 6-hop pipeline table.

### Key results
| Item | Status |
|---|---|
| `TaskDescriptor` + validation | `VERIFIED` (`task.rs:19-30`, `:59-128`) |
| `ExecutionResult` + validation | `VERIFIED` (`task.rs:40-48`, `:132-153`) |
| `TaskDispatcher` | `MISSING` (10-line stub, `dispatcher.rs:4-10`) |
| `ComputeEngine` | `MISSING` (10-line stub, `engine.rs:4-10`) |
| `TaskRequestDto` / `TaskResultDto` | `MISSING` (repo-wide grep = 0; no `dto.rs`) |
| Msg types 11/12 | `MISSING` |
| QUIC handshake code | present (`quic.rs:96-362`) |
| Server post-auth handoff | `FAILED` design (synthetic `AuthResponse` then connection dropped, `quic.rs:244-251`) |
| Android task JNI | `MISSING` (`jni_api.rs:9-17`, `OcnNode.java:8`) |
| Payload ceilings | `PARTIAL` — 64 KB / 2 MB / 10 MB disagree |

### Additional observations
- Two wire formats coexist: 12-byte `FixedHeader` for handshake; `u32` length + bincode for `ConnectionHandle::send/recv` (`quic.rs:57-87`).
- `send()` opens one bi-stream per message and finishes it.
- If all 5 client connect attempts fail, `main` returns `Ok(())` (`main.rs:199,303`) — exit code cannot signal failure.
- Workspace tests (from the earlier run, reused): 66 passed, exit 0. No test covers QUIC.

### Failures
- None (read-only).

### Next
- Runtime verification of the loopback handshake; Phase C.

---

## Checkpoint 03 — 19:22:30 — Runtime verification (new evidence)

### Completed
- Ran the already-built `target\debug\ocn-node.exe` as two loopback nodes on ports 9310/9311. No rebuild, no Git action.
- Started the two processes from this session and stopped only those two afterwards.

### Commands and result
```
target\debug\ocn-node.exe --node-id node-A --listen 127.0.0.1:9310
target\debug\ocn-node.exe --node-id node-B --listen 127.0.0.1:9311 --peer 127.0.0.1:9310
```

Server output (excerpt):
```
Incoming QUIC connection accepted from 127.0.0.1:62572
Authentication failed for client 'node-B'
... repeated 5 times ...
```

Client output (excerpt):
```
Connected to server at 127.0.0.1:9310
Connection attempt 1/5 failed: closed by peer: 0. Retrying in 1s...
... repeated 5 times ...
```

### Root cause (static confirmation)
- Client proof uses `server_node_id = expected_peer_fingerprint.unwrap_or("node-server")` (`quic.rs:317`).
- Server proof uses `identity.node_id` (`quic.rs:206`).
- With no fingerprint: `"node-server"` vs `"node-A"` → HMAC mismatch. With a fingerprint: a 64-hex string vs `"node-A"` → still mismatch.
- Therefore **no configuration can authenticate**. `FAILED`.

### Consequences
- Legacy Echo / Hash / RTT / Throughput tests never execute.
- The earlier claim "loopback-only validation completed" is not supported.

### Failures
- The loopback run itself failed as described (this is the finding, not a harness error).

### Next
- Phase C.

---

## Checkpoint 04 — 19:24:10 — Phase C (Mind / Policy / TTM)

### Completed
- Read `ocn-mind` sources, orphan modules, and all AI adapter files.
- Located the exact cause of the dead policy fields and the nature of the "TTM" inference.

### Key results
| Item | Evidence | Status |
|---|---|---|
| `PolicyEngine.max_cpu_usage`/`max_memory_mb` dead | `scheduler_bridge.rs:5-18,20-75`; compiler warning | `FAILED` |
| Only `request.budget` used | `:43,48` | `VERIFIED` |
| Rust-side TTM clamp | absent (`granite_ttm.rs:122`) | `PARTIAL` |
| Python-side clamp | `runtime_service.py:76-77` | `VERIFIED` |
| Fallback to `RuleBasedProvider` | absent (`scheduler_bridge.rs:118-121`) | `MISSING` |
| "TTM inference" is simulated | `runtime_service.py:64-72`; `test_inference.py:45,60-70`; `rust_adapter.rs:43-72` | `FAILED` (claim) |
| `DataSource` provenance type | `schema.rs:6-11`, `baselines/rule_based.rs:22`; not compiled | `MISSING` in live path |
| 512-length contract | enforced only in Python (`runtime_service.py:53-54`) | `PARTIAL` |
| Device OCN evidence | logs contain no OCN lines | `UNVERIFIED` |

### Correction to an earlier draft of this log
An earlier draft stated the full-chain test used "real inference". That was wrong. The test loads real weights and spawns the real Python process, but `runtime_service.py:64` explicitly says **"Simulated prediction using trend + model weights"** and never runs a TTM forward pass. The log and audit now reflect the simulated nature.

### Failures
- None (read-only).

### Next
- Phase D and deliverables.

---

## Checkpoint 05 — 19:26:30 — Phase D and final deliverables

### Completed
- Built the dependency graph and 6-layer test plan with success/failure criteria.
- Wrote:
  - `OCN_EXTENDED_AUDIT.md` (19,944 B)
  - `OCN_PHASE31_IMPLEMENTATION_PLAN.md` (13,197 B)
  - `OCN_PHASE32_REPAIR_PLAN.md` (11,083 B)
  - this log (rewritten with accurate timestamps)

### Commands
- No builds or tests re-run. Reused the verified `cargo test --workspace` result (66 passed, 0 failed, exit 0).

### Failures
- None.

### Open items requiring approval
1. Phase A: confirm Plan A, model-weight strategy, `.so` strategy, `.gitkeep`, backup location, remote.
2. Phase 3.1: approve D1–D7 and the wire format before B0.
3. Phase 3.2: approve effective-ceiling behavior, fallback thresholds, TTM honesty strategy.

---

## Session Summary

| Phase | Outcome |
|---|---|
| A — version control | Final artifact matrix and coexistence plan; nothing executed |
| B — task pipeline | 2 hops verified, 4 missing; transport `FAILED` on loopback (new runtime evidence) |
| C — Mind/Policy | 2 logic defects, 1 honesty defect; root causes located |
| D — test design | 6 layers with gates and recovery rules |

**Guardrails honored:** no business-code edits; no `git init/add/commit/reset/clean`; no deletions, moves, or overwrites of existing reports; no unknown process termination; no system/registry/power changes.
