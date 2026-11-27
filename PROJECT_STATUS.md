# OCN Project Status

Single source of truth for milestone status, evidence, and open items.
Update this file instead of creating additional reports.

Last updated: 2026-11-27
Repository: `C:\Users\qixua\Desktop\OCN`

## 1. Milestone status

| Milestone | Status | Notes |
|---|---|---|
| B-2 — identity authentication (AuthChallenge v2, pairing store, TLS exporter binding) | **CLOSED** | Implemented; authentication code was not modified in MS-Real-Device |
| B-3 — machine-readable auth error codes / node exit code | **PAUSED** | Two intentionally RED tests still fail by design |
| MS-Compute-Loopback | **COMPLETE** | 7/7 real-QUIC loopback tests pass |
| Same-connection concurrency | **IMPLEMENTED** | Bounded spawn-per-stream + single-flight dedup; 7/7 new regression tests pass, see §7 |
| MS-Real-Device | **IN PROGRESS** | Server/client startup flow and full scenario suite verified on one host; dual-device execution pending a second Windows PC |

## 2. MS-Real-Device deliverables

| Deliverable | State |
|---|---|
| Reuse `TaskServer`, `TaskClient`, `QuicConnection` without rewriting | Done |
| Runnable compute server entry point | `ocn-node/src/bin/ocn-compute.rs` (`server` subcommand) |
| Runnable compute client entry point | `ocn-node/src/bin/ocn-compute.rs` (`client` subcommand) |
| B-2 authenticated connection before tasks | Done; `OCN_CLIENT_AUTHENTICATED` appears only after auth |
| Real `ComputeExecutor` on the server | Done; no simulated result path |
| Client-side independent result-hash verification | Done with `sha2` in the client, separate from the server executor |
| Small text / 256 KiB / 1 MiB / 8 concurrent / duplicate / invalid / timeout scenarios | Done |
| Connection time, task time, throughput, failure reasons | Printed as `TASK`, `SCENARIO`, `CLIENT_SUMMARY`, and JSON report |
| README updated | `README.md` created (no README existed before this milestone) |
| Project status document | This file |

## 3. Environment available for this run

| Item | Value |
|---|---|
| Hosts accessible to the agent | **One**: `DESKTOP` |
| OS | Windows |
| Network interface used | Wi-Fi `192.168.68.114` |
| Server binary | `target\release\ocn-compute.exe` |
| Identity storage | `%TEMP%\ocn_realdevice_identities` (outside the repository) |
| Evidence directory | `artifacts\ms-real-device\` |

Only one Windows machine was reachable. No remote shell, second physical host, or
second Windows VM was available. Therefore **no dual-device result was produced**.

## 4. Single-device, two-process validation (not dual-device evidence)

Two independent OS processes were used: a real server process and a real client process.
They communicate over a real QUIC socket with real TLS and the real B-2 handshake.
This validates the startup flow and the full scenario suite, but both processes run on
`DESKTOP`. It is **not** evidence of cross-device execution.

Run label: `single-device-two-process`

| Metric | Value |
|---|---|
| Server node / client node | `compute-b-local` / `compute-a-local` |
| Transport | QUIC, loopback `127.0.0.1:47807` |
| B-2 connect time | 4.902 ms |
| Warm-up (recorded separately) | 432.176 ms, passed |
| Small UTF-8 text (55 B) | 0.341 ms |
| 256 KiB payload | 7.041 ms, 30.28 MiB/s scenario throughput |
| 1 MiB payload | 21.677 ms, 39.68 MiB/s scenario throughput |
| 8 concurrent × 256 KiB | wall 30.114 ms; min/avg/max 8.469 / 17.038 / 24.352 ms; 66.41 MiB/s |
| Duplicate task | first 0.179 ms; replay 0.113 ms, `duplicate=true` |
| Invalid input hash | rejected in 0.137 ms with `error_code=5` (`INVALID_TASK`) |
| Timeout probe | 309.423 ms bounded read; follow-up recovery task 0.567 ms |
| Total | 16/16 passed, 3,408,040 B, 374.583 ms measured, 8.68 MiB/s overall |
| Client exit code | 0 |

Evidence:

- `artifacts/ms-real-device/single-device-server.log`
- `artifacts/ms-real-device/single-device-client.log`
- `artifacts/ms-real-device/single-device-report.json`

Notes:

- The timeout scenario writes an incomplete task frame, so the server waits for the body
  and the client's bounded read times out deterministically. A follow-up valid task on the
  same connection proves the timeout did not poison the session.
- Duplicate detection is server-side and keyed by task id; the second response has
  `duplicate=true` and the same verified hash.
- The first task after a fresh connection/server can take roughly 400 ms on this Windows
  host (cold start). A single warm-up task is therefore run and recorded as `warmup_ms`,
  but excluded from scenario statistics. Without the warm-up, the same `small` scenario
  measured 0.6–3 ms on already-warm processes.

### LAN-interface startup check (still single host)

The server was also bound to the machine's LAN address instead of loopback:

- Bind: `192.168.68.114:47802`
- Client target: `192.168.68.114:47802`
- B-2 connect: 6.062 ms
- `small`: 2.753 ms, passed
- Client exit code: 0

Evidence: `artifacts/ms-real-device/lan-interface-*.log` and `lan-interface-report.json`.

Both the server and client ran on `DESKTOP`; the peer observed by the server was the same
host. This checks the non-loopback bind path and the Windows firewall path, not remote
reachability.

### Secret-handling check

A scan of all files under `artifacts/ms-real-device/` for pairing tokens, private-key
markers, and token log lines returned no matches. Logs contain only the public certificate
fingerprint, peer addresses, node ids, and metrics.

### Security regression checks

Negative checks were run against a live server to confirm the protections are enforced:

| Case | Result | Exit code |
|---|---|---|
| Wrong server certificate fingerprint | TLS pinning mismatch; connection aborted | 1 |
| Correct fingerprint, wrong `server_node_id` | B-2 pairing-store mismatch; connection aborted | 1 |
| Correct fingerprint/node id, wrong pairing token | Authentication rejected; peer closed the connection before any task was served | 1 |

The server log contains no `OCN_CLIENT_AUTHENTICATED` line for these cases, so no task
stream was accepted. The coarse connection-level rejection for a wrong token is expected
while B-3 (machine-readable auth error codes) remains paused; it is still a rejection, not
a bypass. Evidence: `artifacts/ms-real-device/security-*.log`.

## 5. Dual-device status

**BLOCKED — second Windows PC not available.**

To finish MS-Real-Device, run the server on Windows B and the client on Windows A, then
capture the checklist below. Do not reuse the single-device report as dual-device evidence.

Required evidence:

1. Windows B hostname and LAN IP; server node id and public fingerprint from
   `OCN_COMPUTE_SERVER_READY`.
2. Windows A hostname and LAN IP; client node id.
3. Server log line `OCN_CLIENT_AUTHENTICATED peer=<A-LAN-IP>` where the peer is the second
   machine, not `127.0.0.1` or the server's own address.
4. Client report created with `--label dual-device`.
5. All 16 scenario tasks pass (`--scenario all`) over the LAN.
6. Connection time, per-task latency, throughput, and any failure reason preserved in the
   JSON report.
7. Confirmation that no token or private key appears in either log.

Runbook: see the real-device section of [README.md](README.md).

## 6. Known behavior and risks

- `TaskServer` now runs each accepted stream in its own task and bounds concurrent
  admission with a semaphore (default: available CPUs, `with_max_inflight(n)` to override).
  Before the 2026-11-27 fix the accept loop awaited one stream at a time; root cause,
  before/after measurements, and remaining limits are in §7.
- The first task on a fresh Windows process can show a ~400 ms cold-start outlier; it is
  captured as `warmup_ms`.
- The server's deduplication cache has a 300 s TTL and is keyed by task id. The client uses
  a per-run timestamp prefix so separate runs do not accidentally hit old cache entries.
- B-3 remains paused; the following tests fail intentionally:
  `red_b3_auth_result_carries_nonzero_error_code` and
  `red_b3_binary_exits_nonzero_after_auth_failure`.

## 7. Same-connection concurrency: root cause, fix, and measurement

Reviewed read-only on 2026-11-27, then implemented on the same day. This section keeps the
original root-cause analysis, records the implemented change, and separates measured facts
from remaining assumptions. B-2 and B-3 were not touched, Android was not touched, and no
Git operations were run.

### 7.1 Verdict

Confirmed by code trace and by a live probe against the unmodified server:

- The serialization point was the accept loop in
  `ocn-compute/src/server.rs`. It `await`ed `handle_stream` inline, so the next
  `accept_task` call could not happen until the current task was fully written back.
- No lock and no shared mutable state caused the serialization. `ComputeExecutor` is
  stateless (`ocn-compute/src/runtime.rs:17-55`) and `TaskDeduplicationCache` only holds
  its mutex for short bookkeeping operations.
- Consequence beyond throughput: one stream that never completes its frame blocked every
  other stream **on that connection** for up to `request_timeout` (15 s). Measured
  14,797 ms. Any authenticated peer could do this to its own connection, and it was a
  correctness issue, not only a performance one.

Fixed in §7.5; measured after the fix in §7.9.

### 7.2 Pre-fix call path

| Step | Location | Notes |
|---|---|---|
| Client opens one bidi stream per task | `ocn-core/src/quic.rs:157-163` | `open_task` writes the frame, finishes the send half, keeps the recv half |
| Client awaits its own result | `quic.rs:109-119` | `TaskExchange` owns the matching `RecvStream`; results cannot cross streams |
| Server accepts one stream | `ocn-core/src/quic.rs:169-175` | `accept_task` = `timeout(idle_timeout, accept_bi())`, `idle_timeout` = 1 s |
| **Server handled it inline** | **`server.rs` accept loop (pre-fix)** | **`Ok(Some(stream)) => self.handle_stream(&mut stream).await`** |
| Decode request | `server.rs:69-87` | bounded by `request_timeout` = 15 s |
| Validate + execute | `server.rs:89-139` | `check_or_register` → `execute_new` |
| Execute | `server.rs:141-183` | `spawn_blocking(ComputeExecutor::execute)`, then `timeout` |
| Write result | `server.rs:99` | `stream.send_result`, then `finish()` |

Only after `send_result` returns does the loop call `accept_task` again. Every other
already-accepted-by-QUIC stream waits in quinn's queue, unread.

### 7.3 Measured evidence

Probe run on `DESKTOP` (12 logical cores) against the unmodified `TaskServer`, over real
QUIC with the real B-2 handshake. Log: `artifacts/ms-real-device/concurrency-probe.log`.

Probe 1 — head-of-line blocking:

| Step | Result |
|---|---|
| Open stream A, announce 4096 B, write 5 B, keep the stream open | stream stalls as designed |
| Submit a valid task on a sibling stream of the same connection | completed only after **14,797.1 ms** |
| After resetting stream A, next task | 0.4 ms |

14.8 s matches `request_timeout` = 15 s. The valid task was not slow; it was never read.

Probe 3 — 8 × 4 MiB submitted concurrently on one connection:

| Order | Latency (ms) |
|---|---|
| 1 | 159.7 |
| 2 | 236.7 |
| 3 | 306.2 |
| 4 | 372.5 |
| 5 | 445.3 |
| 6 | 509.1 |
| 7 | 575.4 |
| 8 | 638.8 |

Wall 666.4 ms, all successful, `execution_count` = 10 (8 + 2 earlier probes). Each task adds
a near-constant ~65–73 ms step. That is the signature of strictly additive per-task work,
not of CPU contention: 8 SHA-256 tasks over 4 MiB on 12 cores should finish together. The
observed max/min latency ratio is 4.0; true parallel execution would be close to 1.

Cross-check with the accepted single-device evidence
(`artifacts/ms-real-device/single-device-report.json`): 8 × 256 KiB, every task reported
`server_execution_ms = 1`, yet latencies were 8.5 / 15.5 / 10.9 / 16.7 / 17.1 / 21.6 /
21.6 / 24.4 ms, wall 30.1 ms. Execution is 1 ms per task; the remaining 20+ ms is queueing.

### 7.4 Secondary bottlenecks

1. **Deduplication mutex clones payload bytes while held** —
   `deduplication.rs:33-53`. On a cache hit, `result.clone()` (line 47) copies the whole
   `ExecutionResult`, including the result payload (up to `TASK_MAX_PAYLOAD_SIZE` = 10 MiB,
   `task_wire.rs:42`), inside the global `Mutex`. Every other task's `check_or_register`
   and `store_completed` waits for that memcpy. Also `purge_expired_locked`
   (`deduplication.rs:95-110`) is O(cache) and runs under the same lock on every check
   (capacity 1024, `server.rs:35`). Not the main serialization point, but it is the most
   likely next contention point now that requests run in parallel.
   *Classification: code-verified; magnitude not measured.*
2. **`request_timeout` was per stream but blocked the connection** — the 15 s read budget
   for one stream stalled every sibling. Fixed in §7.5.
3. **No admission bound** — pre-fix, `TaskServer` had no semaphore and the accept loop was
   an accidental limit of 1. Parallelizing without a bound would let a peer keep up to
   quinn's default `max_concurrent_bidi_streams` = 100 streams in flight (quinn-proto
   0.11.19 `config/transport.rs:373`; no custom `TransportConfig` is set —
   `ocn-core/src/identity.rs:134,214`). At the 10 MiB wire limit that is a large memory
   exposure, so the semaphore in §7.5 is mandatory, not optional. `serve_connection` is
   still spawned per connection without a cap, so many stalled connections multiply the
   same exposure; that remains open.
4. **Timeout does not stop work** — `spawn_blocking` cannot be cancelled, so the hashing
   continues detached while the cache is marked `Failed`. Parallelism turns this from a
   rare oddity into accumulated background CPU load. The permit is now held until the
   blocking closure really ends (§7.5), which fixes accounting but not the wasted CPU.
5. **`ResourceManager` is not wired into the server** — `resource_manager.rs` is only used
   by its own tests and `tests/integration_tests.rs`. `TaskDescriptor::resource_requirements`
   (`server.rs:232-236`) is ignored at runtime. Out of scope here, but it means the
   semaphore is currently the only resource control.

### 7.5 Implemented change

Applied to `ocn-compute/src/server.rs` and `ocn-compute/src/deduplication.rs`. No wire,
auth, or client change.

```rust
// Accept loop: admit with a bound, then run the handler in its own task.
pub async fn serve_connection(self: &Arc<Self>, connection: &QuicConnection) {
    loop {
        match connection.accept_task(self.idle_timeout).await {
            Ok(Some(stream)) => {
                let permit = match self.inflight.clone().acquire_owned().await {
                    Ok(p) => p,            // backpressure: stop accepting at capacity
                    Err(_) => break,
                };
                let guard = InflightGuard::new(permit, self.inflight_now.clone());
                self.peak_inflight.fetch_max(guard.observed(), Ordering::SeqCst);
                let server = Arc::clone(self);
                tokio::spawn(async move { server.handle_stream(stream, guard).await });
            }
            Ok(None) => continue,
            Err(_) => break,
        }
    }
}
```

Details that matter:

- `max_inflight` defaults to `available_parallelism()` and is overridable with
  `with_max_inflight(n)`. It bounds all connections served by one `TaskServer`.
- `InflightGuard` owns the `OwnedSemaphorePermit` and keeps the `inflight_now` gauge
  accurate. On execution timeout the guard is **moved into a reaper task** that awaits the
  still-running `spawn_blocking` closure, so a timed-out task keeps its capacity until the
  CPU work really ends.
- `handle_stream` now takes `TaskStream` by value plus the guard; the rest of the request
  path is unchanged.
- `serve_connection` keeps `&Arc<Self>`, so the existing call sites in
  `ocn-node/src/bin/ocn-compute.rs` and `tests/quic_loopback.rs` needed no edits.
- `peak_inflight()` exposes the high-water mark so the limit can be tested directly.

**Single-flight deduplication (implemented).** Parallelizing would otherwise turn a
concurrent duplicate into `ALREADY_RUNNING` (error 10) instead of a replay. The cache now
carries a `broadcast::Sender<String>` of completed idempotency keys:

- `TaskServer::execute_request` subscribes **before** calling `check_or_register`, so a
  completion that races with the check is observed either as a terminal cache state or as
  a delivered notification. There is no lost-wakeup window.
- `store_completed` / `store_failed` publish the key after inserting the terminal state.
- On `AlreadyRunning` the waiter blocks on the receiver, filtering for its own key, for at
  most its own `request.timeout_ms`; it then reads the cache once through the new
  read-only `lookup()` and replays the stored result with `duplicate = true`.
- The waiter holds its admission permit while waiting, so a duplicate flood cannot bypass
  the limit.
- If the original is still running when the waiter's budget expires, the waiter returns
  `TIMEOUT` (6) with an explicit message instead of `ALREADY_RUNNING`, because the wait
  budget is what was exhausted.

### 7.6 Effect on existing guarantees

| Area | Effect (implemented) |
|---|---|
| Deduplication | Concurrent identical ids are coalesced: one execution, the waiter replays the stored result with `duplicate = true`, verified in `duplicate_id_concurrent_is_coalesced`. Sequential retries behave exactly as before. Cache key, TTL (300 s), capacity (1024) unchanged. |
| Timeout | Per-stream client timeout unchanged. The server's `request_timeout` now delays only its own stream. A timed-out execution keeps its admission permit through the reaper until the blocking closure ends. |
| Resource limits | Strengthened: a semaphore bounds admitted work where the accept loop previously bounded it implicitly at 1. `ResourceRequirements` is still not wired. |
| Connection interruption | Spawned handlers die with the connection; `send_result` errors were already ignored. Only an in-progress `spawn_blocking` outlives its stream, which is pre-existing and now accounted for by the reaper. Covered by `connection_interruption_isolation`. |
| Result verification | Unaffected. One result per stream, `TaskExchange` holds the matching `RecvStream`, and `verify_for_request` (`task_wire.rs:302-311`) runs per request. No shared result state was introduced. |
| Memory | Bounded by the admission limit instead of unbounded: at most `max_inflight` × (request + result), each up to 10 MiB on the wire. Default limit is the CPU count. A `TransportConfig` cap on `max_concurrent_bidi_streams` is still not set. |

### 7.7 Regression tests (implemented in `ocn-compute/tests/quic_concurrency.rs`)

Pre-fix RED was verified by temporarily restoring the inline `await` in the accept loop,
running the four discriminating tests, then restoring the fix (file hash confirmed
identical afterwards).

| Test | Asserts | Pre-fix | Post-fix |
|---|---|---|---|
| `stalled_stream_does_not_block_sibling` | stalled 4096 B stream + sibling task; sibling completes in < 2 s | **FAILED** — timed out after 3 s | PASSED |
| `eight_concurrent_tasks_scale` | 8 × 4 MiB; `max/min` latency < 2.0, `peak_inflight >= 2`, `execution_count == 8` | **FAILED** — spread 4.58, peak 1 | PASSED — spread ≈ 1.1 |
| `duplicate_id_concurrent_is_coalesced` | same id on two streams at once: both succeed, equal hash, exactly one `duplicate == false`, `execution_count == 1` | PASSED (serial replay) | PASSED (coalesced) |
| `stalled_stream_does_not_consume_sibling_timeout` | 4 sibling tasks with a 1 s budget while a stream stalls | **FAILED** — timed out after 1 s | PASSED |
| `connection_interruption_isolation` | 4 tasks in flight, connection closed, new connection verified | PASSED | PASSED |
| `semaphore_permits_released_after_cancelled_streams` | 4 reset streams, then 6 tasks with only 2 permits | PASSED | PASSED |
| `inflight_limit_is_enforced` | limit 3, 8 × 4 MiB: `peak <= 3` and `>= 2` | **FAILED** — peak 1 | PASSED — peak 3 |

Assertions are ratios and ceilings (`max/min`, "under 2 s"), not host-specific
milliseconds, so they stay valid on smaller machines.

### 7.8 Measured result after the fix

Same probe, same host (`DESKTOP`, 12 logical cores), same real QUIC + B-2 path, loopback.
Log: `artifacts/ms-real-device/concurrency-probe.log` (both the before and after sections).

| Measurement | Before | After |
|---|---|---|
| Stalled stream vs. sibling task | 14,797.1 ms | **0.8 ms** |
| 8 × 4 MiB latency spread (`max/min`) | 4.00 (160 → 639 ms) | **1.09** (298 → 326 ms) |
| 8 × 4 MiB wall time | 666.4 ms | **351.0 ms** |
| All results verified, `execution_count` | 10 / 10 ok | 10 / 10 ok |

End-to-end through the shipped `ocn-compute` binary, two processes, `--scenario all`
(`artifacts/ms-real-device/concurrency-after-fix-report.json`): 16/16 tasks passed, exit
code 0, connect 6.1 ms. The 8 × 256 KiB `concurrent` scenario is now **bandwidth-bound**
(2 MiB of results over one congestion-controlled connection): wall 30.1 → 24.8 ms and
spread 8.5–24.4 → 7.8–20.3 ms. That scenario is a poor discriminator after the fix, which
is why the 4 MiB probe and `peak_inflight` are the primary evidence.

### 7.9 Remaining risks and unverified assumptions

- The deduplication cache still clones a cached result while holding its mutex
  (`deduplication.rs`). Code-verified, not benchmarked; it is now the most likely next
  contention point for large cached results.
- `ResourceManager` is still not wired into the server; the semaphore is the only admission
  control, and `TaskDescriptor::resource_requirements` is still ignored.
- Peak memory is bounded but not small: `max_inflight` × up to 10 MiB request + 10 MiB
  result. No `TransportConfig` cap on `max_concurrent_bidi_streams` is set.
- `spawn_blocking` work cannot be cancelled. The reaper fixes accounting only; a timed-out
  task still consumes CPU until its hash finishes.
- A peer that opens `max_inflight` stalled streams can occupy every permit for up to
  `request_timeout` (15 s), which now affects the whole server rather than one connection.
  Narrowing that needs a shorter read budget or a per-peer admission cap; not done here.
- `ALREADY_RUNNING` (10) is no longer produced by the duplicate path: the waiter re-reads
  the cache until it reaches a terminal state or its budget expires, then returns
  `TIMEOUT`. The constant stays in the wire protocol for compatibility.
- All measurements here are single-host loopback. Higher-RTT dual-device behaviour is
  unmeasured, and no dual-device claim is made.

## 8. Change log

### 2026-11-27 — same-connection concurrency fix

- `ocn-compute/src/server.rs`: the accept loop no longer awaits the handler inline. Each
  admitted stream runs in its own task, bounded by `Arc<Semaphore>` with
  `with_max_inflight(n)` (default `available_parallelism()`).
- Added `InflightGuard`, an admission permit that keeps the in-flight gauge accurate and is
  moved into a reaper task on execution timeout so detached blocking work stays accounted.
- Added `peak_inflight()` / `max_inflight()` for direct limit testing.
- `ocn-compute/src/deduplication.rs`: added a `broadcast` completion channel,
  `subscribe_completions()`, and read-only `lookup()`. Concurrent duplicates now wait for
  the original and replay its cached result instead of returning `ALREADY_RUNNING`.
- Added `ocn-compute/tests/quic_concurrency.rs` with 7 tests; verified RED on the pre-fix
  accept loop for the 4 discriminating tests, then GREEN.
- `ocn-compute/Cargo.toml`: added `quinn` as a dev-dependency for the stalled-stream helper.
- Wire protocol, B-2 authentication, client verification, B-3, and Android untouched.
- Measured on one host over loopback only: stall 14,797 ms → 0.8 ms, 8 × 4 MiB spread
  4.00 → 1.09, wall 666 → 351 ms. No dual-device claim.
- No Git operations were run.

### 2026-11-27 — concurrency bottleneck review (read-only)

- Traced the single-connection serialization to `ocn-compute/src/server.rs:59-67`.
- Measured it with a throwaway probe outside the repository: a stalled stream blocked a
  sibling task for 14,797 ms; 8 × 4 MiB showed a strictly additive 160→639 ms staircase.
- Logged evidence at `artifacts/ms-real-device/concurrency-probe.log`.
- Documented secondary bottlenecks: dedup cache clone under the mutex, 15 s per-stream
  read blocking the connection, missing admission bound, non-cancellable blocking work,
  unwired `ResourceManager`.
- Designed a minimal fix (spawn per stream + bounded semaphore + owned `TaskStream`) and
  the single-flight dedup change required to keep duplicate semantics, plus nine tests.
- No production code modified. B-2 unchanged, B-3 still paused, no Android changes,
  no Git operations.

### 2026-11-27 — MS-Real-Device (partial)

- Added `ocn-node/src/bin/ocn-compute.rs` with `server` and `client` roles, reusing
  `TaskServer`, `TaskClient`, `QuicConnection`, `QuicTransport`, B-2 auth, and
  `ComputeExecutor`.
- Added scenario suite: small, 256k, 1m, concurrent (8), duplicate, invalid, timeout.
- Added metrics and JSON reporting, environment-based token handling, LAN bind warning,
  and public-fingerprint pairing.
- Added `README.md` and this status document.
- Verified single-device two-process flow and a LAN-interface bind on one host.
- No Android changes. No Git operations. B-2 code unchanged. B-3 untouched.
