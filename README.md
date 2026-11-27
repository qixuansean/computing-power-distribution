# OCN — OpenCompute Network

OCN is a distributed compute prototype. This repository contains the Rust workspace for
the node, core QUIC transport, compute runtime, and the real-device compute CLI.

Canonical project state lives in [PROJECT_STATUS.md](PROJECT_STATUS.md). Do not create
additional status reports; update that file instead.

## Current milestone status

| Item | State |
|---|---|
| B-2 identity authentication | **CLOSED** (implemented, unchanged) |
| B-3 auth error codes / exit codes | **PAUSED** |
| MS-Compute-Loopback | **COMPLETE** — 7/7 real-QUIC loopback tests pass |
| MS-Real-Device | **IN PROGRESS** — server/client startup flow verified on one host; dual-device run pending a second Windows PC |

## Security posture

- TLS certificate pinning is enforced with `--server-fingerprint`; verification is never disabled.
- B-2 identity verification runs before any task stream is accepted. A mismatch between
  `server_node_id` and the pinned certificate fingerprint aborts the connection.
- The pairing token is read from `OCN_PAIRING_TOKEN` by default. It is never logged.
  `--token` exists only as a fallback and prints a warning because command-line arguments
  are visible to other local processes.
- Identity private keys stay in the identity directory (`%TEMP%\ocn_identities` by default
  or `--identity-dir`). Only the public certificate fingerprint is printed.
- The server binds `127.0.0.1` by default. Binding a LAN address is explicit and intended
  only for a controlled private network. Do not expose the UDP port to the public internet.

## Build

```powershell
cargo build --release -p ocn-node --bin ocn-compute
```

The binary exposes two roles:

```powershell
.\target\release\ocn-compute.exe server --help
.\target\release\ocn-compute.exe client --help
```

## Real-device runbook

Both machines must share the same pairing token and be on the same controlled network.
Use a firewall rule only on the private profile and only for the chosen UDP port.

### Windows B — compute server

```powershell
# Generate a strong token once and enter the same value on both machines.
# Never commit it and never paste it into logs.
$env:OCN_PAIRING_TOKEN = "<strong-random-token>"

.\target\release\ocn-compute.exe server `
  --node-id compute-b-01 `
  --listen 0.0.0.0:9100 `
  --identity-dir "$env:TEMP\ocn_identities"
```

Wait for the startup line and copy the public fingerprint:

```text
OCN_COMPUTE_SERVER_READY node_id=compute-b-01 fingerprint=<64-hex> listen=0.0.0.0:9100
```

The server prints `OCN_CLIENT_AUTHENTICATED` only after a successful B-2 handshake.
Keep the identity directory stable so the fingerprint does not change between runs.

### Windows A — compute client

Copy the server node id and fingerprint out-of-band (not over the same untrusted channel).

```powershell
$env:OCN_PAIRING_TOKEN = "<same-strong-random-token>"

.\target\release\ocn-compute.exe client `
  --server 192.168.68.114:9100 `
  --server-node-id compute-b-01 `
  --server-fingerprint <64-hex-from-B> `
  --node-id compute-a-01 `
  --identity-dir "$env:TEMP\ocn_identities" `
  --label dual-device `
  --report .\artifacts\ms-real-device\dual-device-report.json
```

The client runs all scenarios by default and exits non-zero if any expectation fails.
Use `--scenario small|256k|1m|concurrent|duplicate|invalid|timeout` to run one scenario.

### Firewall (optional, administrator, private profile only)

```powershell
New-NetFirewallRule -DisplayName "OCN Compute QUIC" `
  -Direction Inbound -Protocol UDP -LocalPort 9100 -Profile Private -Action Allow
```

Remove the rule after testing if the port is not needed permanently.

## Scenarios and recorded metrics

| Scenario | Meaning |
|---|---|
| `small` | Small UTF-8 text payload |
| `256k` | 256 KiB pattern payload |
| `1m` | 1 MiB pattern payload |
| `concurrent` | 8 tasks × 256 KiB submitted concurrently, one QUIC stream each |
| `duplicate` | Same task id twice; second must be a deduplicated replay |
| `invalid` | Wrong input hash; server must reject with `INVALID_TASK` |
| `timeout` | Incomplete frame plus bounded read; follow-up task proves recovery |

> Note: each task stream now runs in its own server task, bounded by an in-flight admission
> limit (default: available CPUs). Before the 2026-11-27 fix the accept loop awaited each
> stream, so the 8 `concurrent` tasks queued and a stalled stream could delay its siblings
> by up to the 15 s request timeout. Measurements and remaining limits are in §7 of
> [PROJECT_STATUS.md](PROJECT_STATUS.md).

The client records:

- `connect_ms` — time from `connect()` start to completed B-2 authentication
- `warmup_ms` — one explicit warm-up task, recorded separately and excluded from scenario stats
- per-task end-to-end latency and server-reported execution time
- per-task and per-scenario throughput in MiB/s
- duplicate flag, error code, and failure reason
- a JSON report via `--report`

Always set `--label` so evidence can never be mistaken for a different test type
(for example `single-device-two-process` versus `dual-device`).

## Tests

```powershell
# Real-QUIC loopback integration suite (success, invalid input, duplicates,
# timeout, interrupted stream, concurrent tasks)
cargo test -p ocn-compute --test quic_loopback

# Entire workspace. The two B-3 RED tests are intentionally failing while B-3 is paused.
cargo test --workspace -- `
  --skip red_b3_auth_result_carries_nonzero_error_code `
  --skip red_b3_binary_exits_nonzero_after_auth_failure
```

## Repository layout

| Path | Purpose |
|---|---|
| `ocn-core/` | QUIC transport, B-2 auth, task wire protocol, identity |
| `ocn-compute/` | Task DTOs, executor, deduplication, QUIC task server/client |
| `ocn-node/` | Node binary and the `ocn-compute` real-device CLI |
| `ocn-mind/` | Local inference runtime |
| `artifacts/ms-real-device/` | Captured single-device validation logs and JSON reports |
| `PROJECT_STATUS.md` | Single source of truth for milestone and evidence status |

Android sources are out of scope for these milestones and must not be modified.
