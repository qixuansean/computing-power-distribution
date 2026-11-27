# OCN 下一阶段执行门禁

**生成时间：** 2026-11-23 19:52
**状态：** **已暂停，等待审核。** 本轮只做了备份与报告，未改一行业务代码，未执行任何 Git 操作。
**恢复点：** `C:\Users\qixua\OCN_BACKUPS\OCN_20261123_1930`（131 文件 / 4,740,867 B / SHA-256 全量一致）

---

## 1. 本轮完成情况

| # | 任务 | 状态 | 交付物 |
|---|---|---|---|
| 1 | 项目总量盘点 | ✅ | `OCN_BACKUP_REPORT.md` §2（10,614 文件 / 3.5 GB） |
| 2 | 项目外完整备份（排除 target/日志/构建缓存） | ✅ | 同上 §3 |
| 3 | 保留 Rust/Python/Android/配置/manifest/文档/`.git` | ✅ | 同上 §2.1、§4.3 |
| 4 | 文件清单 + SHA-256 双向校验 | ✅ | 131/131，0 缺失 0 多余 0 不符 |
| 5 | 备份可读性确认 | ✅ | 模型文件二进制头读取成功 |
| 6 | 未执行 Git 初始化/提交/重置/迁移 | ✅ | 根目录仍非仓库；仓库数仍为 2 |
| 7 | D1–D7 逐项设计复审 | ✅ | `OCN_PHASE31_DESIGN_REVIEW.md` |
| 8 | QUIC 认证专项复审（身份分层 + 测试先行） | ✅ | 同上 §1–§6 |
| 9 | 协议格式统一设计（长度/超时/错误/断线/幂等/哈希） | ✅ | 同上 §8 |
| 10 | 启发式预测定性标记 | ✅（文档层） | `OCN_PHASE32_MODEL_REVIEW.md` §2 |
| 11 | 真实 TTM 前向可行性检查 | ✅ | 同上 §3 |
| 12 | 真实推理独立实施计划 | ✅ | 同上 §5 |
| 13 | Policy / 回退问题复核（不改代码） | ✅ | 同上 §6 |

---

## 2. 三个必须先知晓的更正

### 2.1 "66 项测试" 中有 5 项是空的

`ocn-core/src/tests.rs:139-162` 的 `test_node_info_exchange`、`test_invalid_token_rejection`、`test_duplicate_challenge_rejection`、`test_unauthenticated_ping_rejection`、`test_malformed_header_rejection` 函数体内只有注释，没有任何断言。

**有效断言的测试数 = 61。** 这 5 项恰好覆盖本次要修的协议行为，属于"名字承诺的覆盖并不存在"。

### 2.2 认证测试锁定了 bug

`tests.rs:44-63` 把 `"node-server"` 这个错误常量写死进断言，还断言"不同 nonce 产生不同 proof"。它证明的是函数确定性，而不是协议正确性。

### 2.3 `model_loader.py` 在当前环境必然失败

`transformers 5.24.0` 不提供 `AutoModelForTimeSeriesForecasting`，`tinytimemixer` 模块也不存在，`tsfm` 未安装。`model_loader.py:37` 的 import 必然抛 `ImportError`，随后被包装成 `{"status":"fallback","reason":"import_error"}` 正常返回。该文件第 39 行自己写着"占位符"。

---

## 3. 待批准清单（请逐项确认）

### A. 备份

| ID | 事项 | 建议 | 需要您回答 |
|---|---|---|---|
| A-1 | 接受 `OCN_BACKUP_REPORT.md` 中的备份作为恢复点 | 接受 | 是 / 否 |
| A-2 | 备份与源在同一物理磁盘（R-1 高风险） | 追加一份到外置盘或云端 | 需要 / 暂不需要 |

### B. Phase 3.1 设计

| ID | 事项 | 建议 | 需要您回答 |
|---|---|---|---|
| B-1 | 身份三层定义（证书身份 / 节点身份 / 预期对端）与规则 R-1…R-6 | 接受 | 是 / 否 |
| B-2 | `AuthChallenge` v2 负载（`payload_version` + `server_node_id` + `server_cert_fingerprint` + nonce + expiry） | 接受，硬切换，不做双版本 | 是 / 否 |
| B-3 | 认证错误码 1–7（区分"token 错""绑定错""钉扎错"） | 接受 | 是 / 否 |
| B-4 | 先写测试 T-A1…T-A11，确认红灯后再改实现 | 接受 | 是 / 否 |
| B-5 | 统一 `FixedHeader` 为唯一线格式，**移除** bincode 线上路径（`ConnectionHandle` 由 `send/recv` 改为 `send_frame/recv_frame`） | 接受 | 是 / 否 |
| B-6 | 长度上限：`MAX_FRAME_PAYLOAD = 2 MiB`、`CONTROL_MAX = 64 KiB`、`TASK_MAX_PAYLOAD = 1,900,000 B` | 接受 | 是 / 否 |
| B-7 | 超时表（握手 10 s / 空闲 60 s / 任务取 `min(timeout_ms, deadline)`） | 接受 | 是 / 否 |
| B-8 | BatchHash 语义：`result = SHA-256(payload)`（方案 A），`result_hash = SHA-256(result)` | 方案 A | A / B / 其他 |
| B-9 | 幂等：键 + `input_hash` 绑定，冲突返回错误码 8，不执行 | 接受 | 是 / 否 |
| B-10 | 工作包顺序 `B0 → B0.5 → B1 → B2 → B3 → B4 → B5 → B6 → B7` | 接受 | 是 / 否 |
| B-11 | Android = 工作节点/服务端；Windows = 协调者（可兼本地执行） | 接受 | 是 / 否 |
| B-12 | 新增 `MetricsProvider` trait（Host/Android/Static 三实现） | 接受 | 是 / 否 |

### C. Phase 3.2

| ID | 事项 | 建议 | 需要您回答 |
|---|---|---|---|
| C-1 | 把当前预测重命名为 `HeuristicTrendProvider` / `inference_mode=heuristic_trend_v1` / `data_source=SIMULATED` | 立即执行（0.5 天） | 是 / 否 |
| C-2 | 真实前向：自研 `ttm_forward.py`（仅 torch+numpy），`tsfm` 只用于离线对照 | 接受 | 是 / 否 |
| C-3 | 输出映射改为"窗口内峰值 + p95 内存 + 标定置信度"，并做 `[0,1]` 裁剪 | 接受 | 是 / 否 |
| C-4 | T-5（真实输入历史管线）完成前，禁止发布任何准确率结论 | 接受 | 是 / 否 |
| C-5 | `PolicyEngine` 有效上限 = `min(engine, budget)`（行为变更） | 接受，公告后实施 | 是 / 否 |
| C-6 | 超时回退表述改为"检测后亚毫秒切换，检测延迟受 `timeout_ms` 约束" | 接受 | 是 / 否 |

### D. 版本管理（沿用上一轮 10 项）

| ID | 事项 | 状态 |
|---|---|---|
| D-1…D-10 | 见 `OCN_SOURCE_CONTROL_PLAN.md` §4 | 仍未批准，未执行 |

---

## 4. 批准后允许的第一步（严格顺序）

**只有 B-1 ~ B-5 全部为"是"时，才允许开始 B0。**

### 步骤 0：只读基线（0.5 小时）

```powershell
cargo test --workspace --no-fail-fast
```
- 记录真实测试数（预期 66，其中 5 项空）。
- 不改代码；若失败，先停止并报告，不进入步骤 1。

### 步骤 1：B0 红灯测试（先写测试）

新建 `ocn-core/src/tests_auth_binding.rs`（或在 `tests.rs` 内追加模块）：

- T-A1 golden transcript 向量
- T-A2 两侧 proof 在真实节点名下相等
- T-A3 挑战指纹不符 → 客户端中止
- T-A4 挑战节点名与 SAN 不符 → 客户端中止
- T-A5 Hello/AuthResponse 节点名不一致 → 服务端拒绝
- T-A6 客户端证书 SAN 不符 → 服务端拒绝
- T-A7 钉扎不匹配 → 明确错误
- T-A9 错误 token → 错误码 4

运行 → **必须全部失败**（或部分因函数不存在而编译失败，视为红灯）。

### 步骤 2：B0 实现

仅限这些文件：
- `ocn-core/src/identity.rs`（新增 SAN 解析与指纹函数；`verify_client_cert` 加 SAN 校验）
- `ocn-core/src/quic.rs`（挑战/证明/结果的载荷与取值来源）
- `ocn-core/src/messages.rs`（`AuthChallengePayload` 字段）
- `ocn-core/src/tests.rs`（重写 `test_hkdf_hmac_auth_proof_calculation`）

**禁止**在本步骤触碰 `ocn-compute`、`ocn-mind`、Android。

### 步骤 3：B0.5 环回验证

```powershell
target\debug\ocn-node.exe --node-id node-A --listen 127.0.0.1:9310
target\debug\ocn-node.exe --node-id node-B --listen 127.0.0.1:9311 --peer 127.0.0.1:9310
```
通过条件：双方认证成功、连接保持 ≥ 5 s、无重连循环、客户端退出码反映真实结果。

### 步骤 4：暂停并报告

提交 B0/B0.5 结果（含测试数量变化、日志片段），**再次等待批准**后才进入 B1。

---

## 5. 硬性停止条件

出现任一条立即停止并报告，不得继续：

1. `cargo test` 出现非预期失败且两次修复无效。
2. 编译时间或内存使用异常增长（例如 `rustc` 占用超过物理内存 70%）。
3. 备份目录与源出现不一致（说明源在未被知晓的情况下被修改）。
4. 出现任何未预期的 Git 状态变化（根目录突然出现 `.git`）。
5. 磁盘剩余空间 < 20 GB。
6. 需要修改 `Cargo.toml` 依赖版本才能继续（说明方案有隐藏依赖）。
7. 任何设备操作出现 `FATAL EXCEPTION` 或系统级 ANR。

---

## 6. 回滚程序

```powershell
# 1) 停止所有由本次会话启动的进程（先确认 PID 来源）
# 2) 从恢复点还原（保留备份，不覆盖备份）
robocopy "C:\Users\qixua\OCN_BACKUPS\OCN_20261123_1930" "C:\Users\qixua\Desktop\OCN" /E /COPY:DAT /DCOPY:DAT
# 3) 重建
cargo build --workspace
```

注意：还原不会恢复 `target/` 与 Gradle 构建目录，需要重新构建；也不恢复 `%TEMP%\ocn_identities`（节点身份），如需保留请单独备份该目录。

---

## 7. 当前状态快照

| 项 | 值 |
|---|---|
| 时间 | 2026-11-23 19:52 |
| 根目录是否为 Git 仓库 | 否（`Test-Path .git` = False） |
| `.git` 目录数 | 2（`ocn-core`、`ocn-node`） |
| `ocn-node` 残留进程 | 0 |
| 备份文件数 | 131 |
| 本会话新建文件 | 4 份报告 + 1 个备份目录 |
| 本会话修改的业务代码 | **0 个文件** |
| 本会话 Git 操作 | **0 次** |

---

## 8. 合规声明

- 未执行 `git init` / `add` / `commit` / `reset` / `clean` / `checkout` / `remote` / `gc`。
- 未修改任何 Rust / Python / Android 源文件或配置。
- 未删除、未移动任何文件；未覆盖任何既有报告。
- 未运行 `cargo build` / `cargo test`（避免与备份校验争用 IO；基线沿用已验证结果）。
- 未对真实 Android 设备执行任何操作。
- 备份写入项目目录之外的新目录。

---

## 9. 一句话总结

恢复点已建立且逐文件校验通过；设计复审发现了 **5 个空测试、1 个锁定 bug 的测试、1 条必然失败的加载器 import、以及 1 个会系统性低估资源峰值的输出映射**。业务代码一行未动，等待您对 §3 的 A/B/C 三组逐项批准。
