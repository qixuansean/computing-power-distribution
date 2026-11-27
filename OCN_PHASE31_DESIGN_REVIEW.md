# OCN Phase 3.1 设计复审（D1–D7）

**日期：** 2026-11-23
**状态：** 设计复审。**未修改任何业务代码，未执行任何 Git 操作。**
**前置：** `OCN_EXTENDED_AUDIT.md`（F-04…F-16）、`OCN_BACKUP_REPORT.md`（恢复点已建立）
**证据标签：** `VERIFIED` / `PARTIAL` / `MISSING` / `FAILED` / `UNVERIFIED`

---

## 0. 复审方法与一处必须更正的基线

本轮逐行重读了 `quic.rs`（90–362）、`identity.rs`（1–233）、`auth.rs`（证明函数全文）、`messages.rs`、`proto.rs`（1–307）、`task.rs`、`runtime.rs`、`deduplication.rs`、`resource_manager.rs`、`tests.rs`、`main.rs`（1–304）。

**基线更正（重要）：** 既有 66 项测试中有 **5 项是空测试**，函数体内只有注释，不做任何断言：

```rust
// ocn-core/src/tests.rs
139: async fn test_node_info_exchange()            { /* PENDING: ... */ }
145: async fn test_invalid_token_rejection()       { /* PENDING: ... */ }
150: async fn test_duplicate_challenge_rejection() { /* PENDING: ... */ }
155: async fn test_unauthenticated_ping_rejection(){ /* PENDING: ... */ }
160: async fn test_malformed_header_rejection()   { /* PENDING: ... */ }
```

因此**有效断言的测试数是 61 项，不是 66 项**。这 5 项恰好覆盖本次要修的认证与协议行为，属于"测试名承诺了覆盖，实际没有覆盖"。任何后续报告若引用 "66 项测试保护了协议层" 都是不准确的。

此外 `test_hkdf_hmac_auth_proof_calculation`（`tests.rs:44-63`）把错误的常量 `"node-server"` 写死在断言里，测试通过只能证明"同一函数两次调用结果相同"，**它锁定了 bug 而不是防止了 bug**。

---

## 1. 先定义三种身份（问题根源）

当前代码把三者混用，这是 `F-04` 的根本原因。

| 身份 | 定义 | 当前产生位置 | 用途（应当） |
|---|---|---|---|
| **证书身份** `cert_fp` | `SHA-256(DER 证书)`，64 位小写 hex | `identity.rs:25-28`、`:44-51` | 只用于**钉扎/比对**，不是节点名 |
| **节点身份** `node_id` | 人类可读名称，已作为证书 SAN DNS 名写入 | `identity.rs:19`（`SanType::DnsName(node_id)`） | 协议中的实体标识 |
| **预期对端身份** `expected_peer` | 配置的钉扎值：至少 `cert_fp`，可选 `node_id` | `quic.rs:98-102`、`main.rs` 的 `--peer-fingerprint` | 决定"我是否允许这个对端" |

关键事实：`node_id ↔ cert` 的绑定**已经存在于证书里**（SAN），但**两端都没有校验它**。

- 服务端 `verify_client_cert`（`identity.rs:86-102`）在 `allowed = None` 时直接 `Ok(assertion())`，**不检查 SAN、不检查链**（自签场景下也无从检查）。→ 任何自签证书都能通过 TLS。
- 客户端 `compute_auth_proof` 的 `server_node_id` 取自 `expected_peer_fingerprint.unwrap_or("node-server")`（`quic.rs:317`），把**证书指纹**当成**节点名**使用。
- 服务端用真实 `identity.node_id` 计算（`quic.rs:206`）。

结论重复一遍，避免后续误读：**这不是"配置错了"，而是"没有任何配置能让两端一致"**——把 64 位 hex 指纹填进 `--peer-fingerprint`，客户端算出的 transcript 里服务器名就是那串 hex，而服务端算的是 `node-A`。

---

## 2. 认证缺陷登记

| ID | 缺陷 | 证据 | 状态 |
|---|---|---|---|
| A-1 | 客户端把证书指纹/常量当作 `server_node_id` | `quic.rs:317` vs `quic.rs:206` | `FAILED`（已实机复现，5 次重连全败） |
| A-2 | 服务端在未钉扎时接受任意自签客户端证书，且不校验 SAN | `identity.rs:86-102` | `FAILED` |
| A-3 | `client_node_id` 未与证书绑定；Hello 与 AuthResponse 中的 ID 可以不同（服务端忽略 `_resp_node_id`） | `quic.rs:193` | `FAILED` |
| A-4 | 服务端身份没有在挑战中传递；客户端只能"猜" | `messages.rs:21-24` 的 `AuthChallengePayload` **未被使用**；实际线格式是手工拼接 `nonce||expiry`（`quic.rs:178-186`） | `FAILED` |
| A-5 | 认证失败时只发 `AuthResult{success:false}` 然后静默断开，客户端只看到 `closed by peer: 0`，无法区分"token 错"与"绑定错" | `quic.rs:211-221`、`quic.rs:252-254` | `PARTIAL` |
| A-6 | 节点身份存于 `%TEMP%\ocn_identities`（`main.rs:41`），临时目录清理会导致指纹漂移，钉扎失效 | `main.rs:41`、`identity.rs:38-65` | `PARTIAL` |
| A-7 | 5 项协议/认证测试为空；1 项断言写死错误常量 | `tests.rs:44-63`、`:139-162` | `FAILED`（覆盖假象） |

---

## 3. 认证绑定规则（权威定义）

以下为**规范**，实现必须逐条满足：

**R-1（节点名可信条件）** 只有在同时满足下列三条时，才接受对端声明的 `node_id`：
1. TLS 握手成功（QUIC + 证书交换完成）；
2. 对端证书 SAN 中**恰好一个 DNS 名**且等于声明的 `node_id`；
3. 若配置了钉扎，则 `SHA-256(peer_cert_der)` 等于钉扎值。

**R-2（指纹不得冒充节点名）** `cert_fp` 只能出现在 `peer_cert_fingerprint` 字段或日志标签中。任何把 `cert_fp` 赋给 `node_id` / `server_node_id` / `client_node_id` 的代码都视为缺陷。

**R-3（transcript 不变）** `compute_auth_proof` 的字节布局保持不变（`auth.rs` 已包含 `Len(ServerNodeID)||ServerNodeID`）。改变的是**取值来源**：客户端从挑战负载读取 `server_node_id`，并先用规则 R-1 校验它，再用于计算。

**R-4（服务端以证书为准）** 服务端从 `connection.peer_identity()` 取出客户端证书，解析 SAN 得到 `cert_node_id`；若 Hello 中的 `client_node_id != cert_node_id`，或 AuthResponse 中的 ID 与 Hello 不一致，直接拒绝（错误码见 §5）。

**R-5（双向）** 客户端同样校验服务端证书 SAN 与挑战中的 `server_node_id` 一致；不一致即中止，不再发送 AuthResponse。

**R-6（身份持久化）** Android 端身份必须写入应用私有目录（`Context.getFilesDir()/ocn/identity`），不得使用系统临时目录。

---

## 4. 认证消息线格式（AuthChallenge v2）

统一使用 12 字节 `FixedHeader`（见 §6）。`msg_type = 2`。负载（大端）：

| 偏移 | 字段 | 类型 | 约束 |
|---|---|---|---|
| 0 | `payload_version` | `u8` | 必须为 `2` |
| 1 | `server_challenge_nonce` | `[u8;16]` | CSPRNG |
| 17 | `server_node_id` | `u32 len + UTF-8` | 1–128 字节 |
| … | `server_cert_fingerprint` | `[u8;32]` | 服务端 DER 的 SHA-256 原始字节 |
| … | `expires_in_secs` | `u32` | 1–300 |

`AuthResult`（`msg_type = 4`）负载扩展为：

| 偏移 | 字段 | 类型 | 说明 |
|---|---|---|---|
| 0 | `success` | `u8` | 0/1 |
| 1 | `error_code` | `u8` | 0 = 无错误；见 §5 |
| 2 | `message` | `u32 len + UTF-8` | 人类可读原因 |
| … | `server_node_id` | `u32 len + UTF-8` | 成功时为服务端 ID，便于客户端记录 |

> 兼容性：当前无任何已部署对端，且 v1 在逻辑上不可用（A-1）。因此采用**硬切换**，不实现双版本协商。`payload_version` 字段为将来预留；若客户端读到非 2，必须报错退出而不是猜测。

---

## 5. 认证错误码（v1）

| 码 | 含义 | 触发点 |
|---|---|---|
| 1 | 协议版本不支持 | 挑战 `payload_version != 2` |
| 2 | 节点名与证书 SAN 不符 | 服务端 R-4 / 客户端 R-5 |
| 3 | 证书钉扎不匹配 | TLS 层（`identity.rs:99`、`:176`） |
| 4 | 认证证明不匹配（token 错/transcript 错） | `quic.rs:209` |
| 5 | 挑战过期或已被消费 | `PendingChallengeStore` |
| 6 | 对端 ID 前后不一致（Hello vs AuthResponse） | `quic.rs:193` 修复后 |
| 7 | 内部错误 | 解析失败等 |

客户端必须把 4 与 3 区分开来——当前两者都表现为 `closed by peer: 0`。

---

## 6. 测试先行（认证）

**顺序要求：先写测试并确认红灯，再改实现。** 下表每一项都必须能在修改前运行失败。

| ID | 层 | 测试内容 | 修复前预期 | 通过标准 |
|---|---|---|---|---|
| T-A1 | 单元 | `compute_auth_proof` 固定输入 → 固定 32 字节十六进制 | 新增，无向量 | 输出等于写死的 golden hex；任何 transcript 改动都会使其失败 |
| T-A2 | 单元 | 客户端 `server_node_id` 来自挑战而非常量 | 以 `"node-A"` 为服务端名时 proof ≠ 服务端 proof | 两侧 proof 相等 |
| T-A3 | 单元 | 挑战中的 `server_cert_fingerprint` 与实际证书不符 | 无此校验 | 客户端返回错误码 2/3，不发送 AuthResponse |
| T-A4 | 单元 | 挑战中的 `server_node_id` 与证书 SAN 不符 | 无此校验 | 客户端中止，错误码 2 |
| T-A5 | 单元 | Hello 声称 `node-X`，AuthResponse 声称 `node-Y` | 服务端忽略差异 | 服务端拒绝，错误码 6 |
| T-A6 | 单元 | 客户端证书 SAN 与其声明 ID 不符 | 服务端接受 | 服务端拒绝，错误码 2 |
| T-A7 | 单元 | 配置了钉扎，但客户端证书指纹不同 | TLS 层已拒绝（`identity.rs:99`） | 握手失败，错误为"钉扎不匹配"而非静默断开 |
| T-A8 | 单元 | 同一挑战重放 | 已有 `PendingChallengeStore` 测试 | 第二次被拒，错误码 5 |
| T-A9 | 单元 | 错误 pairing token | 会有 proof 不匹配，但只表现成断开 | 返回错误码 4，客户端日志可区分 |
| T-A10 | 传输 | 双节点环回真实握手 | **当前失败**（5 次重连） | 服务端与客户端同时打印认证成功；连接保持 ≥ 5 s |
| T-A11 | 传输 | 环回 + 错误钉扎 | 5 次 `closed by peer: 0` | 一次失败即退出，退出码非 0，错误信息明确 |

**T-A10 的成功判据必须同时满足**：同一进程对中，服务端出现 `authenticated successfully`，客户端出现 `Client authenticated successfully`，且客户端不再出现 `Connection attempt ... failed`。

---

## 7. D1 —— 认证身份绑定：逐项审查

| 维度 | 内容 |
|---|---|
| **修改文件/函数** | `ocn-core/src/quic.rs`：`start_server` 内 Hello 分支（136–186）、AuthResponse 分支（188–257）、`connect`（274–362）；`ocn-core/src/identity.rs`：新增 `peer_node_id_from_cert()`、`verify_client_cert` 增加 SAN 校验；`ocn-core/src/messages.rs`：`AuthChallengePayload` 增加 `server_node_id`/`server_cert_fingerprint`/`payload_version`；`ocn-node/src/main.rs`：身份目录 + 退出码 |
| **现有接口** | `compute_auth_proof(session_key, server_challenge, client_nonce, client_node_id, server_node_id) -> [u8;32]`（不变）；`Transport::start_server/connect`（不变）；`ConnectionHandle`（见 §8 变更） |
| **新接口** | `NodeIdentity::peer_node_id_from_cert(der: &[u8]) -> Result<String, IdentityError>`；`NodeIdentity::fingerprint_of(der: &[u8]) -> String`；`AuthChallengePayload::{encode,decode}` 显式实现（替换手工拼接） |
| **协议字段** | 见 §4；**不新增 msg_type**，只扩展 2 与 4 的负载 |
| **Windows 职责** | 作为 coordinator 时校验服务端 SAN；作为 worker 时校验客户端 SAN 并签发挑战 |
| **Android 职责** | 同 worker；身份持久化到应用私有目录；不得把指纹当节点名 |
| **安全风险** | ① 硬切换导致旧客户端无法连接——当前无部署对端，可接受；② SAN 解析需防御畸形证书（多 SAN、非 DNS 类型）→ 必须要求"恰好一个 DNS SAN"；③ 若实现方为省事跳过 R-4，则 A-3 的冒充面仍在 |
| **兼容性** | 线格式变化仅限挑战/结果负载，`FixedHeader` 与版本号 1 不变；`compute_auth_proof` 字节布局不变，golden 向量可跨版本复用 |
| **对 66 项测试影响** | `test_hkdf_hmac_auth_proof_calculation`（`tests.rs:44-63`）需重写为 golden 向量；5 项空测试（`tests.rs:139-162`）需落地为真实测试。**测试总数上升，通过数不再虚高** |
| **新增测试** | T-A1 … T-A11 |

---

## 8. D2 —— 协议格式统一：逐项审查

### 8.1 唯一线格式

```
┌────────┬─────────┬──────────┬──────────────┐
│ magic  │ version │ msg_type │ payload_size │  12 字节，大端
│ 4 B    │ u16     │ u16      │ u32          │
└────────┴─────────┴──────────┴──────────────┘
payload: payload_size 字节，按 msg_type 各自定义
```

`magic = "OCN1"`，`version = 1`（`proto.rs:6-8`、`:46-53`）。

**规范：所有线上流量必须使用该格式。** 现有的 `u32 长度 + bincode` 路径（`quic.rs:60-87`）**必须移除**，而不是"保留但标记废弃"——因为用户要求不能保留两套不兼容格式，而且两套格式的 `ConnectionHandle` 语义（一问一答 vs 单流单消息）不同，混用会产生难以定位的半包问题。

### 8.2 消息号注册表

| 号 | 名称 | 方向 | 负载 | 上限 |
|---|---|---|---|---|
| 1 | Hello | C→S | `u32 node_id + u16 version + [16] nonce` | 64 KiB |
| 2 | AuthChallenge | S→C | §4 | 64 KiB |
| 3 | AuthResponse | C→S | `u32 node_id + [32] proof` | 64 KiB |
| 4 | AuthResult | S→C | §4 | 64 KiB |
| 5 | NodeInfo | S→C | `node_id, node_name, platform_id, u16 ver, vec<string> caps` | 64 KiB |
| 6 | Ping | 双向 | `u64 timestamp_ms` | 64 KiB |
| 7 | Pong | 双向 | `u64 timestamp_ms` | 64 KiB |
| 8 | Disconnect | 双向 | `u32 reason` | 64 KiB |
| 11 | TaskRequest | 协调者→工作节点 | §8.4 | `TASK_MAX_PAYLOAD` |
| 12 | TaskResult | 工作节点→协调者 | §8.5 | `TASK_MAX_PAYLOAD` |

保留 9、10 给后续 (Echo/Throughput)，不得复用。

### 8.3 长度上限（消除三套冲突常量）

| 常量 | 值 | 作用域 | 取代 |
|---|---|---|---|
| `MAX_FRAME_PAYLOAD` | 2 MiB | 所有帧的硬上限，`read_fixed_frame` 使用 | `MAX_PACKET_SIZE`（10 MB，`proto.rs:12`）用于**线上**的部分 |
| `CONTROL_MAX` | 64 KiB | 控制类消息（1–8） | `CONTROL_MAX_PAYLOAD_SIZE`（保留语义） |
| `TASK_MAX_PAYLOAD` | 1,900,000 B | 11/12 负载 | `ocn-compute` 的 `MAX_PAYLOAD_SIZE`（10 MB，`task.rs:5`） |

`TASK_MAX_PAYLOAD` 取 1.9 MB 的原因：`FixedHeader(12) + task 头部字段（约 200 B）` 之后仍要落在 2 MiB 帧上限内，留出余量避免边界抖动。

**规则：解码先读 header，比对 `payload_size` 与上限，再分配内存。** `read_fixed_frame`（`proto.rs:216-239`）已经这样做了，问题只是三个常量不一致。

### 8.4 TaskRequest（11）负载

| 字段 | 类型 | 约束 |
|---|---|---|
| `task_id` | str | 1–128 |
| `task_type` | u8 | 1 = BatchHash |
| `protocol_version` | u16 | = 1 |
| `input_hash` | `[u8;32]` | `SHA-256(payload)` |
| `priority` | u8 | 0–100 |
| `deadline_unix_ms` | u64 | now < x ≤ now+24h |
| `timeout_ms` | u32 | 1,000–3,600,000 |
| `cpu_cores` | u16 | 1–256 |
| `memory_mb` | u32 | 1–1,048,576 |
| `gpu_required` | u8 | 必须 0 |
| `idempotency_key` | str | 1–128 |
| `payload` | blob | ≤ `TASK_MAX_PAYLOAD` |

### 8.5 TaskResult（12）负载

| 字段 | 类型 | 约束 |
|---|---|---|
| `task_id` | str | 回显 |
| `node_id` | str | 工作节点真实 ID（来自 R-1，不是自报） |
| `status` | u8 | 0 Success / 1 Failed / 2 Rejected |
| `error_code` | u8 present + u32 | Success 时 present=0 |
| `execution_time_ms` | u64 | ≥ 1 |
| `result_hash` | `[u8;32]` | `SHA-256(result)` |
| `result` | blob | ≤ `TASK_MAX_PAYLOAD` |

### 8.6 哈希规则（必须写进实现注释）

- `input_hash = SHA-256(payload)` —— 已实现且已测试（`task.rs:122-127`）。
- `result_hash = SHA-256(result)` —— 已实现且强制校验（`task.rs:148-153`）。
- **BatchHash 的语义缺陷（需决策）：** 当前 `ComputeExecutor::execute_batch_hash` 返回 `result = payload`（`runtime.rs:43`），即"把输入原样送回"。这对名为 BatchHash 的任务是错的，也意味着传输量翻倍。

两种候选：

| 方案 | `result` | 协调者校验 | 传输量 | 测试影响 |
|---|---|---|---|---|
| **A（推荐）** | 32 字节摘要 | `SHA-256(result)` 校验 + 与本地重算的 `SHA-256(payload)` 比对 | 极小 | `runtime.rs:107` 一条断言需改 |
| B | 原始 payload | 仅校验 `result_hash` | = 输入大小 | 无 |

选 A 的理由：真正验证了"远端确实计算过"，且避免 1.9 MB 结果回传。代价是 `ocn-compute/src/runtime.rs:91-108` 的 `assert_eq!(result.result, payload.to_vec())` 必须改为与摘要比较——我核对了 `integration_tests.rs`，它只断言 `status` 与 `validate()`，不受影响。

### 8.7 超时与断线语义

| 阶段 | 超时 | 处置 |
|---|---|---|
| TLS/QUIC 握手 | quinn 默认 | 失败即重试（最多 5 次，1 s 间隔） |
| 每个握手帧 | 10 s | 关闭连接，错误码 7 |
| 认证总时长 | 30 s（挑战 TTL） | 过期挑战拒绝，错误码 5 |
| 空闲连接 | 60 s（新增） | 发 Ping，2 次无 Pong 则断开 |
| 单任务 | `min(timeout_ms, deadline - now)` | 到期返回 `Failed` + 错误码 6 |
| 单帧读取 | `min(任务剩余, 30 s)` | 超时视为传输失败 |

**断线后的任务状态：** 协调者维护 `pending: Map<idempotency_key, TaskState>`。传输失败时状态置 `Unknown`（不是 `Failed`），用**同一** `idempotency_key` 重试，最多 2 次（退避 250 ms / 1 s）。工作节点的去重缓存保证重试不会重复执行。若重试耗尽，向上层返回 `Unknown`，由调用者决定是否放弃——**不得**伪装成成功。

### 8.8 幂等键与重复执行

现状：`TaskDeduplicationCache` 只以 `idempotency_key` 为键（`deduplication.rs:39`），且**不存 `input_hash`**。这意味着同一个 key 配不同 payload 会命中错误的结果。

规范：

1. 键 = `idempotency_key`；
2. 条目内必须存 `input_hash`；
3. 命中时：若 `input_hash` 相同 → 按状态返回（`AlreadyRunning` → 错误码 5；`Completed` → 重放结果）；
4. 若 `input_hash` 不同 → **拒绝，错误码 8 `IdempotencyConflict`**，不得执行；
5. TTL = `max(timeout_ms, 60 s)`；容量上限由配置给出（默认 4096）。

这是对 `deduplication.rs` 的真实修改点，需要在 B4 中落地并新增测试。

### 8.9 D2 影响面

| 项 | 变化 |
|---|---|
| `Transport` trait | 不变 |
| `ConnectionHandle` | `send/recv` → `send_frame(u16, &[u8])` / `recv_frame(Duration) -> (u16, Vec<u8>)`；**破坏性变更** |
| 调用点 | `ocn-node/src/main.rs` 共 11 处（128,139,153,163,167,171,207,209,229,230,246,248,261,262,285,288,289）；`quic.rs:251` 的 dummy 包移除 |
| `OCNPacket`/`OCNHeader`/bincode | 从线上路径移除；类型可保留给单元测试或删除 |
| `ocn-core/src/tests.rs` | `test_packet_size_limit_protection`（132-137）依赖 `OCNPacket::serialize`，若删除该类型需同步改写为帧编码测试 |

---

## 9. D3 —— 长度上限：审查结论

见 §8.3。补充：

- `proto.rs:10-12` 三个常量并存且无注释说明各自适用场景，是当前"2 MB 还是 10 MB"混乱的来源。
- `ocn-compute/src/task.rs:5` 的 `MAX_PAYLOAD_SIZE = 10 MB` 与 `proto.rs:11` 的 2 MB 直接冲突：一个 5 MB 任务能通过 `TaskDescriptor::validate()`，却必然在传输层被拒。
- 修复方式不是"改数字"，而是**单一来源**：`ocn-core` 定义 `MAX_FRAME_PAYLOAD` 与 `TASK_MAX_PAYLOAD`，`ocn-compute` 依赖并引用，不再自带常量。

**验收测试：** 1,899,999 / 1,900,000 / 1,900,001 三档；第三档必须在**分配内存前**被拒（通过断言 `payload_size` 检查先于 `vec![0; n]`）。

---

## 10. D4 —— 服务端认证后循环

现状：`quic.rs:244-251` 构造 `QuicConnection` 后向 channel 发送一个**伪造的** `AuthResponse` 包，随后 `start_server` 的任务结束，连接没有任何读取循环；`ocn-node` 的 `main.rs:112-179` 只处理旧的 `OCNMsgType` 变体。→ 连接在发完 NodeInfo 后即被丢弃。

目标结构：

```
认证成功
  → 发送 NodeInfo(5)
  → loop {
        match recv_frame(remaining_deadline) {
            Ok((11, p)) => spawn(handle_task(p))     // 任务处理
            Ok((6,  p)) => send_frame(7, pong)
            Ok((8,  _)) => break
            Ok((_,  _)) => ignore + 计数（超过 100 次异常帧则断开）
            Err(e)      => { log(peer_node_id, e); break }
        }
    }
```

- 移除 `quic.rs:250-251` 的 dummy packet。
- `ocn-node` 的主循环改为消费 `(peer_node_id, connection)` 并启动上述循环，不再匹配旧 `OCNMsgType`。
- 关闭时记录对端真实 ID（来自 R-1 校验后的 SAN），便于审计。

**验收：** 认证后连接存活 ≥ 30 s；Ping→Pong 往返 ≤ 1 s；对端断开时服务端日志含对端 node_id。

---

## 11. D5 —— Windows 与 Android 职责

| 能力 | Windows | Android | 依据 |
|---|---|---|---|
| 发起任务 | ✅ 协调者 | ❌ | 设备资源受限，不宜做长时协调 |
| 执行任务 | ✅（可选，本地调试） | ✅ 主要工作节点 | `ResourceManager` 面向移动端指标 |
| 资源准入 | 简单阈值 | 电池/温度/CPU/RAM 全量 | `resource_manager.rs:84-138` |
| 身份 | 持久化到项目/配置目录 | `getFilesDir()/ocn/identity` | `main.rs:41` 目前用 `%TEMP%`，需改 |
| 指标采集 | `sysinfo` 或 mock | `BatteryManager`、`/sys/class/thermal`、`/proc/stat`、`/proc/meminfo` | **当前完全没有实现**（`can_accept_task` 只在测试中被调用） |

**新增缺口（本轮发现）：** `SystemMetrics` 没有任何生产代码填充。B4 必须引入 `MetricsProvider` trait：

```rust
pub trait MetricsProvider: Send + Sync {
    fn snapshot(&self) -> Result<SystemMetrics, ResourceError>;
}
```

Host 与 Android 各一个实现；Android 侧通过 JNI 从 Java 读取（避免 Rust 直接依赖 Android API 版本）。测试用 `StaticMetricsProvider`。

---

## 12. D6 —— 幂等：审查结论

见 §8.8。三个必须落地的点：

1. `TaskExecutionState` 增加 `input_hash`；
2. `check_or_register` 命中且哈希不同 → 新错误 `IdempotencyConflict`；
3. 缓存实例必须是**进程级单例**并在服务启动时构造——当前全仓无任何生产代码构造它（`deduplication.rs` 仅在测试中出现）。

---

## 13. D7 —— 错误信令

| 场景 | 旧行为 | 新行为 |
|---|---|---|
| 认证失败 | 静默断开 | `AuthResult{success:0, error_code}` + 日志 + 客户端可区分原因 |
| 任务校验失败 | 不存在 | `TaskResult{Rejected, 1}` |
| 资源拒绝 | 不存在 | `TaskResult{Rejected, 2}` |
| 不支持类型 | 不存在 | `TaskResult{Rejected, 3}` |
| 执行失败 | 不存在 | `TaskResult{Failed, 4}` |
| 重复进行中 | 不存在 | `TaskResult{Rejected, 5}` |
| 超时 | 不存在 | `TaskResult{Failed, 6}` |
| 幂等冲突 | 不存在 | `TaskResult{Rejected, 8}` |
| 连接全部失败 | **进程返回 0**（`main.rs:303`） | 返回非 0 并打印结构化原因 |

`main.rs:303` 的 `Ok(())` 是 `F-16` 的直接来源：CI 或脚本无法通过退出码发现问题。

---

## 14. 对现有测试的精确影响

| 测试 | 文件:行 | 影响 | 处理 |
|---|---|---|---|
| `test_hkdf_hmac_auth_proof_calculation` | `ocn-core/src/tests.rs:44` | 写死 `"node-server"`，锁定 bug | 重写为 golden 向量 + 不同节点名对比 |
| `test_packet_size_limit_protection` | `:132` | 依赖 `OCNPacket::serialize` | 若移除 bincode 类型，改为帧编码超限测试 |
| `test_node_info_exchange` | `:139` | 空 | 实现真实断言（发送 5，解析字段） |
| `test_invalid_token_rejection` | `:145` | 空 | 实现：错误 token → 错误码 4 |
| `test_duplicate_challenge_rejection` | `:150` | 空 | 实现：重放 → 错误码 5 |
| `test_unauthenticated_ping_rejection` | `:155` | 空 | 实现：认证前 Ping → 断开 |
| `test_malformed_header_rejection` | `:160` | 空 | 实现：magic/version 错误 → Err |
| `test_batch_hash_execution` | `ocn-compute/src/runtime.rs:91` | 若选 BatchHash 方案 A，`result` 语义变化 | 改 `:107` 一条断言 |
| `test_integration_normal_batch_hash` | `ocn-compute/tests/integration_tests.rs:35` | 只断言 status/validate | 不受影响 |
| 其余 57 项 | — | 不触碰协议/运行时 | 应全部保持通过 |

**门禁：** B0 完成后，`cargo test --workspace` 必须为"测试总数 ≥ 66 且 0 失败"，且**不得**通过把新测试写成空函数来凑数。

---

## 15. 更新后的工作包

| 包 | 内容 | 依赖 | 验收 |
|---|---|---|---|
| **B0** | 认证身份绑定（§3、§4、§5、T-A1…T-A9） | 无 | 单元测试全绿；golden 向量固定 |
| **B0.5** | 双节点环回真实握手（T-A10/T-A11）+ 退出码 | B0 | 双方日志同时成功；无重连循环 |
| **B1** | 帧格式统一，`ConnectionHandle` 改为 `send_frame/recv_frame`，移除 bincode 线上路径 | B0.5 | 0/1/64 KiB/1.9 MB 往返；超限在分配前拒绝 |
| **B2** | 服务端认证后循环（§10） | B1 | 连接存活 ≥ 30 s；Ping/Pong ≤ 1 s |
| **B3** | DTO 编解码（11/12，§8.4-8.5） | B1 | 属性测试 + golden 向量 + 截断输入不 panic |
| **B4** | `TaskDispatcher` + 去重 + `MetricsProvider` + BatchHash 语义 | B3 | 4 种结果路径 + 幂等冲突 + 资源拒绝 |
| **B5** | 客户端 `submit_task`（超时/重试/哈希复核） | B4 | 1 B / 1 MB / 1.9 MB；错误哈希被本地拒绝 |
| **B6** | Android JNI（startWorker/executeTask/stopWorker） | B2,B4 | 三个符号导出；无 FATAL |
| **B7** | 设备端 E2E | B6 | 哈希一致；标注 `REAL_DEVICE` |

关键路径：`B0 → B0.5 → B1 → B2 → B3 → B4 → B5 → B6 → B7`。

---

## 16. 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| 硬切换认证格式 | 旧客户端全断 | 无部署对端；一次性切换 |
| SAN 解析处理畸形证书不当 | 崩溃或绕过 | 要求"恰好一个 DNS SAN"；畸形即拒；独立单元测试 |
| `ConnectionHandle` 破坏性变更波及 17 处调用 | 编译失败面大 | 先加新方法并保留默认实现，再逐个迁移，最后删除旧方法 |
| BatchHash 语义变更影响外部假设 | 中 | 明确写入协议文档；旧行为可由 `EchoHash`（保留号）提供 |
| 指标采集在 Android 上权限/兼容性问题 | 阻塞 B4 | 先用 `StaticMetricsProvider` 跑通链路，再接真实指标；不把指标作为 E2E 前置条件 |
| 1.9 MB 帧在移动网络下的实际表现未知 | 中 | 实测前不宣称性能；必要时降为 512 KB 分块 |

---

## 17. 本轮未做

- 未修改任何 Rust / Python / Android 代码。
- 未执行 Git 操作。
- 未运行 `cargo build` / `cargo test`（避免与备份校验争用 IO；基线沿用已验证结果）。
- 未对真实设备做任何操作。

**等待批准项见 `OCN_NEXT_EXECUTION_GATE.md`。**
