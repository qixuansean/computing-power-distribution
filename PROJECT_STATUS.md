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

- `TaskServer` processes streams sequentially per connection. Eight concurrent tasks are
  submitted on eight independent QUIC streams, but they queue on that one connection.
  Separate connections are served concurrently. On loopback this measured 30 ms wall for
  8 × 256 KiB. If strict parallel execution on a single connection becomes a requirement,
  that is a follow-up change.
- The first task on a fresh Windows process can show a ~400 ms cold-start outlier; it is
  captured as `warmup_ms`.
- The server's deduplication cache has a 300 s TTL and is keyed by task id. The client uses
  a per-run timestamp prefix so separate runs do not accidentally hit old cache entries.
- B-3 remains paused; the following tests fail intentionally:
  `red_b3_auth_result_carries_nonzero_error_code` and
  `red_b3_binary_exits_nonzero_after_auth_failure`.

## 7. Change log

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
