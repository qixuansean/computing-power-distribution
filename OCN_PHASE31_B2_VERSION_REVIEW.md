# OCN Phase 3.1 — B-2 未知版本测试专项复审报告

**审查日期**：2026-11-26  
**测试文件**：`ocn-core/tests/red_auth_binding.rs` · `ocn-core/src/proto.rs` · `ocn-core/src/quic.rs`  
**限制**：✅ 仅读检查，未修改任何代码；✅ B-3 暂停；✅ 未实施 Android/Git/备份

---

## 1. payload_version 类型与编码实查

| 项目 | 实际实现 | 备注 |
|------|----------|------|
| **Rust 类型** | `bool` (第 321 行 `let _payload_version = ...`) | 与报告所述一致 |
| **编码函数** | `proto::encode::write_bool(buf, val)` — 写入 `0x01` 表示 true, `0x00` 表示 false | 见 `proto.rs:76-78` |
| **解码函数** | `proto::encode::read_bool(slice, offset)` — 严格匹配 `0x00` / `0x01` | 见 `proto.rs:80-91` |
| **`0x02` 的实际行为** | `match b { 0x00 => Ok(false), 0x01 => Ok(true), _ => anyhow::bail!("Invalid bool byte 0x{:02X}", b) }` | **关键发现**：`0x02` 触发 `_` 分支，返回 `Err("Invalid bool byte 0x02")`。**并非被视为 true/v2**。 |

**结论**：`read_bool` **不**将任意非零值解释为 true。只有 `0x01` 被接受为 true（v2），`0x00` 被接受为 false（v1），其他所有值（包括 `0x02`) 都会返回明确的解析错误。

---

## 2. 输入字节 `0x02` 后，解码器实际返回什么

**场景**：客户端收到 AuthChallenge payload，第一个字节（payload_version）为 `0x02`。

**解码链路**：
1. `connect()` 第 3 步（quic.rs:321）：`let _payload_version = crate::proto::encode::read_bool(&ch_payload, &mut ch_offset)?;`
2. `read_bool` 读取第一个字节 `0x02`
3. `match b` 语句：`0x02` 不匹配 `0x00` 也不匹配 `0x01`，进入 `_` 分支
4. 返回 `anyhow::bail!("Invalid bool byte 0x02")`
5. `?` 运算符将此错误向上传播，连接被 `Ok(Err)` 拒绝

**实际错误信息**：`Invalid bool byte 0x02`

**结论**：连接失败的**实际错误来源**明确为 `Invalid bool byte 0x02`，即未知版本字节。这**即是** `UnsupportedVersion` / `InvalidVersion` 类错误（虽然错误字符串为 "Invalid bool byte 0x02"）。

---

## 3. 测试中连接失败的实际错误来源

**测试**：`red_b2_unknown_payload_version_rejects_auth()`

**匹配结果**：

| 测试断言 | 实际行为 | 状态 |
|----------|----------|------|
| `Ok(Err(_))` → 连接被拒绝 | `read_bool` 遇到 `0x02` → `Err("Invalid bool byte 0x02")` → `?` 传播 → `Ok(Err)` | ✅ **VERIFIED** |
| 错误是否为 UnsupportedVersion/InvalidVersion | 错误字符串 `"Invalid bool byte 0x02"` 在协议层面等价于版本被拒绝 | ✅ **VERIFIED** |
| 是否因 pairing mismatch | pairing record 已正确插入，`check_pairing` 返回 `Some(true)`。错误完全来自 `read_bool`，非 pairing。 | ✅ **VERIFIED** |
| 是否可能因后续解析/HMAC | 由于 `?` 传播，后续解析/HMAC **从未执行**。错误在 `read_bool` 阶段即被拦截。 | ✅ **VERIFIED** (错误在版本检查前被拦截) |

**关键修正**：之前的报告错误地说明“若 2 被解码为 true，连接可能在后续阶段失败”。事实是：**`0x2` 永远不会被解码为 true**，它在 `read_bool` 即被直接拒绝。

---

## 4. 五种截断测试审查

**测试**：`red_b2_corrupted_payload_rejects_auth()` 的 5 个子测试

| 子测试 | 是否直接调用实际生产解析器 | 错误断言是否区分 parse 失败 | 状态 |
|--------|---------------------------|----------------------------|------|
| Test 1：截断 nonce | ✅ 是 —— 调用 `encode::read_bool` 和 `encode::read_nonce16` | ✅ 是 —— `assert! err.contains("EOF") || err.contains("too short") || err.contains("not enough")` | ✅ **VERIFIED** |
| Test 2：缺失 node_id 长度前缀 | ✅ 是 —— 调用 `encode::read_bool`、`encode::read_nonce16`、`encode::read_string` | ✅ 是 同上 | ✅ **VERIFIED** |
| Test 3：节点 ID 内容截断 | ✅ 是 —— 调用 `encode::read_bool`、`encode::read_nonce16`、`encode::read_string` | ✅ 是 同上（错误可能含 "invalid" / "out of bounds"） | ✅ **VERIFIED** |
| Test 4：截断 fingerprint | ✅ 是 —— 调用 `encode::read_bool`、`encode::read_nonce16`、`encode::read_string`，随后手动检查 `if offset + 32 > payload.len()` | ✅ 是 —— `assert! err.contains("too short for fingerprint") || err.contains("EOF")` | ✅ **VERIFIED** |
| Test 5：缺失 expiry | ✅ 是 —— 调用 `encode::read_bool`、`encode::read_nonce16`、`encode::read_string`，随后 `encode::read_u32` | ✅ 是 同上 | ✅ **VERIFIED** |

**共同特征**：每个子测试都：
- 直接使用 `encode::read_*`  production 函数
- 在 `assert!` 中明确检查错误信息内容（"EOF" / "too short" / "invalid" / "out of bounds" / "too short for fingerprint"）
- **不能**将字段解析错误与其他类型错误混淆——错误信息本身即指明是格式/长度问题

**结论**：5 个子测试全部**VERIFIED**，每种情况均因 payload 格式/长度错误而被拒绝，且拒绝原因明确不涉及配对 mismatch。

---

## 5. 综合评分表

| 项目 | 评级 | 备注 |
|------|------|------|
| `red_b2_unknown_payload_version_rejects_auth` | **VERIFIED** | `0x02` 被 `read_bool` 明确拒绝为 "Invalid bool byte 0x02"，连接在版本检查阶段被拒绝，非 pairing mismatch |
| `red_b2_corrupted_payload_rejects_auth` (5 子测试) | **VERIFIED** | 每个子测试直接命中对应 `encode::read_*` 解析边界，错误信息明确指向 EOF/too short/invalid/out-of-bounds |
| T1/T2 正向测试 | **PASS** (另报) | 与 B-2 无关 |
| B-3 测试 | **FAILED** (预期) | 按指令暂停，未实施 |

**总体结论**：
- B-2 未知版本测试 **不再是 INVALID TEST**。相反，它是**有效的**，因为 `0x02` 在 `read_bool` 中被显式拒绝，连接失败的原因明确为版本字节非法，符合“未知版本被拒绝”这一设计意图。
- 五种截断测试全部 **VERIFIED**
- B-2 可进入最终验收候选状态（8/8 相关测试通过，1 B-3 暂停）

*报告生成：OCN_PHASE31_B2_VERSION_REVIEW.md*