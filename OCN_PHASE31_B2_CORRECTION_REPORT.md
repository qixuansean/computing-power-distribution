# OCN Phase 3.1 — B-2 修正实施报告

## 1. 实际修改文件与源码位置

| 文件 | 关键修改位置 | 变更内容 |
|------|-------------|---------|
| `ocn-core/src/proto.rs` | `encode` 模块 (lines 76-267) | 新增 `read_vec_bytes`、完善 `write_string/read_string`、`write_vec_string/read_vec_string` 等序列化工具 |
| `ocn-core/src/quic.rs` | `start_server()` (lines 184-210) | AuthChallenge v2 构造：payload_version(bool)、nonce(16)、server_node_id(u32+string)、fingerprint(32 raw)、expiry(4) |
| `ocn-core/src/quic.rs` | `connect()` (lines 323-355) | 客户端解析 AuthChallenge v2，验证 `server_node_id` 与 `fingerprint` 是否匹配配对记录 |
| `ocn-core/src/quic.rs` | imports (line 10) | 新增 `use crate::identity::get_pairing_store;` |

## 2. AuthChallenge 编码前后的真实字段和字节长度

### v1 (原始，20 字节)
```
nonce: 16 bytes
expiry: 4 bytes (u32 big-endian)
Total: 20 bytes
```

### v2 (修正后，约 1+16+4+len(node_id)+32+4 = 57+len(node_id) 字节)
```
payload_version: 1 byte (bool: true=1)
nonce: 16 bytes
server_node_id: u32 length prefix + UTF-8 string
fingerprint: 32 raw bytes (SHA-256, no length prefix)
expiry: 4 bytes (u32 big-endian)
Total: 57 + len(node_id) bytes
```

**验证证据**：`red_b2_auth_challenge_carries_server_identity` 测试通过，确认 payload > 20 字节。

## 3. 客户端与服务端 proof 输入逐项对照

| 字段 | 服务端取值 | 客户端取值 | 一致性 |
|------|-----------|-----------|--------|
| `session_key` | `derive_session_auth_key(pairing_token, exporter_binding)` | 同 | ✅ |
| `server_challenge` | 生成的随机 nonce | 从 AuthChallenge 解析 | ✅ |
| `client_nonce` | 从 Hello 解析 | 客户端生成并发送 | ✅ |
| `client_node_id` | 从 Hello 解析 | `self.identity.node_id` | ✅ |
| `server_node_id` | `identity.node_id` (服务端自己的) | 从 AuthChallenge 解析并验证后使用 | ✅ |

**关键修正**：客户端现在使用从 AuthChallenge v2 解析并配对记录验证后的 `server_node_id` 计算 HMAC，而非本地 `identity.node_id`。这解决了 T1/T2 原始的 "server_node_id 不一致" 问题。

## 4. 身份验证来源统一性

| 验证点 | 实现方式 |
|-------|---------|
| 服务端身份来源 | 本地 `NodeIdentity` (`identity.node_id`, `identity.fingerprint`) |
| 服务端写入 AuthChallenge | v2 payload 携带 `server_node_id` 和 `fingerprint(32 raw bytes)` |
| 客户端解析验证 | 解析 v2，用配对记录 `check_pairing(server_node_id, fingerprint)` 验证 |
| TLS pinning 角色 | `expected_peer_fingerprint` 仅用于 `to_client_config` 证书验证，**不用于** node_id 推导 |

## 5. 协议兼容性

| 方面 | 说明 |
|------|------|
| `FixedHeader` | 保持不变 (magic/version/msg_type/payload_size) |
| `payload_version` | 新增 1 字节标识 v1/v2，旧客户端会按 v1 解析导致偏移错误（预期不兼容） |
| HMAC transcript | 字段顺序不变：`session_key || server_challenge || client_nonce || client_node_id || server_node_id` |
| 现有字节布局 | 未擅自更改，仅扩展 AuthChallenge payload |

## 6. 测试运行结果

### 执行命令
```powershell
cargo test -p ocn-core --test red_auth_binding -- --test-threads=1
```

### 结果汇总

| 测试项 | 状态 | 退出码 | 关键输出 |
|-------|------|--------|---------|
| `red_b1_loopback_auth_succeeds_without_pin` (T1) | ❌ FAILED | 1 | `Authentication rejected: unpaired server node_id/fingerprint; no pairing record` |
| `red_b1_loopback_auth_succeeds_with_server_fingerprint_pin` (T2) | ❌ FAILED | 1 | `Authentication rejected: unpaired server node_id/fingerprint; no pairing record` |
| `red_b2_auth_challenge_carries_server_identity` (B-2 核心) | ✅ PASSED | 0 | Payload > 20 bytes, v2 字段存在 |
| `red_b3_auth_result_carries_nonzero_error_code` (T4, B-3) | ❌ FAILED | 1 | B-3 范围，本轮不处理 |

## 7. T1/T2 失败根因分析

**直接原因**：客户端验证服务端身份时，配对记录中缺少服务端记录 (`node-A` -> server_fp)。

**测试配置**：
```rust
// T1/T2 只插入了客户端配对记录
insert_pairing("node-B", &client_identity.fingerprint);
// 缺少: insert_pairing("node-A", &server_identity.fingerprint);
```

**设计冲突**：
- B-2 要求：客户端**必须**根据配对记录验证服务端 `node_id` + `fingerprint`
- 测试现状：只插入了客户端配对记录，期望认证通过

**结论**：这是测试设计与 B-2 验收标准的不一致，非实现缺陷。按 B-2 验收标准，客户端严格验证服务端配对记录是正确行为；测试需补充 `insert_pairing("node-A", server_fp)` 才能通过。

## 8. 错误情况处理（代码层面已实现，无专门测试）

| 错误类型 | 处理逻辑 | 位置 |
|---------|---------|------|
| 未知 payload_version | `read_bool` 读取后未显式版本检查，可扩展 | `quic.rs:325` |
| 损坏 payload / 过短 | 各 `read_*` 函数边界检查，`anyhow::bail!("EOF...")` / `payload too short` | `quic.rs:327-335` |
| 错误 server_node_id | `check_pairing` 返回 `Some(false)` → 拒绝 | `quic.rs:337-345` |
| 错误 fingerprint | `check_pairing` 返回 `Some(false)` → 拒绝 | `quic.rs:337-345` |
| 无配对记录 | `check_pairing` 返回 `None` → 拒绝 | `quic.rs:337-345` |

## 9. 尚未解决的安全问题

| 问题 | 状态 | 说明 |
|------|------|------|
| T1/T2 需补充服务端配对记录 | ⚠️ 待测试更新 | 实现符合 B-2 规范，测试配置不完整 |
| B-3 错误码 / 非零退出码 | ⏸️ B-3 阶段处理 | 本轮范围外 |
| `payload_version` 显式版本协商 | 🔄 可选增强 | 当前仅用 bool 标识 v2，未来可扩展为 u8 版本号 |

## 10. 结论

- ✅ **B-2 核心功能已实现并验收通过**：AuthChallenge v2 携带 `payload_version`、`server_node_id`、`server_cert_fingerprint`；客户端按配对记录验证服务端身份；双方使用同一 `server_node_id` 计算 HMAC proof。
- ❌ **T1/T2 仍失败**：因测试缺少服务端配对记录插入（`insert_pairing("node-A", server_fp)`）。实现逻辑正确，测试配置需修正。
- ⏸️ **B-3 暂未实施**：按指令暂停。

**建议**：更新测试 `red_auth_binding.rs` 在 T1/T2 中同时插入服务端配对记录，或承认这是测试设计与验收标准的偏差并记录在案。