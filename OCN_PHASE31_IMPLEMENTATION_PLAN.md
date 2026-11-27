# OCN Phase 3.1 — Minimal Viable Windows ↔ Android BatchHash Implementation Plan

**Date:** 2026-11-23  
**Status:** Plan only. **No code changed. No Git operation performed.**  
**Basis:** `OCN_EXTENDED_AUDIT.md` findings F-04 … F-16.

---

## 1. Objective and Non-Goals

### Objective
Produce one verifiable end-to-end path:

```
Windows coordinator
  TaskDescriptor → TaskRequestDto → FixedHeader(11) → QUIC stream
        ↓
Android worker (JNI)
  decode → validate → resource check → dedup → BatchHash → TaskResultDto → FixedHeader(12)
        ↓
Windows coordinator
  decode → verify result hash → ExecutionResult
```

### Non-goals (this phase)
- No scheduling/AI involvement in the task path.
- No multi-node fan-out, no load balancing.
- No persistence of task results.
- No real TTM inference.
- No change to `MindProvider` public interface.

---

## 2. Blocking Design Decisions

| # | Decision | Recommendation | Why |
|---|---|---|---|
| D1 | Where does the server node ID come from for auth proof? | Extend `AuthChallenge` payload with `server_node_id`; client reads it and binds it in the HMAC | Current client guesses `"node-server"` → auth always fails (F-04) |
| D2 | One framing or two? | **Unify on 12-byte `FixedHeader`** for all post-auth messages | Golden-vector tested; bincode has no message-type field and pins `bincode 1.3` |
| D3 | Task payload ceiling | Single constant `TASK_MAX_PAYLOAD = 1_900_000` bytes (fits inside 2 MB `DATA_MAX_PAYLOAD_SIZE` with framing overhead) | Removes the 2 MB vs 10 MB contradiction (F-11) |
| D4 | Who reads post-auth frames? | `ocn-node` spawns a per-connection loop immediately after the synthetic handoff packet; the handoff packet is explicitly ignored | Server currently drops the connection (F-05) |
| D5 | Android role | Android = **compute worker/server**; Windows = coordinator/client | Keeps resource checks device-side; matches `ResourceManager` design |
| D6 | Idempotency scope | In-memory `TaskDeduplicationCache`, TTL = `max(timeout_ms, 60 s)` | Already implemented and tested, just unwired (F-13) |
| D7 | Error signaling | `TaskResultDto.status = Rejected|Failed` + `error_code`; never a silent close | Prevents F-16-style silent failure |

---

## 3. Wire Format (normative for this plan)

All multi-byte integers are **big-endian**. Strings are `u32 length + UTF-8`, max 128 bytes unless stated. Blobs are `u32 length + bytes`.

### 3.1 `MsgType 11` — TaskRequest payload

| Field | Type | Constraint |
|---|---|---|
| `task_id` | string | 1–128 |
| `task_type` | u8 | `1 = BatchHash` (others reserved) |
| `protocol_version` | u16 | must be `1` |
| `input_hash` | `[u8; 32]` | SHA-256 of `payload` |
| `priority` | u8 | 0–100 |
| `deadline_unix_ms` | u64 | now < value ≤ now + 24 h |
| `timeout_ms` | u32 | 1 000–3 600 000 |
| `cpu_cores` | u16 | 1–256 |
| `memory_mb` | u32 | 1–1 048 576 |
| `gpu_required` | u8 (0/1) | must be 0 for BatchHash |
| `idempotency_key` | string | 1–128 |
| `payload` | blob | ≤ `TASK_MAX_PAYLOAD` (1 900 000) |

### 3.2 `MsgType 12` — TaskResult payload

| Field | Type | Constraint |
|---|---|---|
| `task_id` | string | echoed |
| `node_id` | string | worker identity, 1–128 |
| `status` | u8 | 0 Success, 1 Failed, 2 Rejected |
| `error_code` | u8 presence + u32 | absent for Success |
| `execution_time_ms` | u64 | ≥ 1 |
| `result_hash` | `[u8; 32]` | SHA-256 of `result` |
| `result` | blob | ≤ `TASK_MAX_PAYLOAD` |

### 3.3 Error codes (v1)
| Code | Meaning |
|---|---|
| 1 | Validation failed |
| 2 | Resource rejected |
| 3 | Unsupported task type |
| 4 | Execution failed |
| 5 | Duplicate in progress |
| 6 | Deadline exceeded |
| 7 | Internal error |

### 3.4 Design sketch (not applied)
```rust
// ocn-core/src/dto.rs  (new file — DO NOT create until approved)
pub const MSG_TASK_REQUEST: u16 = 11;
pub const MSG_TASK_RESULT: u16 = 12;
pub const TASK_MAX_PAYLOAD: usize = 1_900_000;

pub struct TaskRequestDto { /* fields above */ }
pub struct TaskResultDto  { /* fields above */ }

impl TaskRequestDto {
    pub fn encode(&self) -> anyhow::Result<Vec<u8>>;
    pub fn decode(payload: &[u8]) -> anyhow::Result<Self>;
}
```

---

## 4. Work Packages

### B0 — Repair auth identity binding (prerequisite, blocking)
**Files:** `ocn-core/src/quic.rs`, optionally `ocn-core/src/messages.rs` (document the payload shape), `ocn-core/src/tests.rs`.

**Change:** Append `server_node_id` to the `AuthChallenge` payload. Client parses it (with a defensive fallback if absent) and uses it in `compute_auth_proof`; server uses `identity.node_id`. Delete the `"node-server"` fallback.

**Acceptance:**
- Two-node loopback: server logs `authenticated successfully`; client logs `Client authenticated successfully`.
- Fixed-vector unit test: client proof == server proof for the same transcript.
- No HMAC transcript change other than the server ID value.

**Verification command (later):**
```
target\debug\ocn-node.exe --node-id node-A --listen 127.0.0.1:9310
target\debug\ocn-node.exe --node-id node-B --listen 127.0.0.1:9311 --peer 127.0.0.1:9310
```

### B1 — Unify framing on `FixedHeader`
**Files:** `ocn-core/src/quic.rs`, `ocn-core/src/transport.rs`.

**Change:** Add `send_frame(msg_type, &[u8])` and `recv_frame(timeout) -> (u16, Vec<u8>)` to `QuicConnection`. Route Echo/Hash/Ping/Throughput through these (msg types 6–10) or keep the legacy bincode path explicitly isolated and marked deprecated.

**Acceptance:**
- A unit test round-trips a 0-byte, 1-byte, 64 KB, and 1.9 MB frame.
- Oversize frame is rejected before allocation (`payload_size > limit`).
- No bincode/OCHHeader in the new task path.

### B2 — Per-connection server loop
**Files:** `ocn-core/src/quic.rs`, `ocn-node/src/main.rs`.

**Change:** After successful auth, keep the connection alive. `ocn-node` receives the synthetic packet as a **Connected** marker and immediately `tokio::spawn`s a loop:
```text
loop {
  match conn.recv_frame(remaining_deadline).await {
    Ok((11, p)) => handle_task(p),
    Ok((6, p))  => pong,
    Ok((_, _))  => ignore,
    Err(_)      => break,
  }
}
```
Remove the legacy `AuthRequest` arm from the server path (auth is now transport-level).

**Acceptance:**
- Server stays connected ≥ 30 s after auth; a follow-up Ping receives a Pong.
- Connection close is logged with peer node ID.

### B3 — DTOs and codecs
**Files:** `ocn-core/src/dto.rs` (new), `ocn-core/src/lib.rs`, `ocn-core/src/messages.rs` (optional re-export).

**Acceptance:**
- Round-trip property test for all fields, including empty/1-byte/max payload.
- Golden byte vector for a fixed request and a fixed result.
- Truncated payload → `Err`, never panic.
- Unknown `task_type` → `Err` at decode or validate.

### B4 — `TaskDispatcher` implementation
**Files:** `ocn-compute/src/dispatcher.rs` (replace stub), `ocn-compute/Cargo.toml` (add `ocn-core` dependency).

**Pipeline:**
```
TaskRequestDto::decode
  → TaskDescriptor conversion
  → task.validate()
  → ResourceManager::can_accept_task(current_metrics)
  → TaskDeduplicationCache::check_or_register
      ├─ NewTask        → ComputeExecutor::execute → store_completed → TaskResultDto
      ├─ AlreadyRunning → TaskResultDto{Rejected, code 5}
      └─ Completed(r)   → cached TaskResultDto (replay)
  → result.validate() → encode
```
Use `spawn_blocking` for hashing of large payloads.

**Acceptance:**
- Happy path returns `status=Success`, normalized `result` = input payload, correct hash.
- Duplicate idempotency key returns the cached result with identical `result_hash`.
- Invalid input hash → `Rejected`, code 1.
- Low battery/high temp metrics → `Rejected`, code 2.

### B5 — Client task API
**Files:** `ocn-compute/src/client.rs` (new) or `ocn-node/src/main.rs`.

**Change:** `submit_task(conn, dto, deadline) -> TaskResultDto`.
- One frame out, one frame in, remaining-deadline timeout.
- Retry transport errors up to 2 times with backoff 250 ms / 1 s, reusing the idempotency key.
- Never retry `Rejected`/`Failed`.
- Recompute `SHA-256(result)` and compare `result_hash`; mismatch → reject locally.

**Acceptance:**
- Loopback BatchHash 1 byte / 1 MB / 1.9 MB all succeed.
- Induced wrong hash → client reports validation error, no false success.
- Killing the worker mid-task yields a timeout, not a hang.

### B6 — Android JNI task entry
**Files:** `ocn-core/src/jni_api.rs`, `android/app/src/main/java/org/opencompute/network/OcnNode.java`, `MainActivity.java`.

**New symbols:**
```
Java_org_opencompute_network_OcnNode_startWorker(env, class, port: jint, token: jstring) -> jboolean
Java_org_opencompute_network_OcnNode_executeTask(env, class, request: jbyteArray) -> jbyteArray
Java_org_opencompute_network_OcnNode_stopWorker(env, class) -> void
```
- `startWorker` builds a `tokio::runtime::Runtime` once, spawns `QuicTransport::start_server`, stores a global handle.
- `executeTask` is **synchronous** for the Java caller, runs on the runtime, returns the encoded `TaskResultDto`.
- All panics are caught with `catch_unwind` and converted to a JNI error, never aborting the app.
- Java side calls these from a background `ExecutorService`, not the UI thread.

**Acceptance:**
- `nm -D`/`objdump -T` on the arm64 `.so` shows the three exported symbols.
- A host-side JVM smoke test (if feasible) returns a result for a fixed payload.
- `adb logcat` shows no `FATAL EXCEPTION`.

### B7 — Device E2E harness
**Files:** `scripts/e2e_device.ps1` (new), `android/.../MainActivity.java` (status text).

**Flow:**
1. `build_arm64.ps1` → copy `.so` to `jniLibs`.
2. `gradlew assembleDebug` → `adb install -r`.
3. Start Windows `ocn-node` server on `0.0.0.0:9400`.
4. Launch app; it calls `startWorker(9400, token)` and displays the device IP.
5. Windows client sends a fixed 64 KB payload (input hash precomputed).
6. Assert: `status=Success`, `result_hash == SHA256(payload)`, `node_id == android-poco-m6`.
7. Capture `adb logcat -d` and the Windows client log into `logs/e2e_<timestamp>.txt`.

**Acceptance:**
- One real device run produces a matching hash.
- Report records `DataSource::REAL_DEVICE` and the measured round-trip; no fabricated numbers.

---

## 5. Ordering and Dependencies

```
B0 ──> B1 ──> B2 ──> B3 ──> B4 ──> B5 ──> L4 loopback pass
                                              └─> B6 ──> B7 device pass
```

- B0 and B1 can be reviewed together; B1 must not start before B0 is proven.
- B4 depends on B3 only through the DTO types; it can be unit-tested with hand-built DTOs.
- B6 depends on B4 (dispatcher) and B2 (server loop).
- B7 is the only phase allowed to report device performance.

---

## 6. Test Matrix (Phase 3.1)

| ID | Layer | Test | Pass criterion |
|---|---|---|---|
| T-01 | Unit | Existing 66 tests | All pass, 0 failed |
| T-02 | Unit | DTO round-trip × sizes | Byte-identical re-encode |
| T-03 | Unit | DTO malformed inputs | `Err`, no panic |
| T-04 | Unit | Dispatcher: new/duplicate/invalid/rejected | 4 distinct outcomes |
| T-05 | Transport | Two-node loopback auth | Both sides report success |
| T-06 | Transport | Post-auth Ping/Pong | Pong within 1 s |
| T-07 | E2E | Loopback BatchHash 1 B / 1 MB / 1.9 MB | Hash match, no timeout |
| T-08 | E2E | Duplicate submission | Cached result, one execution |
| T-09 | E2E | Worker killed mid-task | Client timeout, clean error |
| T-10 | JNI | Symbol export | 3 symbols present |
| T-11 | Device | 64 KB BatchHash | Hash match, `REAL_DEVICE` label |
| T-12 | Device | 1.8 MB payload | Hash match or explicit size rejection |

**Recovery rule:** if T-07 fails, do not attempt T-11. If T-11 fails twice, stop and report with full logcat.

---

## 7. Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Auth fix alters transcript and breaks compatibility with any peer | Medium | High | Single-version protocol; no deployed peers; add golden test |
| `bincode` legacy path still used by Echo tests | High | Low | Keep isolated; mark deprecated; delete in a later phase |
| Android NDK/JNI build breaks (`os error 4551` historically) | Medium | High | Verify build policy before B6; build early |
| Windows mDNS needs admin (observed warning) | High | Low | Use direct IP for E2E; mDNS not required |
| 1.9 MB frame exceeds UDP/QUIC practical size | Low | Medium | QUIC handles fragmentation; measure; fall back to 512 KB chunks if needed |
| Device thermal throttling distorts timing | High | Low | Record temperature; do not report timing as model performance |
| Duplicate execution under retry | Medium | Medium | Idempotency key + cache; test T-08 |

---

## 8. Effort Estimate (engineering, not calendar)

| Package | Effort |
|---|---|
| B0 | 0.5 day |
| B1 | 0.5 day |
| B2 | 1 day |
| B3 | 0.5 day |
| B4 | 1 day |
| B5 | 1 day |
| B6 | 1 day |
| B7 | 1 day |
| **Total** | **~6.5 engineering days** |

---

## 9. Approval Gates

1. Approve D1–D7 (§2).
2. Approve the wire format (§3).
3. Approve the package order (§5).
4. Confirm Android = worker/server (D5).

Only after these approvals may B0 begin. No code has been written.
