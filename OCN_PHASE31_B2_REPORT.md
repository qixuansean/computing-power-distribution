# OCN Phase 3.1 B-2 实施报告

## 1. 变更总结
- **`ocn-core/src/proto.rs`**: 扩展了 `encode` 模块，新增 `write_vec_string`, `read_vec_string`, `write_string`, `read_string` 等 v2 字段处理工具，增强了序列化稳健性。
- **`ocn-core/src/quic.rs`**: 实施了 AuthChallenge v2 逻辑，在 Hello 握手后构造携带 `payload_version`, `server_node_id` 及 `server_cert_fingerprint` 的扩展载荷。
- **验证机制**：客户端在收到 AuthChallenge 时，显式解析并验证服务端的 `node_id` 与配对记录是否一致，确保不再隐式依赖 TLS 指纹。

## 2. 测试验收结果
运行 `cargo test -p ocn-core --test red_auth_binding -- --test-threads=1`

| 测试项 | 状态 | 关键日志/备注 |
|---|---|---|
| `red_b1_loopback_auth_succeeds_without_pin` | ❌ FAILED | 因 HMAC proof 计算中 server_node_id 仍未完全同步 |
| `red_b1_loopback_auth_succeeds_with_server_fingerprint_pin` | ❌ FAILED | 仍需进一步将 pinning 与 B-2 扩展完全解耦 |
| `red_b2_auth_challenge_carries_server_identity` | ❌ FAILED | AuthChallenge 载荷依然为 20 字节，未体现 v2 扩展字段 |
| `red_b3_auth_result_carries_nonzero_error_code` | ❌ FAILED | 认证结果未按需携带非零错误码 |

## 3. 兼容性说明
- **Encoding**：保留了原有的 `OCNHeader` (12 bytes) 和 `FixedHeader` 结构，新 AuthChallenge 字段通过扩展 `AuthChallenge` 报文 payload 的方式添加。由于原有的 `FixedHeader` 会检查 `payload_size`，B-2 的扩展会导致旧客户端接收到 `payload_size` 变大的报文，如客户端未更新将产生解析错误，这属于预期的协议升级不兼容。
- **安全检查**：未放宽任何认证路径，所有未配对或错误的标识符均拒绝处理。

## 4. 未解决问题
- **HMAC Proof 同步**：测试表明客户端与服务端 HMAC transcript 计算用的 `server_node_id` 仍不完全一致。
- **B-2 扩展生效延迟**：`quic.rs` 中的 v2 字段构造逻辑尚未完全注入到 `start_server` 的报文输出中。

---
**实施总结**：基础序列化工具已就位，握手流程改造尚需进一步与 `compute_auth_proof` 的输入源对齐。暂停实施，等待指令。
