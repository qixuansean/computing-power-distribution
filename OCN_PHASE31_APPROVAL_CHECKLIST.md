# OCN Phase 3.1 审批材料（B-1 ~ B-5 / B-8）

**生成：** 2026-11-23 20:05
**依据：** `OCN_NEXT_EXECUTION_GATE.md` §3（B 组 12 项） + 第 3 节重点要求
**状态：** 仅整理材料。**未修改业务代码，未执行 Git 操作，未开始 B0。**
**配套：** `OCN_PHASE31_DESIGN_REVIEW.md`（设计全文）、`OCN_BACKUP_REPORT.md`（恢复点）

---

## 0. 阅读前必须知道的两处修正

### 0.1 上一版文档对 "17 个调用点" 的列举有误

`OCN_PHASE31_DESIGN_REVIEW.md` §8.9 把 `main.rs:114/184/189/199/251` 也列进了调用点，但这几处**不是** `ConnectionHandle` 的方法调用：

| 行 | 实际是什么 | 是否属于 17 |
|---|---|---|
| `main.rs:114` | `server_rx.recv()`，`mpsc::Receiver` | ❌ |
| `main.rs:184/189/199` | `conn_opt` 变量声明/赋值/模式匹配 | ❌ |
| `quic.rs:251` | `tx.send(...)`，`mpsc::Sender` | ❌ |

真正的 17 个调用点 = **`send` 12 个 + `recv` 5 个**，全部在 `ocn-node/src/main.rs`。本文 §6 给出逐行清单。上一版文档的"17"这个**数字是对的，列表是错的**，以本文为准。

### 0.2 B-1 出现一个上一版未发现的依赖问题

上一版设计把"校验证书 SAN 等于 node_id"写成理所当然的一步。本轮核查发现：

- `rcgen 0.13.2` 确实提供 `CertificateParams::from_ca_cert_der()`，能解析出 `subject_alt_names`；
- 但该函数带有 `#[cfg(feature = "x509-parser")]`，而 `rcgen` 的默认特性是 `["crypto", "pem", "ring"]`，**不包含** `x509-parser`；
- `Cargo.lock` 中**不存在** `x509-parser` 这个 crate。

结论：要做严格 SAN 校验，必须修改 `ocn-core/Cargo.toml` 启用 `rcgen/x509-parser` 特性，从而引入 `x509-parser`、`asn1-rs`、`oid-registry` 等传递依赖。这正好命中 `OCN_NEXT_EXECUTION_GATE.md` §5 的停止条件 6（"需要修改 Cargo.toml 依赖才能继续 → 停止并报告"）。

因此 B-1 不能简单标"建议批准"，本文给出三个子方案供选择，见 §2.4。

---

## 1. 审批总表

| ID | 事项 | 结论 | 理由摘要 |
|---|---|---|---|
| **B-1** | 身份三层定义 + 绑定规则 | **待确认** | 规则本身正确，但严格 SAN 校验需新增依赖；另有零依赖替代方案 |
| **B-2** | `AuthChallenge` v2 负载 | **建议批准** | 纯负载扩展，无生产调用点，风险最低 |
| **B-3** | 认证错误码 1–7 | **建议批准** | 消除"只看到 closed by peer: 0"，改动面 2 处编解码 |
| **B-4** | 测试先行 T-A1…T-A11 | **建议批准** | 唯一能防止"空测试再次出现"的机制 |
| **B-5** | 统一 FixedHeader，移除 bincode 线上路径 | **建议批准（分批）** | 方向正确；17 点中 8 删 3 迁 6 退役，建议按两步落地以控制编译中断 |
| **B-6** | 长度上限三常量 | 建议批准 | 消除 2 MB / 10 MB 冲突 |
| **B-7** | 超时表 | 建议批准 | 补齐空闲超时与任务级截止时间 |
| **B-8** | BatchHash 语义方案 A | **建议批准** | 语义自洽、可独立验证；仅 1 条断言需改 |
| **B-9** | 幂等键 + input_hash 绑定 | 建议批准 | 修复"同 key 不同 payload 命中错误结果" |
| **B-10** | 工作包顺序 | 建议批准 | 依赖关系正确 |
| **B-11** | Android = 工作节点 | 建议批准 | 与 `ResourceManager` 设计一致 |
| **B-12** | `MetricsProvider` | 建议批准 | 当前无任何生产代码填充 `SystemMetrics` |
| **C-1** | `HeuristicTrendProvider` 标识 | **建议批准（并入 B0 之后立即执行）** | 成本低，消除虚假声明 |
| **C-4** | 准确率声明限制 | **建议批准（作为 B5/B7 验收门禁）** | 否则设备端数字必然被误读 |

**不建议批准：无。** B-1 是唯一需要您做选择的项。

---

## 2. B-1 —— 身份三层定义与绑定规则

### 2.1 涉及文件与函数

| 文件 | 函数/项 | 行 | 拟改动 | 类型 |
|---|---|---|---|---|
| `ocn-core/src/identity.rs` | `NodeIdentity::generate` | 17–36 | 复用新的指纹/SAN 辅助函数 | 重构 |
| 同上 | `NodeIdentity::load_or_generate` | 38–65 | 同上；增加 SAN 与 node_id 一致性自检 | 重构 + 校验 |
| 同上 | `NodeIdentity::to_server_config` | 67–149 | 内嵌 `StrictClientCertVerifier` 增加 SAN 结构校验 | 修改 |
| 同上 | `StrictClientCertVerifier::verify_client_cert` | 86–103 | 增加"证书必须含恰好一个 DNS SAN" | 修改 |
| 同上 | `NodeIdentity::to_client_config` | 151–232 | 内嵌 `StrictServerCertVerifier` 同步规则 | 修改 |
| 同上 | `StrictServerCertVerifier::verify_server_cert` | 161–180 | 保留指纹钉扎；SAN 校验放到挑战阶段 | 修改 |
| 同上 | **新增** `fn cert_fingerprint(der: &[u8]) -> String` | — | 消除 4 处重复的 SHA-256 计算（25–28、44–51、92–95、169–172） | 新增 |
| 同上 | **新增** `fn dns_san_names(der: &[u8]) -> Result<Vec<String>, IdentityError>` | — | 返回 DNS 类型 SAN 列表（子方案 a） | 新增 |
| `ocn-core/src/quic.rs` | `QuicTransport::connect` | 274–362 | 用挑战中的 `server_node_id`；加 R-5 校验 | 修改 |
| 同上 | `start_server` 认证分支 | 188–257 | 加 R-4：Hello/AuthResponse/证书三方一致 | 修改 |
| 同上 | `QuicTransport` 结构体 | 15–24 | 新增 `expected_peer_node_id: Option<String>` | 结构变更 |
| 同上 | `QuicTransport::new` | 27–46 | 参数从 7 个变 8 个（或用 builder 规避） | 接口变更 |
| `ocn-node/src/main.rs` | 身份初始化 | 41–42 | 身份目录从 `temp_dir()` 改为可配置 | 修改 |
| 同上 | CLI 定义 | 20–40 | 新增 `--identity-dir`、`--peer-node-id` | 新增 |

### 2.2 接口变更

```rust
// 现有
pub struct NodeIdentity { pub node_id: String, pub cert_der: Vec<u8>, pub key_der: Vec<u8>, pub fingerprint: String }
impl NodeIdentity {
    pub fn generate(node_id: &str) -> anyhow::Result<Self>;
    pub fn load_or_generate<P: AsRef<Path>>(node_id: &str, storage_dir: P) -> anyhow::Result<Self>;
    pub fn to_server_config(&self, expected_client_fingerprints: Option<HashSet<String>>) -> anyhow::Result<quinn::ServerConfig>;
    pub fn to_client_config(&self, expected_server_fingerprint: Option<String>) -> anyhow::Result<quinn::ClientConfig>;
}

// 拟新增（不破坏现有签名）
impl NodeIdentity {
    pub fn cert_fingerprint(der: &[u8]) -> String;                       // 纯函数
    pub fn dns_san_names(der: &[u8]) -> Result<Vec<String>, IdentityError>;
    pub fn peer_identity_from_connection(conn: &quinn::Connection)
        -> Result<(String /*node_id*/, String /*fingerprint*/), IdentityError>;
}
```

`QuicTransport::new` 有 7 个参数，直接加第 8 个会让唯一调用点（`main.rs:45-53`）必须改，且未来难扩展。**推荐加 builder**：

```rust
impl QuicTransport {
    pub fn with_expected_peer_node_id(mut self, id: Option<String>) -> Self;
}
```

这样 `new` 签名不变，调用点数量为 **0**（仅新增一行链式调用）。

### 2.3 调用点统计

| 符号 | 生产调用点 | 测试调用点 |
|---|---|---|
| `NodeIdentity::generate` | 1（`load_or_generate` 内部，`:60`） | 1（`tests.rs:122-123` 间接） |
| `load_or_generate` | 1（`main.rs:42`） | 1（`tests.rs:122`） |
| `to_server_config` | 1（`quic.rs:103`） | 0 |
| `to_client_config` | 1（`quic.rs:276`） | 0 |
| `verify_client_cert` | rustls 内部回调（非显式调用） | 0 |
| `verify_server_cert` | rustls 内部回调 | 0 |
| `QuicTransport::new` | 1（`main.rs:45-53`） | 0 |

**总显式调用点：5 个。** 改动集中在 `identity.rs` 与 `quic.rs`，`ocn-compute` / `ocn-mind` / Android 完全不受影响。

### 2.4 三个子方案（这是 B-1 的核心决策）

| 方案 | 做法 | 新增依赖 | 安全强度 | 工作量 | 结论 |
|---|---|---|---|---|---|
| **B-1a** | 启用 `rcgen/x509-parser`，用 `CertificateParams::from_ca_cert_der()` 解析 SAN，严格校验 `SAN == node_id` | **是**：`x509-parser`、`asn1-rs`、`oid-registry`、`data-encoding` 等传递依赖 | 中（见下方分析） | 2 天（含依赖审计与构建验证） | 可行，但需您批准改 `Cargo.toml` |
| **B-1b** | 不解析 SAN。引入配对记录 `peers.json`：`node_id → cert_fingerprint`。首次配对（凭 token）时写入，之后每次连接校验指纹必须匹配该记录 | **否** | 中（等价，见下） | 1.5 天 | **推荐** |
| **B-1c** | 自写最小 DER 解析器（约 80–120 行）提取 SAN 扩展 | 否 | 中 | 2.5 天 + 畸形输入测试 | 可行但不划算 |

**关键安全分析（决定推荐方向）：**

当前所有证书都是**自签**的（`identity.rs:21` `params.self_signed()`）。自签模型下，任何持有配对 token 的人都可以自己生成一张 SAN 为 `node-A` 的证书——**SAN 校验拦不住恶意方**。它真正防的是：

1. 实现错误（例如客户端把指纹当节点名，即当前 bug）；
2. 配置错误（把 A 的证书配给 B）；
3. 日志/审计中身份串号。

真正提供信任的是**指纹钉扎**或**配对记录**。因此 B-1b 在安全强度上不弱于 B-1a，而且不引入新依赖、不需要改 `Cargo.toml`，也不会触发门禁 §5 的停止条件。

B-1b 同时满足您提出的三条硬性要求：

- 不得把证书指纹当 node_id → 指纹只用于**查表**得到 node_id，不参与 transcript；
- 区分三种身份 → `peers.json` 就是"预期对端身份"的持久化，证书身份与节点身份分列两个字段；
- 两端规则一致 → 服务端与客户端使用同一份 `PeerDirectory` 校验逻辑。

**若选 B-1b，仍保留 SAN 的"尽力校验"：** 启动时用 `rcgen` 重建自身证书的 SAN 并与 `node_id` 比对（自己的证书可以重建，无需解析外部证书），确保自身身份自洽。外部证书的 SAN 暂时只记录不强制。

### 2.5 修复前失败的测试（B-1）

| ID | 测试 | 修复前表现 | 失败原因 |
|---|---|---|---|
| T-A2 | 两侧 proof 在真实节点名下相等 | 失败 | 客户端用 `"node-server"`（`quic.rs:317`），服务端用 `node-A`（`:206`） |
| T-A3 | 挑战中的服务器指纹与实际证书不符 → 客户端中止 | 失败（无此逻辑） | 挑战负载里根本没有指纹字段 |
| T-A4 | 挑战中的 `server_node_id` 与证书身份不符 → 客户端中止 | 失败（无此逻辑） | 同上 |
| T-A5 | Hello 声称 A、AuthResponse 声称 B → 服务端拒绝 | 失败 | `quic.rs:193` 把 `_resp_node_id` 丢弃 |
| T-A6 | 客户端证书与声明 ID 无绑定 → 拒绝 | 失败 | `verify_client_cert` 不查 SAN（`:86-103`） |
| T-A7 | 钉扎不匹配 → 明确错误 | **部分已通过** | TLS 层已拒绝（`:99`/`:176`），但错误只表现为 `closed by peer: 0`；需要断言错误分类 |
| T-A9 | 错误 token → 错误码 4 | 失败 | 无错误码字段 |

### 2.6 修复后成功标准（B-1）

| 测试 | 断言 |
|---|---|
| T-A2 | `client_proof == server_proof`，且服务端名 ≠ `"node-server"` |
| T-A3 | 客户端返回 `Err(AuthError::ServerFingerprintMismatch)`，**未**发送 AuthResponse（用 mock 计数验证） |
| T-A4 | 客户端返回 `Err(AuthError::ServerIdentityMismatch)` |
| T-A5 | 服务端返回 `AuthResult{success:0, error_code:6}` |
| T-A6 | 服务端返回 `AuthResult{success:0, error_code:2}` 或 TLS 层拒绝 |
| T-A7 | 客户端在**一次**尝试后退出，错误字符串含 `fingerprint mismatch`，退出码 ≠ 0 |
| T-A9 | `error_code == 4`，且与 `error_code == 3` 可区分 |

### 2.7 回归范围（B-1）

- `cargo test -p ocn-core`：全部通过（含 5 项空测试改造后）
- `cargo test --workspace`：0 失败
- 双节点环回（T-A10）：双方认证成功
- **不涉及**：`ocn-compute`、`ocn-mind`、Android、模型文件

### 2.8 结论

> **B-1：待确认。** 规则本身应批准，但必须先选 B-1a / B-1b / B-1c。我的建议是 **B-1b**：零新依赖、不触碰 `Cargo.toml`、安全强度等价，且能直接满足三条硬性要求。若您坚持"证书 SAN 必须强制校验"，则选 B-1a，代价是新增 4 个左右传递依赖并需要一次完整的构建验证。

---

## 3. B-2 —— `AuthChallenge` v2 负载

### 3.1 涉及文件与函数

| 文件 | 项 | 行 | 改动 |
|---|---|---|---|
| `ocn-core/src/messages.rs` | `AuthChallengePayload` | 20–24 | 增加 3 个字段 |
| 同上 | **新增** `impl AuthChallengePayload { encode/decode }` | — | 显式编解码 |
| `ocn-core/src/quic.rs` | 服务端发送挑战 | 176–186 | 改用 `encode()` |
| 同上 | 客户端解析挑战 | 301–310 | 改用 `decode()`，校验版本 |
| `ocn-core/src/lib.rs` | `pub use messages::{...}` | 25 | 无需改（字段增加不影响再导出） |

### 3.2 接口变更

```rust
// 现有（messages.rs:20-24）——注意：当前没有任何生产代码使用它
pub struct AuthChallengePayload {
    pub server_challenge_nonce: [u8; 16],
    pub expires_in_secs: u32,
}

// 拟改为
pub struct AuthChallengePayload {
    pub payload_version: u8,              // = 2
    pub server_challenge_nonce: [u8; 16],
    pub server_node_id: String,           // 1..=128
    pub server_cert_fingerprint: [u8; 32],// DER 的 SHA-256 原始字节
    pub expires_in_secs: u32,             // 1..=300
}
impl AuthChallengePayload {
    pub fn encode(&self) -> Vec<u8>;
    pub fn decode(buf: &[u8]) -> anyhow::Result<Self>;
}
```

### 3.3 调用点统计

| 项 | 数量 | 说明 |
|---|---|---|
| `AuthChallengePayload` 生产使用 | **0** | 当前是死结构体，实际线格式是手工拼接（`quic.rs:178-180`） |
| 手工拼接点 | 1（`quic.rs:178-186`） | 改为 `encode()` |
| 手工解析点 | 1（`quic.rs:308-310`） | 改为 `decode()` |
| 结构体导出 | 1（`lib.rs:25`） | 字段增加，无外部构造者 |

**风险极低**：因为它现在根本没被使用，改它不会破坏任何现有行为。

### 3.4 修复前失败的测试

| ID | 修复前 |
|---|---|
| T-A3 | 失败：挑战里没有 `server_cert_fingerprint`，测试无法通过 |
| T-A4 | 失败：没有 `server_node_id` |

### 3.5 修复后成功标准

- 编码 → 解码 → 字段完全相等（含 128 字节边界 node_id）；
- `payload_version != 2` → `Err(AuthError::UnsupportedVersion)`；
- 截断负载（少 1 字节）→ `Err`，不 panic；
- 长度前缀声明 200 字节但实际 10 字节 → `Err`；
- 总长度 > 300 字节 → `Err`（防止内存放大）。

### 3.6 回归范围

`cargo test -p ocn-core`；环回 T-A10；不影响其他 crate。

### 3.7 结论

> **B-2：建议批准。** 纯负载扩展、零生产调用点、可独立单元测试。这是 B-1 的前置，无论选 a/b/c 都需要它。

---

## 4. B-3 —— 认证错误码

### 4.1 涉及文件与函数

| 文件 | 项 | 行 | 改动 |
|---|---|---|---|
| `ocn-core/src/messages.rs` | `AuthResultPayload` | 32–36 | 增加 `error_code`、`server_node_id` |
| 同上 | **新增** `enum AuthErrorCode` | — | 1–7 |
| `ocn-core/src/quic.rs` | 服务端构造 AuthResult | 211–221 | 填充错误码 |
| 同上 | 客户端解析 AuthResult | 331–344 | 读取错误码并映射为 `AuthError` |
| `ocn-node/src/main.rs` | 客户端处理 | 209–223 | 当前匹配旧 `OCNMsgType`，B-5 后整块删除；退出码改由 `connect()` 的 `Err` 决定 |

### 4.2 接口变更

```rust
// 现有
pub struct AuthResultPayload { pub success: bool, pub message: String }

// 拟改为
pub struct AuthResultPayload {
    pub success: bool,
    pub error_code: u8,          // 0 = 无错误
    pub message: String,
    pub server_node_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("unsupported protocol version")]          UnsupportedVersion, // 1
    #[error("node identity mismatch: cert != claimed")] IdentityMismatch, // 2
    #[error("certificate fingerprint mismatch")]       FingerprintMismatch, // 3
    #[error("authentication proof mismatch")]          ProofMismatch,     // 4
    #[error("challenge expired or consumed")]          ChallengeInvalid, // 5
    #[error("inconsistent peer id (hello != auth)")]   PeerIdInconsistent, // 6
    #[error("internal error: {0}")]                    Internal(String),  // 7
}
```

### 4.3 调用点统计

| 项 | 数量 |
|---|---|
| 服务端编码 AuthResult | 1（`quic.rs:213-215`，手工） |
| 客户端解码 AuthResult | 1（`quic.rs:338-340`，手工） |
| `main.rs` 中旧分支 | 1（`:211-222`，B-5 时删除） |
| 既有测试引用 | 0 |

### 4.4 修复前失败 / 修复后标准

| 测试 | 修复前 | 修复后断言 |
|---|---|---|
| T-A9 | 失败：无 `error_code` | 错误 token → `error_code == 4` |
| T-A5 | 失败 | ID 不一致 → `error_code == 6` |
| T-A3 | 失败 | 指纹不符 → `error_code == 3`（TLS 层）或客户端本地 3 |
| T-A7 | 部分通过但不可区分 | 断言错误字符串含 `fingerprint mismatch`，且**只尝试 1 次** |

### 4.5 回归范围

`ocn-core` 全部 + 环回 T-A10/T-A11。**注意**：`main.rs:214` 和 `:220` 当前 `return Ok(())`，即认证被拒也返回成功——B-3 必须同时把这两处改为返回非 0，否则 `F-16`（静默成功）未修复。

### 4.6 结论

> **B-3：建议批准。** 它是"可诊断性"的基础：没有错误码，T-A3/T-A4/T-A9 永远只能看到 `closed by peer: 0`，测试无法写断言。

---

## 5. B-4 —— 测试先行

### 5.1 涉及文件

| 文件 | 改动 |
|---|---|
| `ocn-core/src/tests.rs` | 追加 8 个测试 + 重写 1 个 + 落地 5 个空测试 |
| `ocn-core/src/lib.rs` | 不改（`#[cfg(test)] mod tests;` 已存在，新测试放在同一模块内） |

**明确不动**：`ocn-compute`、`ocn-mind`、`ocn-node` 的测试。

### 5.2 为什么必须先行

当前基线里：

- 5 个测试是空函数（`tests.rs:139-162`）；
- 1 个测试把 bug 常量写进断言（`tests.rs:55`）。

如果先改实现再补测试，新测试会按"实现现状"书写，重演同样的问题。**先写红灯测试是唯一能保证断言针对需求而非实现的机制。**

### 5.3 红灯清单与判定方式

| ID | 测试内容 | 红灯判定 | 通过后断言 |
|---|---|---|---|
| T-A1 | golden transcript | 新增测试，首次运行**必然失败**（向量尚不存在，或计算值与预期不符） | 固定输入的 32 字节输出等于写死的 hex |
| T-A2 | 挑战提供 server_node_id | 失败：客户端 proof ≠ 服务端 proof | 两侧相等 |
| T-A3 | 指纹不符 | 失败：无字段/无校验 | `Err(FingerprintMismatch)`，未发送帧 |
| T-A4 | 节点名不符 | 失败 | `Err(IdentityMismatch)` |
| T-A5 | Hello/AuthResponse 不一致 | 失败 | `AuthResult.error_code == 6` |
| T-A6 | 证书未绑定身份 | 失败 | `error_code == 2` 或 TLS 拒绝 |
| T-A7 | 钉扎不符 | 部分通过（TLS 已拒）→ 强化断言后失败 | 一次退出 + 明确错误字符串 |
| T-A9 | 错误 token | 失败 | `error_code == 4` |
| T-A10 | 环回真实握手 | **当前已实机失败**（5 次重连） | 双方成功 + 连接存活 ≥ 5 s |
| T-A11 | 环回错误钉扎 | 失败（现表现为 5 次 `closed by peer: 0`） | 1 次失败即退出，退出码 ≠ 0 |

### 5.4 测试数量与"空测试"门禁

| 阶段 | 测试数 | 其中空测试 |
|---|---|---|
| 当前基线 | 66 | **5** |
| B0 完成后 | 74 ~ 75 | **必须为 0** |

**门禁写法（可机械检查）：**

```powershell
# 断言不存在空测试：函数体行数 <= 2 且不含 assert
Select-String -Path ocn-core\src\tests.rs -Pattern "async fn test_" -Context 0,3
```

任何 `async fn test_xxx() { /* 注释 */ }` 形式一律视为未完成。

### 5.5 回归范围

| 范围 | 命令 | 通过标准 |
|---|---|---|
| Core 单元 | `cargo test -p ocn-core` | 74–75 项全绿 |
| 全工作区 | `cargo test --workspace` | 0 失败 |
| 传输层 | 手工双节点环回 | 见 T-A10 |
| 编译告警 | `cargo build 2>&1` | 不得新增 `never read` 类告警 |

### 5.6 结论

> **B-4：建议批准。** 这是本次修复里唯一"防复发"的机制。没有它，B-1/B-2/B-3 的成果无法被证明，也无法被后人守住。

---

## 6. B-5 —— 统一 `FixedHeader`，移除 bincode 线上路径

### 6.1 17 个调用点逐行迁移表

**全部位于 `ocn-node/src/main.rs`。** 分类：删除 8、迁移 3、退役 6。

| # | 行 | 方向 | 当前消息 | 迁移动作 | 目标 msg_type | 理由 |
|---|---|---|---|---|---|---|
| 1 | 128 | S→C | `AuthResponse{replay}` | **删除** | — | 旧服务端 `match` 整块（117–177）被 B2 的新循环取代；认证已在 `start_server` 内完成 |
| 2 | 139 | S→C | `AuthResponse{token}` | **删除** | — | 同上 |
| 3 | 153 | S→C | `Echo` 回显 | **删除** | — | Echo 由 Ping/Pong 取代 |
| 4 | 163 | S→C | `HashTestResult` | **删除** | — | 由 `TaskResult`(12) 取代 |
| 5 | 167 | S→C | `Pong` | **迁移** | 7 | 搬进 B2 新循环，用 `send_frame(7, &ts)` |
| 6 | 171 | S→C | `ThroughputAck` | **退役** | 13/14 保留 | 吞吐基准默认不跑；如保留则编入 `--self-test` |
| 7 | 207 | C→S | `AuthRequest` | **删除** | — | `connect()` 内部已完成 Hello/Challenge/Proof，main 不再手工认证 |
| 8 | 209 | C→S | `recv AuthResponse` | **删除** | — | 同上 |
| 9 | 229 | C→S | `Echo` | **退役** | 9（保留） | 自测基准，非协议必要 |
| 10 | 230 | C→S | `recv Echo` | **退役** | — | 同上 |
| 11 | 246 | C→S | `HashTest` | **删除** | — | 由 B5 的 `submit_task`（msg 11/12）取代 |
| 12 | 248 | C→S | `recv HashTestResult` | **删除** | — | 同上 |
| 13 | 261 | C→S | `Ping` | **迁移** | 6 | `send_frame(6, &ts)  ` |
| 14 | 262 | C→S | `recv Pong` | **迁移** | 7 | `recv_frame(Duration)` 校验 msg_type==7 |
| 15 | 285 | C→S | `ThroughputStart` | **退役** | 13 | 默认路径移除 |
| 16 | 288 | C→S | `ThroughputChunk` | **退役** | 14 | 默认路径移除 |
| 17 | 289 | C→S | `recv Ack` | **退役** | — | 默认路径移除 |

**计数校验：** 删除 8（#1,2,3,4,7,8,11,12）+ 迁移 3（#5,13,14）+ 退役 6（#6,9,10,15,16,17）= 17 ✅

### 6.2 迁移后的 `main.rs` 结构

```
server 模式
  start_server() → 每次认证成功回调 (peer_node_id, QuicConnection)
    → spawn(conn_loop)         // B2
         recv_frame → 6(Ping)/11(Task)/8(Disconnect)

client 模式（--peer）
  transport.connect() → 已完成认证的 QuicConnection
    → submit_task(conn, dto)   // B5，msg 11/12
```

`main.rs` 从 304 行降到约 120 行：删除 117–177（旧服务端 match）、200–296（旧自测基准）。**这是净减少，不是新增复杂度。**

### 6.3 接口变更

```rust
// ocn-core/src/transport.rs —— 现在
#[async_trait]
pub trait ConnectionHandle: Send + Sync {
    async fn send(&mut self, packet: &OCNPacket) -> anyhow::Result<()>;
    async fn recv(&mut self) -> anyhow::Result<Option<OCNPacket>>;
    fn remote_addr(&self) -> SocketAddr;
}

// 改为
#[async_trait]
pub trait ConnectionHandle: Send + Sync {
    async fn send_frame(&mut self, msg_type: u16, payload: &[u8]) -> anyhow::Result<()>;
    async fn recv_frame(&mut self, timeout: Duration)
        -> anyhow::Result<Option<(u16, Vec<u8>)>>;   // None = 对端正常关闭
    fn remote_addr(&self) -> SocketAddr;
    fn peer_node_id(&self) -> Option<&str> { None }  // 默认实现，不破坏第三方 impl
}
```

语义改进：当前 `recv()` 把**超时**和**关闭**都返回 `Ok(None)`（`quic.rs:82,85`），无法区分。新接口规定 `Ok(None)` 仅表示干净关闭，超时返回 `Err(FrameError::Timeout)`。

### 6.4 是否升级协议版本

**结论：不升级，保持 `PROTOCOL_VERSION = 1`，不做兼容层。**

理由：

1. bincode 路径从未可用——认证在它之前就失败（`F-04`），没有任何真实客户端依赖它；
2. `FixedHeader::decode` 已经严格检查 magic 与 version（`proto.rs:56-63`），版本号变更会强制所有既有 golden 向量失效，收益为零；
3. 无部署对端，硬切换的兼容成本为 0。

**但要加"旧格式检测"，给出可诊断的错误：**

```rust
// 读取首 4 字节
if &first4 != b"OCN1" {
    // bincode 帧以 u32 长度开头，几乎必然命中这里
    warn!("Unsupported wire format from {}: not OCN1 (legacy bincode?)", peer);
    return Err(FrameError::WireFormatMismatch);
}
```

这比"magic 不匹配 → 直接断开"更有用：日志能立刻区分"版本不兼容"与"网络故障"。

### 6.5 兼容性影响矩阵

| 场景 | 影响 | 处置 |
|---|---|---|
| 旧 bincode 客户端连新服务端 | 被拒绝 | 明确日志 `WireFormatMismatch`，不静默 |
| 新客户端连旧服务端 | 旧端 `FixedHeader` 解不出（它读 u32 长度） | 无部署旧端，忽略 |
| 第三方 `ConnectionHandle` 实现 | 编译失败 | 全仓仅 `QuicConnection` 一个实现（`quic.rs:56`） |
| `ocn-core` 公开再导出 | `OCNPacket`/`OCNMsgType`/`MAX_PACKET_SIZE` 从 `lib.rs:21` 移除 | 需检查是否有外部 crate 依赖；当前工作区内 0 处引用（`ocn-compute` 不依赖 `ocn-core`） |
| `bincode` 依赖 | 不再使用 | 建议 B5 保留依赖、只删使用；依赖移除留到 B7 复核 |

### 6.6 分批落地建议（降低编译中断风险）

17 个点一次性改会让 `main.rs` 在整个过程中无法编译。建议分两步：

**B5-a（0.5 天）**
1. `transport.rs` 加新方法，**保留**旧的 `send/recv` 作为默认实现（内部调用 `send_frame`/`recv_frame`，仅用于过渡）；
2. `QuicConnection` 实现新方法；
3. 服务端 Ping/Pong 迁到新方法（#5）；
4. 编译通过，测试通过。

**B5-b（0.5 天）**
1. 删除服务端旧 `match` 块（#1–#4、#6）；
2. 删除客户端手工认证（#7、#8）；
3. 删除/退役自测基准（#9–#17）；
4. **删除**旧的 `send/recv` 默认实现；
5. 删除 `OCNPacket`/`OCNHeader` 及 `lib.rs` 的再导出；
6. 全量测试 + 环回。

这样任何一步失败都能停在可编译状态。

### 6.7 修复前失败的测试

| 测试 | 修复前 |
|---|---|
| 帧往返 0 B / 1 B / 64 KiB / 1.9 MB | 不存在（当前只有 bincode 路径） |
| 超限帧在分配前被拒 | 不存在 |
| 首 4 字节非 `OCN1` → `WireFormatMismatch` | 不存在 |

### 6.8 修复后成功标准

| 项 | 标准 |
|---|---|
| 帧往返 | 4 档长度逐字节相等 |
| 超限 | `payload_size = 2 MiB + 1` → 在 `vec![0; n]` **之前**返回 `Err`（用分配计数或直接代码审查确认） |
| 旧格式 | 发送 bincode 前缀 → 服务端日志含 `WireFormatMismatch`，连接关闭，不 panic |
| 编译 | `cargo build --workspace` 0 error，无新增 warning |
| 环回 | T-A10 + Ping/Pong ≤ 1 s |

### 6.9 回归范围

`cargo test --workspace`；`ocn-core/src/tests.rs:132` 的 `test_packet_size_limit_protection` 需改写为帧超限测试；手工环回。

### 6.10 结论

> **B-5：建议批准，但按 B5-a / B5-b 两步执行。**
> 不同时保留两套格式是正确决定——两套格式的读写语义不同（一问一答 vs 单流单消息），共存会产生难以复现的半包问题。不升版本号也是正确的：没有可兼容的对端，升级只会让 golden 向量失效。

---

## 7. B-8 —— BatchHash 规范（发送端 / 执行端 / 验证端必须完全一致）

### 7.1 定义

| 项 | 定义 |
|---|---|
| **任务类型** | `TaskType::BatchHash`（唯一类型，`task.rs:14-16`） |
| **输入** | `payload: Vec<u8>`，长度 `0 ..= 1_900_000`（`TASK_MAX_PAYLOAD`）。**空载荷合法。** |
| **`input_hash`** | `SHA-256(payload)`，32 字节**原始字节**上线（不是 hex） |
| **执行** | `digest = SHA-256(payload)`，流式计算，64 KiB 分块 |
| **输出 `result`** | `digest`，固定 32 字节 |
| **`result_hash`** | `SHA-256(result)` = `SHA-256(SHA-256(payload))` |
| **`execution_time_ms`** | 执行端测量，`>= 1`（0 时置 1） |
| **无盐、无前缀、无换行** | 直接对字节计算，避免两端理解偏差 |

### 7.2 三方一致性

```
发送端（Windows coordinator）
  1. payload = 原始字节
  2. input_hash = SHA256(payload)
  3. 发送 TaskRequest{input_hash, payload}

执行端（Android/Windows worker）
  4. 收到后重算 SHA256(payload)，必须 == input_hash，否则 Rejected/1
  5. result = SHA256(payload)              // 注意：与 input_hash 值相同
  6. result_hash = SHA256(result)
  7. 发送 TaskResult{result, result_hash, ...}

验证端（Windows coordinator）
  8. SHA256(result) == result_hash ?        // 传输完整性
  9. result.len() == 32 ?                   // 结构
 10. result == input_hash ?                 // 语义正确性（本地已有 input_hash）
```

**第 10 步是最关键的独立验证：** coordinator 不需要再算一次 payload 的哈希——它在发送前已经算过 `input_hash`，直接比对即可。这一步能发现"worker 返回了错误内容但哈希自洽"的情况（例如返回了别的任务的摘要）。

> 注意第 10 步成立的前提正是方案 A（`result = digest`）。若采用方案 B（`result = payload`），coordinator 需要重算 `SHA256(result)` 并与 `input_hash` 比对，传输量等于输入大小。两者都可验证，但 A 的传输量是常数 32 字节。

### 7.3 Golden 向量（已用 PowerShell 独立计算，可直接写入测试）

**payload = 空（0 字节）**

```
input_hash  = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
result      = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
result_hash = 5df6e0e2761359d30a8275058e299fcc0381534545f55cf43e41983f5d4c9456
```

**payload = "abc"（UTF-8，3 字节，0x61 0x62 0x63）**

```
input_hash  = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
result      = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
result_hash = 4f8b42c22dd3729b519ba6f68d2da7cc5b2d606d05daed5ad5128cc03e6c6358
```

这两个向量覆盖了"空输入"与"短输入"两个边界，且 `input_hash == result` 的关系被显式写死——如果实现里错误地多算了一层或多加了长度前缀，向量立刻失败。

**1 MB 载荷不建议写死 hex**，改用属性测试：`result == SHA256(payload)` 且 `result_hash == SHA256(result)`。

### 7.4 错误码（与 §4 的认证错误码分属不同命名空间）

| 码 | 场景 | 状态 |
|---|---|---|
| 1 | 校验失败（input_hash 不匹配、字段越界） | Rejected |
| 2 | 资源拒绝 | Rejected |
| 3 | 不支持的任务类型 | Rejected |
| 4 | 执行失败 | Failed |
| 5 | 同一幂等键正在执行 | Rejected |
| 6 | 截止时间已过 | Failed |
| 7 | 内部错误 | Failed |
| **8** | 幂等键冲突（同 key 不同 input_hash） | Rejected |
| **9** | 结果语义不符（第 10 步失败） | Failed |

### 7.5 对现有测试的影响

| 测试 | 影响 |
|---|---|
| `ocn-compute/src/runtime.rs:91-108` `test_batch_hash_execution` | `:107` 断言 `result.result == payload.to_vec()` 需改为 `result.result == input_hash` |
| `ocn-compute/src/runtime.rs:110-118` `test_empty_payload` | 不变（只断言 status 与 validate） |
| `ocn-compute/src/runtime.rs:120-128` `test_large_payload` | 不变 |
| `ocn-compute/tests/integration_tests.rs:35` `test_integration_normal_batch_hash` | 不变（只断言 status/validate） |
| 其余 62 项 | 不受影响 |

**净影响：1 条断言。** 这是选择方案 A 的额外好处——改动面被限制在最小。

### 7.6 潜在反对意见与回应

| 反对 | 回应 |
|---|---|
| "返回 32 字节，怎么证明算的是真实工作量？" | 不能证明算力消耗——但当前返回原始 payload 同样不能。真要证明工作量需要 PoW（哈希碰撞难度），那是另一个特性，不在 3.1 范围 |
| "下游如果期待拿到数据副本怎么办？" | 那是 `Echo`/`Transfer` 任务类型，应新增 `TaskType` 而不是让 BatchHash 兼任 |
| "改动会影响已有测试" | 只影响 1 条断言，且该断言本来就是错的（把"回显"当成"哈希"） |

### 7.7 结论

> **B-8：建议批准方案 A。** 定义自洽（三层哈希关系明确）、验证可独立完成（不需要重算大载荷）、传输量为常数、测试影响仅 1 行。方案 B 虽然改动更小，但语义上"BatchHash 返回原数据"是自相矛盾的，会把命名问题留到下一阶段。

---

## 8. C-1 与 C-4 纳入实施要求

### 8.1 C-1 —— `HeuristicTrendProvider` 标识

**实施时点：** B0 完成后、B1 开始前（独立小改动，不阻塞主链）。

| 要求 | 具体内容 | 验收 |
|---|---|---|
| 标识 | 结果元数据必须含 `inference_mode = "heuristic_trend_v1"`、`data_source = "SIMULATED"` | 单元测试断言两个键都存在 |
| 命名 | 文档与日志统一称"启发式趋势预测"；代码重命名可选（`GraniteTTMProvider` → 保留类名但加 `is_simulated()` 方法返回 `true`） | 代码审查 |
| 禁止 | 任何报告/日志/注释出现"TTM 模型推理"描述当前路径 | grep 检查 |
| 兼容 | 不改 `MindProvider` trait 签名 | 编译通过 |

**为什么放在 B0 之后：** B0/B0.5 是协议修复，与 Mind 无关；先完成协议再动 Mind 可以避免两条线互相干扰。C-1 本身 0.5 天。

### 8.2 C-4 —— 准确率声明限制

**作为硬性门禁写入 B5 与 B7 的验收条件：**

| 门禁 | 规则 |
|---|---|
| G-1 | 在 `MetricHistory` 环形缓冲（512 点真实数据）完成前，任何输出都不得附带准确率、MAPE、命中率等指标 |
| G-2 | 设备端 E2E（B7）报告只能陈述"哈希一致 / 往返时延"，不得陈述"预测准确" |
| G-3 | 任何引用了 `inference_mode = heuristic_trend_v1` 的结果，报告中必须同时标注"非模型推理" |
| G-4 | 真实前向（T-1…T-3）完成且与参考实现误差 < 1e-5 之前，不得把自研前向结果标为 `real_forward_pass` |
| G-5 | 设备性能数字（延迟/吞吐）只能来自 B7 实测，且必须附设备型号、温度、负载条件 |

**检查方式：** B5/B7 交付报告若出现"准确率""预测精度""TTM 效果"等字样且无对应数据来源，视为未通过验收。

### 8.3 两项的相互关系

```
C-1（标识）  ──> 让 C-4（限制）可执行
C-4（门禁）  ──> 防止 C-1 被绕过（改个名字但继续宣称准确）
```

缺一不可：只做 C-1，报告仍可写"启发式趋势预测准确率 92%"；只做 C-4，无法机械检查哪条路径是模拟的。

---

## 9. 最终审批建议表

| ID | 建议 | 理由 | 前置 |
|---|---|---|---|
| **B-1** | **待确认** | 规则正确，但严格 SAN 需新增依赖（`rcgen/x509-parser` 及其传递依赖，`Cargo.lock` 中目前不存在）。请选 a/b/c；建议 **b**（配对记录，零依赖，安全强度等价） | B-2 先落地（挑战里需要携带服务器身份） |
| **B-2** | **建议批准** | 死结构体转为真实负载；0 生产调用点；可独立测试 | 无 |
| **B-3** | **建议批准** | 把"closed by peer: 0"变成可诊断错误码；顺带修复 `main.rs:214/220` 的静默成功 | 无 |
| **B-4** | **建议批准** | 唯一能防止"空测试"复发的机制；机械可检查 | B-1/B-2/B-3 的红灯测试依赖它 |
| **B-5** | **建议批准（分两步）** | 17 点 = 8 删 + 3 迁 + 6 退役；净减约 180 行；不升版本是对的；加 `WireFormatMismatch` 检测 | B-1/B-2/B-3 |
| B-6 | 建议批准 | 三常量冲突会导致 5 MB 任务通过校验却传不出去 | B-5 |
| B-7 | 建议批准 | 当前无空闲超时，连接可能永久挂起 | B-5 |
| **B-8** | **建议批准方案 A** | 三层哈希关系自洽；验证端可用已持有的 `input_hash` 独立比对；仅 1 条断言需改 | 无（可与 B-5 并行设计） |
| B-9 | 建议批准 | 修 "同 key 不同 payload 命中错误结果" | B-4（缓存需存 input_hash） |
| B-10 | 建议批准 | 顺序与依赖一致 | — |
| B-11 | 建议批准 | 与资源管理设计一致 | — |
| B-12 | 建议批准 | `SystemMetrics` 当前无生产者 | B-4 |
| **C-1** | **建议批准** | 0.5 天，消除虚假声明 | B0 之后 |
| **C-4** | **建议批准** | 与 C-1 互补，防止改名后继续宣称准确率 | 与 C-1 同时 |

---

## 10. 若全部批准，第一步做什么

**只做这一件事，做完即停：**

```powershell
# 1. 建立真实基线（不改代码）
cargo test --workspace --no-fail-fast
```

记录：实际测试数、是否仍为 5 个空测试、是否有告警。

**然后写 B-1/B-2/B-3 的红灯测试，运行，确认全部失败，停下来报告红灯清单。**

在红灯清单被您确认之前，**不写任何实现代码**。

---

## 11. 合规声明

- 未修改任何 Rust / Python / Android / 配置文件（本文件为新增文档）。
- 未执行 `git init/add/commit/reset/clean/checkout`。
- 未运行 `cargo build` / `cargo test`。
- 未对真实设备执行任何操作。
- 未开始 B0，未创建任何 `.rs` 文件。

**当前状态：** 根目录非 Git 仓库；`.git` 目录数 2；无 `ocn-node` 残留进程；恢复点 `OCN_20261123_1930` 完好（131 文件 / 4,740,867 B）。
