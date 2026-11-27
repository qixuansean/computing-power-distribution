# OCN Phase 3.1 — B-2 环境准备报告

## 1. 目录与备份确认
- 项目根目录 `C:\Users\qixua\Desktop\OCN` 可访问。
- 备份目录 `C:\Users\qixua\OCN_BACKUPS\OCN_20261123_1930` 确认存在且完整，未作任何修改。

## 2. B-2 涉及的核心源码分析
- **AuthChallenge 报文格式** (`ocn-core/src/proto.rs`)：目前为 `FixedHeader(12bytes) + Payload(raw nonce:16 + expiry:4 = 20 bytes)`。B-2 需要在 payload 中扩展 `payload_version` (1byte)、`server_node_id` (varstring)、`server_cert_fingerprint` (32bytes)。
- **认证握手流** (`ocn-core/src/quic.rs`)：
  - `start_server()`：处理 `MsgType 1` (Hello)，生成 challenge，计算 proof。需要修改以构造 v2 Challenge。
  - `connect()`：发送 Hello，接收 AuthChallenge。需要修改以解析 v2 字段。
- **HMAC Proof 依赖** (`ocn-core/src/auth.rs`)：`compute_auth_proof` 目前使用 `session_key, challenge, nonce, client_node_id, server_node_id`。需确保 transcript 编码在增加 `payload_version` 等字段后，proof 逻辑仍然健壮。

## 3. 测试基准 (未修改)
- `T1` (`red_b1_loopback_auth_succeeds_without_pin`)：预期红灯（因客户端缺乏服务端 `node_id`），B-2 修复。
- `T2` (`red_b1_loopback_auth_succeeds_with_server_fingerprint_pin`)：预期红灯（因 TLS pinning 与 B-1 变更的交叉依赖），B-2 修复。
- 测试源码路径：`ocn-core/tests/red_auth_binding.rs`。

## 4. B-2 预计修改文件列表
| 文件 | 必要性 |
|------|----------|
| `ocn-core/src/proto.rs` | 增加序列化/反序列化 v2 AuthChallenge 字段的辅助函数 |
| `ocn-core/src/quic.rs` | 实施 AuthChallenge v2 的构造、解析及客户端身份验证逻辑 |
| `ocn-core/src/auth.rs` | 检查 `compute_auth_proof` 是否受字段扩展影响 |

## 5. 当前存在风险与兼容性
- **编码一致性**：Transcript 增加字段后，如果 HMAC proof 包含这些字段，必须确保新旧代码对于 proof 计算保持版本感知的兼容。
- **TLS 交叉依赖**：TLS Pinning (fingerprint) 与 node_id 逻辑在 `connect()` 中仍有隐式耦合，需确保 AuthChallenge v2 携带的 `server_node_id` 成为客户端建立信任的显式依据，而非依赖 TLS 层猜测。

---
**当前操作状态**：只读检查完成，**未实施任何修改**。等待正式实施指令。
