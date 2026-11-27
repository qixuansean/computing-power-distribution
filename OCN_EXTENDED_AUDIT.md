# OCN Extended Audit — Phase 3.1 / 3.2

**Date:** 2026-11-23  
**Window:** Extended single-worker session (started 19:19 local)  
**Scope:** Version-control readiness, task-pipeline trace, Mind/Policy review, layered test design  
**Rule:** Read-only. No business-code edits, no Git operations, no deletions/moves, no report overwrites.

**Evidence tags:** `VERIFIED` (directly observed) · `PARTIAL` (exists but incomplete/ineffective) · `MISSING` · `FAILED` (present and demonstrably broken) · `UNVERIFIED` (no evidence).

---

## 0. Evidence Base

### 0.1 Static evidence
All source files read in full or in cited ranges during this session (see `OCN_PROGRESS_LOG.md` for the read list). Key anchors were re-located by line number at write time.

### 0.2 Runtime evidence — live loopback QUIC run (19:22)

A two-node loopback run was executed with the already-built binary (no rebuild):

```
target\debug\ocn-node.exe --node-id node-A --listen 127.0.0.1:9310
target\debug\ocn-node.exe --node-id node-B --listen 127.0.0.1:9311 --peer 127.0.0.1:9310
```

Observed (verbatim excerpts):

```
server: Incoming QUIC connection accepted from 127.0.0.1:62572
server: Authentication failed for client 'node-B'
client: Connected to server at 127.0.0.1:9310
client: Connection attempt 1/5 failed: closed by peer: 0. Retrying in 1s...
... repeated 5 times ...
```

**Result: the authenticated QUIC path fails on loopback.** The legacy Echo / Hash / RTT / Throughput tests therefore never execute. This is `FAILED`, not `UNVERIFIED`.

Root cause (static): the client computes the auth proof with
`server_node_id = expected_peer_fingerprint.unwrap_or("node-server")` (`quic.rs:317`), while the server computes it with its real `identity.node_id` (`quic.rs:206`). With no `--peer-fingerprint`, the strings are `"node-server"` vs `"node-A"` → different HMAC. Supplying a fingerprint does not help: a 64-hex fingerprint is still not the server's node ID. **No configuration can satisfy both sides.**

### 0.3 Test-suite status (reused, not re-run)
`cargo test --workspace` was executed earlier this session series: **exit 0, 66 passed / 0 failed**. It does not cover QUIC handshake, post-auth messaging, dispatcher, JNI, or device paths. The loopback failure is therefore invisible to CI.

### 0.4 Environment
- `git 2.56.1.windows.1`, `git-lfs 3.8.1` present.
- Free disk 108.9 GB at start.
- `target/` = 3,572.3 MB; `android/app/build/outputs/apk/debug/app-debug.apk` = 1,850,303 B exists.
- Historical `check.out` records `os error 4551 — An Application Control policy has blocked this file` for build scripts. Not reproduced in the current session.

---

## 1. Phase A — Version Control Deep Review

### 1.1 Repository facts

| Item | Finding | Status |
|---|---|---|
| Root `C:\...\OCN` | Not a Git repository (`fatal: not a git repository`, exit 1) | `VERIFIED` |
| `ocn-core\.git` | Branch `master`, **0 commits**, no remote, only untracked `.gitignore`/`Cargo.toml`/`src/` | `VERIFIED` |
| `ocn-node\.git` | Branch `master`, **0 commits**, no remote, only untracked `.gitignore`/`Cargo.toml`/`src/` | `VERIFIED` |
| Submodules | No `.gitmodules` anywhere | `VERIFIED` |
| Worktrees | One per repo | `VERIFIED` |
| Global excludes | None; `.git/info/exclude` empty | `VERIFIED` |
| Tracked files | **0 across all repos** | `VERIFIED` |
| Recovery point | **None** | `MISSING` |

> Note: the earlier unified report mentioned only `ocn-core`; `ocn-node` is a **second** independent repo and must be part of any plan.

### 1.2 Coexistence risk analysis (root monorepo + two independent repos)

| Risk | Mechanism | Severity | Mitigation |
|---|---|---|---|
| A-R1 | `git add .` at root records `ocn-core/` and `ocn-node/` as **gitlinks** (commit hashes only, no content) | High | Root `.gitignore` must list `/ocn-core/` and `/ocn-node/` before the first add |
| A-R2 | Root commit succeeds but clones lack the two crates' content, while `Cargo.toml` references them → build fails after clone | High | Document bootstrap: clone root + clone/restore the two repos; or adopt Plan B later |
| A-R3 | `target/` 3.57 GB accidentally staged | High | `/target/` first line of ignore file |
| A-R4 | Two same-named `master` branches later merged by mistake | Medium | Keep de facto separate; never add each other as remotes |
| A-R5 | Windows `core.ignorecase=true` hides case-only renames | Low | Accept; document |
| A-R6 | Nested repo `.gitignore` (`/target`) is inert because `target` lives at root | Low | Root ignore is authoritative; sub-repo ignore may be left as-is |
| A-R7 | Empty dirs vanish after commit | Low | Optional `.gitkeep`, decision pending |

**Verdict:** Plan A (root monorepo excluding the two nested repo directories) remains the lowest-risk option. It requires **no** deletion, rename, or `.git` manipulation. Plan B (rename `.git` dirs) stays as a fallback only.

### 1.3 Artifact management matrix (final recommendation)

| Artifact | Size | Track? | Mechanism | Rationale |
|---|---|---|---|---|
| `Cargo.lock` | 43 KB | **Yes** | normal | Reproducible builds for a workspace binary |
| `ai/models/.../model.safetensors` | 3,240,592 B | **No** (recommended) | ignore + `ai/manifests/model_checksums.json` (SHA-256 `A706726A…89710`, already matches the file) + `download_model.py` | Keeps history small; integrity is verifiable |
| — alternative | — | Yes | Git LFS 3.8.1 (`*.safetensors filter=lfs`) | Only if offline clones must include weights |
| `android/app/src/main/jniLibs/**/libocn_core.so` | binary | **No** | ignore | Reproducible via `build_arm64.ps1` (`--features android-jni`) |
| `android/gradle/wrapper/gradle-wrapper.jar` | 63,375 B | **Yes** | normal | Wrapper bootstrap must be committed per Gradle convention |
| `target/` | 3,572.3 MB | **No** | `/target/` | Build cache |
| `android/{build,app/build,.gradle}/` | MB | **No** | ignore | Gradle output |
| `local.properties` | absent | **No** | ignore pre-emptively | Machine-specific SDK path |
| `*.log`, `*.out`, `install_logs.txt`, `logcat_output.txt`, `launch_log*.txt`, `bidirectional_log.txt` | ~1.1 MB | **No** | ignore | Run artifacts; `check.out` even contains host-policy errors |
| `ai/temp/` | small | **No** | ignore | HuggingFace cache markers |
| 48 `*.md` | ~200 KB | **Yes** | normal | Project record |
| `apk_extract/` | empty | **No** | ignore | Residual unpack directory |

### 1.4 Phase A conclusion
Plan A is executable with a single ignore file and six commits (root, core, node × init+commit). **No operation was performed.** Confirmation list remains as in `OCN_SOURCE_CONTROL_PLAN.md` §4, with one new item: confirm whether `ocn-node` should remain an independent repo or be folded into the root history.

---

## 2. Phase B — Phase 3.1 Pipeline Trace

### 2.1 Hop-by-hop trace

| # | Hop | Code location | What exists | Status |
|---|---|---|---|---|
| 1 | `TaskDescriptor` | `ocn-compute/src/task.rs:19-30`, validation `:59-128` | Full struct + strict validation (version, IDs, priority, timeout, deadline, payload cap, SHA-256, resources) | `VERIFIED` |
| 2 | `TaskDispatcher` | `ocn-compute/src/dispatcher.rs:4-10` | `pub struct TaskDispatcher;` + `new()` only | `MISSING` |
| 3 | `TaskRequestDto` | repo-wide grep | **0 matches**; `ocn-core/src/dto.rs` absent | `MISSING` |
| 4 | QUIC transport | `ocn-core/src/quic.rs:96-272` (server), `:274-362` (client) | Handshake msg types 1–5 only; post-auth handoff bug; auth fails live | `FAILED` |
| 5 | Android Runtime | `ocn-core/src/jni_api.rs:9-17`, `OcnNode.java:8`, `MainActivity.java:37-38` | Only `stringFromJNI`; no task entry, no network client | `MISSING` |
| 6a | `TaskResultDto` | repo-wide grep | Absent | `MISSING` |
| 6b | `ExecutionResult` | `task.rs:40-48`, `runtime.rs:40-53` | Struct + SHA-256 self-validation | `VERIFIED` (local only) |

### 2.2 Cross-cutting checks

| Concern | Evidence | Status |
|---|---|---|
| Message length limits | `proto.rs:10` control 64 KB; `:11` data 2 MB; `:12` legacy packet 10 MB; `task.rs:5` payload 10 MB | `PARTIAL` — three incompatible ceilings; no single authoritative limit for task payloads |
| Framing consistency | Handshake uses 12-byte `FixedHeader`; `ConnectionHandle::send/recv` uses `u32` length + bincode (`quic.rs:57-87`) | `PARTIAL` — two wire formats coexist; a task message must pick one |
| Per-message stream behavior | `send()` opens a new bi-stream per message and immediately `finish()` (`quic.rs:60-64`) | `VERIFIED` — no stream reuse; one message per stream |
| Server post-auth handling | After auth it enqueues a **dummy** `AuthResponse` and hands off (`quic.rs:244-251`); `ocn-node` matches only legacy variants (`main.rs:112-179`) | `FAILED` — connection is dropped after one synthetic packet; no server read loop |
| Timeouts | Handshake frame 10 s (`quic.rs:136,190,303,333,351`); recv 10 s (`quic.rs:71`); task `timeout_ms` 1 s–1 h (`task.rs:9-10`); deadline ≤ 24 h (`task.rs:11`) | `PARTIAL` — no unified deadline propagation from task to transport |
| Reconnect / retry | Client loop 5 × 1 s (`main.rs:185-197`); no retry after send failure | `PARTIAL` |
| Idempotency | `TaskDeduplicationCache` (`deduplication.rs:32-81`) never constructed outside tests | `PARTIAL` |
| Result validation | `ExecutionResult::validate` (`task.rs:132-153`) local only; no network receipt validation | `PARTIAL` |
| Resource release | TTM child killed on unload (`granite_ttm.rs:166-173`); connections dropped implicitly; no task-level cleanup path | `PARTIAL` |
| Silent failure | If all 5 connect attempts fail, `conn_opt` stays `None` and `main` returns `Ok(())` (`main.rs:199,303`) | `FAILED` — process exit code cannot signal connection failure |
| Test coverage of network | No integration test exercises `QuicTransport`; unit tests only | `MISSING` |

### 2.3 Additional fault found by runtime test (19:22)
The handshake failure is deterministic and configuration-proof (see §0.2). It is the **first** blocker; the dummy-handoff bug is the **second** blocker that would surface immediately after fixing auth. Any Phase 3.1 work must repair these before touching task DTOs.

### 2.4 Phase B conclusion
The compute core is real and well-tested; the network task path is absent, and the transport that would carry it is currently broken. See `OCN_PHASE31_IMPLEMENTATION_PLAN.md` for the minimal viable route.

---

## 3. Phase C — Phase 3.2 Deep Review

### 3.1 Why `max_cpu_usage` / `max_memory_mb` have no effect

- Declaration and storage: `scheduler_bridge.rs:5-18` (`PolicyEngine { max_cpu_usage, max_memory_mb, min_confidence }`, `new()` assigns all three).
- Usage: `validate_recommendation` (`:20-75`) reads `self.min_confidence` at `:26`, but compares CPU/memory only against `request.budget.max_cpu` (`:43`) and `request.budget.max_memory_mb` (`:48`).
- Neither `self.max_cpu_usage` nor `self.max_memory_mb` is referenced anywhere → confirmed by the compiler: *"fields `max_cpu_usage` and `max_memory_mb` are never read"*.
- Consequence: the engine's configured ceiling is advisory only; a caller can pass a permissive `ResourceBudget` and bypass the intended global limit. **`FAILED`** for the stated behavior, `VERIFIED` for the root cause.

### 3.2 TTM output range handling

| Layer | Behavior | Status |
|---|---|---|
| Python `runtime_service.py:74-77` | postprocess then `max(0.0, min(1.0, v))` per point | `VERIFIED` (clamped) |
| Rust `granite_ttm.rs:122` | `avg_predicted = sum / 96.0` → assigned to `cpu_usage` with **no clamp** | `PARTIAL` |
| Rust `memory_mb` | hard-coded `1024` (`:132`) | `PARTIAL` — not derived from model |
| `confidence` | hard-coded `0.88` (`:134,137`) | `PARTIAL` — no calibration |
| Contract doc | `OCN_MIND_INTEGRATION_CONTRACT.md:19-21` mandates `[0,1]` clamp and boundary interception | `PARTIAL` — enforced only on one side |

### 3.3 Timeout fallback

- `OcnSchedulerBridge::recommend_schedule` (`:110-128`) wraps the provider in `timeout`, then returns `Err(MindError::Timeout)` on expiry (`:118-121`).
- `analyze_task` (`:97-108`) behaves the same.
- No `RuleBasedProvider` substitution, no circuit breaker, no retry. Contract §3 promises "`<1ms` switch to `RuleBasedProvider`". **`MISSING`**.

### 3.4 Simulated vs real data — decisive evidence

This is the most important correction in this audit.

1. `ai/adapters/granite_ttm_adapter/runtime_service.py:64` — literal comment: **"Simulated prediction using trend + model weights"**.
2. `runtime_service.py:66-72` — prediction = last normalised value + linear trend + `sin(i*0.1)*0.01` + `mean(weight)*0.05`. No forward pass through TTM layers.
3. `test_inference.py:45,60-70` — "模拟 512 context -> 96 forecast"; performs one `torch.matmul` on a random dummy vector purely to touch the weights, then linearly extrapolates.
4. `ai/adapters/granite_ttm_adapter/rust_adapter.rs:43,53,60,72,151` — explicit "Simulates loading", "Simulate model loading time", "Simulate TTM prediction", "Simulated history".
5. `ocn-mind/src/tests.rs:188-189` asserts only `Err`/`Timeout` behavior, not prediction quality.
6. `ocn-mind/src/providers/granite_ttm.rs:188-217` (`test_granite_ttm_full_chain`) spawns Python and reads a JSON line; its ~2.3 s runtime is **subprocess + torch import latency**, not model inference on a device.

Therefore:
- The *plumbing* (Rust → Python stdin/stdout JSON, weights load, unload) is `VERIFIED`.
- The *inference* is a **heuristic simulation** (`PARTIAL` as a "provider", `FAILED` as a "Granite TTM model" claim).
- No document may state that this path demonstrates TTM forecasting quality or real-device performance. Current wording in older reports must be read as **simulated**.

### 3.5 Data provenance labeling

- `ocn-mind/src/schema.rs:6-11` defines `DataSource { SIMULATED, REAL_DEVICE, HISTORICAL, SYNTHETIC }` — exactly the needed label.
- `ocn-mind/src/baselines/rule_based.rs:22` sets `data_source: DataSource::SIMULATED`.
- **But** `schema.rs`, `runtime.rs`, `benchmark.rs`, and `baselines/` are not declared in `ocn-mind/src/lib.rs` → not compiled, not reachable. **`MISSING`** from the live path.
- Device logs (`bidirectional_log.txt`, `launch_log*.txt`, `logcat_output.txt`) contain Android system traffic only; no OCN protocol lines. **No device-side OCN evidence exists.** `UNVERIFIED`.

### 3.6 Phase C conclusion
Two deterministic logic gaps (unused policy limits, no fallback) and one honesty gap (simulated inference presented as TTM). See `OCN_PHASE32_REPAIR_PLAN.md`.

---

## 4. Phase D — Integration Dependency & Test Design

### 4.1 Dependency graph (critical path)

```
[A0] Handshake identity binding fix
      └─> [A1] Post-auth connection loop
            └─> [A2] Unified framing (FixedHeader)
                  └─> [B1] TaskRequestDto/TaskResultDto (msg 11/12)
                        └─> [B2] TaskDispatcher (DTO <-> TaskDescriptor)
                              └─> [B3] ComputeExecutor + Dedup + ResourceManager wiring
                                    ├─> [C1] Windows loopback task E2E
                                    └─> [D1] JNI executeTask
                                          └─> [D2] Android device E2E
[E1] PolicyEngine limits ── independent
[E2] TTM clamp ────────── independent
[E3] Fallback provider ── depends on E1/E2
```

### 4.2 Layered test plan

| Layer | Scope | Success criterion | Failure recovery |
|---|---|---|---|
| L1 Unit | `ocn-compute`, `ocn-mind` internals | All existing 66 tests still pass | Revert the offending change; re-run that crate only |
| L2 Protocol | DTO encode/decode, golden vectors, boundary sizes | Byte-exact golden vectors; malformed input rejected without panic | Fix codec; keep failing vector as regression |
| L3 Transport | Two-node loopback handshake + auth | Client authenticates, receives NodeInfo, connection stays open ≥ 5 s | Capture both logs; assert node-ID binding values |
| L4 Compute | Loopback BatchHash request → result | Result hash equals independently computed SHA-256; latency recorded | Inspect dispatcher/dedup logs; re-run single test |
| L5 JNI (host) | `executeTask` symbol via JVM or `nm`/`objdump` on `.so` | Symbol exported; returns result bytes for a fixed payload | Rebuild `.so`; verify `--features android-jni` |
| L6 Device E2E | POCO M6 ↔ Windows | Device computes result for a Windows-issued task, hash matches, no ANR | `adb logcat` capture; retry once; if persistent, stop and report |

### 4.3 Gates
- L3 must pass before L4 is attempted.
- L5 must pass before L6.
- No layer may be reported as passing based on a different layer's output.
- Device performance numbers are only reportable from L6 with `DataSource::REAL_DEVICE`.

---

## 5. Consolidated Finding Register

| ID | Finding | Status | Severity |
|---|---|---|---|
| F-01 | No repository at root; zero commits anywhere; no recovery point | `MISSING` | Critical |
| F-02 | Two independent repos (`ocn-core`, `ocn-node`), both empty | `VERIFIED` | High |
| F-03 | No root `.gitignore`; `target` 3.57 GB exposed to accidental add | `MISSING` | High |
| F-04 | QUIC auth fails deterministically on loopback (node-ID binding) | `FAILED` | Critical |
| F-05 | Server drops connection after synthetic handoff packet | `FAILED` | Critical |
| F-06 | Two framing formats coexist (FixedHeader vs bincode) | `PARTIAL` | High |
| F-07 | No `TaskRequestDto`/`TaskResultDto`; no msg types 11/12 | `MISSING` | Critical |
| F-08 | `TaskDispatcher`/`ComputeEngine` are 10-line stubs | `MISSING` | Critical |
| F-09 | Android JNI exposes only `stringFromJNI` | `MISSING` | Critical |
| F-10 | Android app has no OCN network client | `MISSING` | Critical |
| F-11 | Payload ceilings inconsistent (64 KB / 2 MB / 10 MB) | `PARTIAL` | High |
| F-12 | No task-level retry after send failure | `PARTIAL` | Medium |
| F-13 | Dedup cache not wired into any runtime path | `PARTIAL` | Medium |
| F-14 | Local result validation strong; no network-side validation | `PARTIAL` | Medium |
| F-15 | Resource release: TTM child handled; task cleanup undefined | `PARTIAL` | Medium |
| F-16 | Client exits 0 when all connection attempts fail | `FAILED` | Medium |
| F-17 | `PolicyEngine.max_cpu_usage`/`max_memory_mb` never read | `FAILED` | High |
| F-18 | TTM `cpu_usage` not clamped in Rust | `PARTIAL` | Medium |
| F-19 | No fallback provider on timeout/error | `MISSING` | High |
| F-20 | "TTM" inference is a heuristic simulation, not a network forward pass | `FAILED` | Critical (claims) |
| F-21 | `schema.rs`/`runtime.rs`/`benchmark.rs`/`baselines` uncompiled | `PARTIAL` | Medium |
| F-22 | No `DataSource` label in the live result path | `MISSING` | Medium |
| F-23 | No device-side OCN evidence in any log | `UNVERIFIED` | Critical |
| F-24 | Historical build-policy block (`os error 4551`) in `check.out` | `UNVERIFIED` (not reproduced) | Low |
| F-25 | Android APK and `.so` artifacts exist | `VERIFIED` (existence only) | — |

---

## 6. Claims Explicitly Not Supported

- "Loopback validation completed" — the loopback handshake **fails** (F-04).
- "TaskRequest → QUIC → TaskResult wired" — no DTO, no handler, no stream loop (F-05, F-07).
- "Android BatchHash executor implemented" — no JNI entry point (F-09).
- "Granite TTM real model inference verified" — inference is simulated (F-20).
- "PolicyEngine enforces global limits" — two of three limits are dead (F-17).
- Any device throughput/latency number — no device run exists (F-23).

---

## 7. Compliance Statement

No Rust, Android, or Python business code was modified. No `git init/add/commit/reset/clean`. No files or directories were deleted or moved. No existing report was overwritten. No unknown process was terminated; the two `ocn-node.exe` processes started for the loopback test were created and stopped by this session. No system, registry, or power settings were changed.
