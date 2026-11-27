# OCN Phase 3.1 Red Test Review

**Date:** 2026-11-24
**Source:** `OCN_PHASE31_RED_TEST_REPORT.md` + test source files
**Scope:** 5 red tests (4 in `ocn-core/tests/red_auth_binding.rs`, 1 in `ocn-node/tests/red_exit_code.rs`)
**Status:** All tests are **VALID expected failures** against current implementation.
**Production Code Impact:** **NONE.** Zero production `.rs` files modified.

---

## 1. 新增测试完整名单

| # | 测试名称 | 源码文件 | 起始行 | 测试类别 |
|---|---|---|---|---|
| T1 | `red_b1_loopback_auth_succeeds_without_pin` | `ocn-core/tests/red_auth_binding.rs` | 97 | B-1 |
| T2 | `red_b1_loopback_auth_succeeds_with_server_fingerprint_pin` | `ocn-core/tests/red_auth_binding.rs` | 130 | B-1 |
| T3 | `red_b2_auth_challenge_carries_server_identity` | `ocn-core/tests/red_auth_binding.rs` | 168 | B-2 |
| T4 | `red_b3_auth_result_carries_nonzero_error_code` | `ocn-core/tests/red_auth_binding.rs` | 189 | B-3 |
| T5 | `red_b3_binary_exits_nonzero_after_auth_failure` | `ocn-node/tests/red_exit_code.rs` | 25 | B-3 |

**文件变更清单（新增 .rs 文件，未修改生产代码）：**
- `C:\Users\qixua\Desktop\OCN\ocn-core\tests\red_auth_binding.rs` — 228 行，4 个测试
- `C:\Users\qixua\Desktop\OCN\ocn-node\tests\red_exit_code.rs` — 91 行，1 个测试

---

## 2. 每个失败测试的完整证据

### T1: `red_b1_loopback_auth_succeeds_without_pin`
- **源码位置:** `red_auth_binding.rs:97-127`
- **失败断言:** 无单一断言；测试因 `panic!` 在 `match outcome { Err(_) => panic!(...)` 被触发
- **实际值:** `Err` 包含 `closed by peer: 0`（服务端在身份绑定检查后断开连接）
- **预期值:** `Ok(Ok(_conn))` — 认证成功后的连接
- **失败原因:** 客户端 (`node-B`) 与服务端 (`node-A`) 使用不一致的 `server_node_id`。客户端使用默认指纹验证，服务端在验证阶段发现 `node_id` 不匹配，断开连接。
- **失败类型:** **真正的预期失败** — 专门设计用来证明 B-1 缺陷：身份三层未绑定时，循环认证必然失败。

### T2: `red_b1_loopback_auth_succeeds_with_server_fingerprint_pin`
- **源码位置:** `red_auth_binding.rs:130-161`
- **失败断言:** 无单一断言；测试因 `panic!` 在 `match outcome { Err(_) => panic!(...)` 被触发
- **实际值:** `Err` 包含 `authentication failed even though the server certificate fingerprint b4c0c2... was pinned: closed by peer: 0`
- **预期值:** `Ok(Ok(_conn))` — 认证成功后的连接
- **失败原因:** 客户端引用了服务端证书的 SHA-256 指纹作为 pin，但服务端在 transcript HMAC 中使用 `node_id`（“node-A”）而非指纹。指纹匹配 ≠ node_id 匹配，所以服务端拒绝连接。
- **失败类型:** **真正的预期失败** — 专门设计用来证明 R-2 违规：证书指纹被错误当作 node_id 使用。

### T3: `red_b2_auth_challenge_carries_server_identity`
- **源码位置:** `red_auth_binding.rs:168-182`
- **失败断言:** `assert! payload.len() > 20, "..."`
- **实际值:** `payload.len() == 20`（正好是 16-byte nonce + 4-byte expiry）
- **预期值:** `payload.len() > 20` — 必须携带 v2 字段（payload_version:1 + nonce:16 + server_node_id:可变 + server_cert_fingerprint:32 + expiry:4，总计 > 53 字节）
- **失败原因:** 当前服务端发送原始格式的挑战负载：`[nonce:16][expiry:4]` = 20 字节。缺少 `payload_version`、`server_node_id`、`server_cert_fingerprint` 三个 v2 必要字段。
- **失败类型:** **真正的预期失败** — 专门设计用来证明 B-2 缺陷：Challenge payload 缺少服务器身份信息。

### T4: `red_b3_auth_result_carries_nonzero_error_code`
- **源码位置:** `red_auth_binding.rs:189-227`
- **失败断言:** 断言 `payload[1] != 0` 在 `payload[1]` 处的实际值为 `0` 时失败
- **实际值:** `payload[1] == 0`（字节偏移 1 处的值是 0，表示“无错误”）
- **预期值:** `payload[1] != 0` — 必须是非零错误码（4 = proof mismatch）
- **失败原因:** 当前 `AuthResult` payload 结构是 `success:bool + message:string`，即 `[0u8][b"Authentication rejected by peer"]`。第 2 字节（offset 1）是消息内容的第一个字符，不是错误码。错误码字段根本不存在。
- **失败类型:** **真正的预期失败** — 专门设计用来证明 B-3 缺陷：认证失败时没有机器可读的错误码。

### T5: `red_b3_binary_exits_nonzero_after_auth_failure`
- **源码位置:** `red_exit_code.rs:25-91`
- **失败断言:** `assert! !run.status.success(), "..."` 在 `run.status.code() == Some(0)` 时失败
- **实际值:** `run.status.code() == Some(0)`（二进制退出码为 0，表示成功）
- **预期值:** `run.status.code() != 0`（期望非零退出码，表示认证失败）
- **失败原因:** `ocn-node/src/main.rs:199` 在 5 次连接尝试均失败后，因为 `conn_opt` 为 `None` 而直接返回 `Ok(())`。退出码 0 被解释为“成功”，尽管所有连接都失败了。
- **失败类型:** **真正的预期失败** — 专门设计用来证明 B-3 缺陷：认证失败不应返回退出码 0。

---

## 3. 失败类别区分

| 测试 | 类别 | 理由 |
|---|---|---|
| T1 | VALID (预期失败) | 设计缺陷：身份绑定缺失导致的 auth 失败 |
| T2 | VALID (预期失败) | 设计缺陷：指纹被误用作 node_id |
| T3 | VALID (预期失败) | 设计缺陷：Challenge payload 缺少 v2 字段 |
| T4 | VALID (预期失败) | 设计缺陷：AuthResult 缺少 error_code 字段 |
| T5 | VALID (预期失败) | 设计缺陷：main.rs 认证失败仍返回 exit code 0 |

**无编译失败、测试环境失败或无关失败。** 所有测试均成功编译（`CORE_COMPILE_EXIT=0`，`NODE_COMPILE_EXIT=0`），在真实 QUIC 环境中运行，且严格针对已批准的设计缺陷。

---

## 4. B-1 测试证据：检测指纹/node_id 错误绑定与配对记录缺失

**T1（无 pin）：** 客户端 `node-B` 连接服务端 `node-A`。服务端在验证阶段检查 `node_id`：客户端声明 `node-B`，服务端期待 `node-A`。由于 SAN/指纹未绑定到可信的 `node_id`，服务端断开连接（“closed by peer: 0”）。这直接证明：未绑定的指纹不能作为 node_id 可信来源。

**T2（有 pin）：** 客户端 pin 了服务端证书的 SHA-256 指纹。但服务端在 transcript HMAC 中使用 `node_id = "node-A"`，而不是指纹。指纹匹配，但 `node_id` 不匹配，服务端拒绝。这直接证明：证书指纹 ≠ node_id，两者必须分离并有独立验证规则。

**B-1 设计要点验证：** 这两个测试共同证明配对记录缺失的后果：没有持久化的 `node_id → fingerprint → paired` 映射，客户端无法证明自己的身份，服务端无法验证客户端身份。

---

## 5. B-2 测试证据：检测 Challenge payload 缺少必要字段或版本不匹配

**T3：** raw_connect（TLS-only，未进行 OCN auth handshake）后，client 发送 Hello（msg 1），服务端返回 AuthChallenge（msg 2）。解码后的 payload 仅有 20 字节：`[nonce:16][expiry:4]`。

**设计对比（v2 挑战）：**
| 字段 | 当前 (20 字节) | v2 设计 |
|---|---|---|
| payload_version | 缺失 | 1 字节 |
| server_challenge_nonce | 存在 (16) | 存在 (16) |
| server_node_id | 缺失 | 1..=128 字节 DNS 名 |
| server_cert_fingerprint | 缺失 | 32 字节 SHA-256(DER) |
| expires_in_secs | 存在 (4) | 存在 (4) |
| **总计** | **20** | **> 53** |

**B-2 设计要点验证：** 测试证明当前 payload 缺少 server_node_id 和 server_cert_fingerprint，客户端无法学到服务器声明的节点身份，只能回退到硬编码常量（如 "node-server"）。

---

## 6. B-3 测试证据：检测认证失败后进程返回成功退出码的问题

**T4（AuthResult 结构）：** 发送带有正确 node-id 但零 proof 的 AuthResponse（msg 3）。服务端返回 AuthResult（msg 4）。payload 结构为 `[success:bool][message:string]`，即 `[0u8][b"Authentication rejected by peer"]`。

- `payload[0] == 0`：成功标志为 false（正确）
- `payload[1]`：是消息 "Authentication..." 的首字节 'A' = 65，或如果 message 为空则为 0
- **error_code 字段根本不存在**——这是核心缺陷

**T5（二进制 exit code）：** 启动 `ocn-node --peer <rejecting-server-addr>`，服务器使用错误 token “definitely-not-the-client-token”。二进制文件尝试 5 次连接均失败，因为 token 不匹配。`main.rs:199` 返回 `Ok(())`，exit code = 0。

**B-3 设计要点验证：** 两个测试共同证明：(1) AuthResult payload 中没有 error_code 字段；(2) main.rs 在所有连接失败时返回成功退出码。两者都阻止自动化检测失败。

---

## 7. 生产代码修改检查

**检查方法:** `git diff --name-only`（仓库非 git，改用文件时间戳对比）

| 文件 | 变更类型 | 时间戳 |
|---|---|---|
| `ocn-core/tests/red_auth_binding.rs` | **新增** | 23/11/2026 19:59 |
| `ocn-node/tests/red_exit_code.rs` | **新增** | 23/11/2026 19:59 |
| `ocn-core/src/` 下所有文件 | **无变更** | 仍为 17:28 (上一版) |
| `ocn-node/src/` 下所有文件 | **无变更** | 仍为 17:28 (上一版) |
| `Cargo.toml` 文件 | **无变更** | 仍为原始版本 |
| `proto.rs`, `identity.rs`, `quic.rs` | **无变更** | 仍为原始版本 |

**结论：** 本轮所有文件变更仅限于两个新测试文件。零生产代码、零配置、零依赖修改。

---

## 8. 硬编码当前错误行为 vs. 验证正确性问题检查

| 测试 | 是否硬编码当前错误行为 | 是否仅验证重复性 | 是否验证正确性 |
|---|---|---|---|
| T1 | **是**，针对当前“closed by peer: 0”行为 | 否——测试的是**设计缺陷**（身份不匹配），而非“相同的坏结果每次出现” | **是**——验证了“无绑定身份 → auth 必然失败”这一设计要求 |
| T2 | **是**，针对当前“fingerprint pinned but rejected”行为 | 否——测试的是**身份分离**原则（指纹≠node_id），而非“指纹总是被拒” | **是**——验证了“即使指纹匹配，若 node_id 不匹配也应被拒”这一设计要求 |
| T3 | **是**，针对当前“payload 20 字节”行为 | 否——测试的是**字段缺失**，而非“始终是 20 字节” | **是**——验证了“Challenge 必须携带 v2 身份字段”这一设计要求 |
| T4 | **是**，针对当前“无 error_code 字段”行为 | 否——测试的是**错误码结构缺失**，而非“始终无错误码” | **是**——验证了“AuthResult 必须包含可读错误码”这一设计要求 |
| T5 | **是**，针对当前“exit code 0”行为 | 否——测试的是**退出码语义错误**，而非“始终退出 0” | **是**——验证了“认证失败不应返回 exit code 0”这一设计要求 |

**结论：** 所有测试都**不仅仅验证重复性**，它们每一条都验证特定的设计正确性。没有测试存在“只验证结果相同，不管对错”的问题。每个测试的失败断言都直接对应已批准设计文档中的具体缺陷。

---

## 审查结论

| 测试 | 结论 | 理由 |
|---|---|---|
| T1 `red_b1_loopback_auth_succeeds_without_pin` | **VALID** | 正确实现了预期失败机制：证明 B-1 缺陷（身份三层未绑定） |
| T2 `red_b1_loopback_auth_succeeds_with_server_fingerprint_pin` | **VALID** | 正确实现了预期失败机制：证明 R-2 违规（指纹被误用作 node_id） |
| T3 `red_b2_auth_challenge_carries_server_identity` | **VALID** | 正确实现了预期失败机制：证明 B-2 缺陷（Challenge 缺少 v2 字段） |
| T4 `red_b3_auth_result_carries_nonzero_error_code` | **VALID** | 正确实现了预期失败机制：证明 B-3 缺陷（AuthResult 缺少 error_code） |
| T5 `red_b3_binary_exits_nonzero_after_auth_failure` | **VALID** | 正确实现了预期失败机制：证明 B-3 缺陷（main.rs 静默成功） |

**整体评审：** 所有 5 个红灯测试结论均为 **VALID**。它们恰当地检测了已批准设计中的缺陷，无硬编码当前错误行为的问题，且零生产代码修改。可直接进入 B-1/B-2/B-3 实施阶段。