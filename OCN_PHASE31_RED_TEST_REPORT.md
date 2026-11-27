# OCN Phase 3.1 Red Test Report

**Date:** 2026-11-23
**Status:** Red tests executed, failures captured as expected.
**Production Code Impact:** **NONE.** No production code (`.rs` or others) was modified. All changes were limited to `tests/` subdirectories.

---

## 1. 基线测试结果 (`cargo test --workspace`)
- **Exit Code:** 0 (Success)
- **Duration:** 9.1s
- **Total Tests:** 66 (34+8+17+7+0+0+0+0) — *Matches historical expectation.*
- **Observation:** The 5 empty tests (`test_node_info_exchange`, `test_invalid_token_rejection`, `test_duplicate_challenge_rejection`, `test_unauthenticated_ping_rejection`, `test_malformed_header_rejection`) in `ocn-core/src/tests.rs` are still present and functionally empty (only `PENDING` comments), confirming they remain un-implemented.

---

## 2. Red Test Failures (B-1, B-2, B-3)

| Test Name | File | Failure Reason | Exit Code |
| :--- | :--- | :--- | :--- |
| `red_b1_loopback_auth_succeeds_without_pin` | `red_auth_binding.rs` | Auth failed: `closed by peer: 0`. No consistent `node_id`. | 101 |
| `red_b1_loopback_auth_succeeds_with_server_fingerprint_pin` | `red_auth_binding.rs` | Auth failed: `closed by peer: 0`. Proves fingerprint is misused as `node_id`. | 101 |
| `red_b2_auth_challenge_carries_server_identity` | `red_auth_binding.rs` | Payload length 20. Expected > 20. Missing v2 identity fields. | 101 |
| `red_b3_auth_result_carries_nonzero_error_code` | `red_auth_binding.rs` | Payload length indistinguishable from success; error code missing. | 101 |
| `red_b3_binary_exits_nonzero_after_auth_failure` | `red_exit_code.rs` | Binary exited with status 0 despite 5 auth failures. | 101 |

---

## 3. Test Details Summary

### B-1 (Auth Binding)
- **Panic:** `RED B-1: authenticated loopback failed: closed by peer: 0.`
- **Context:** Confirmed that the current server accepts the connection but drops it due to the `node_id` / certificate identity mismatch during the handshake.

### B-2 (Challenge Identity)
- **Panic:** `AuthChallenge payload is 20 bytes. Expected > 20.`
- **Context:** Confirmed that the current server sends only the 16-byte nonce and 4-byte expiry, lacking the `server_node_id` and `server_cert_fingerprint` required by the v2 design.

### B-3 (Error Reporting)
- **Auth Result:** `AuthResult` payload is too short to contain an error code, and the client-side binary treats `return Ok(())` (the default when no `client_conn` is found after retry) as success, failing the `red_b3_binary_exits_nonzero_after_auth_failure` test (expected code != 0, actual = 0).

---

## 4. Compliance Checklist
- [x] Production code modified? **No.**
- [x] Git operations performed? **No.**
- [x] Files modified/moved/deleted? **No.**
- [x] Protocol version/message format modified? **No.**
- [x] Red tests created for B-1, B-2, B-3? **Yes.**

**Conclusion:** All red tests are successfully failing for the expected design reasons. The environment is verified. Standing by for approval to proceed to B-1 implementation.
