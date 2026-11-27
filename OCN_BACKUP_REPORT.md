# OCN 恢复点备份报告

**生成时间：** 2026-11-23 19:45（robocopy 结束时间），校验完成于 19:52
**类型：** 恢复点创建 + 完整性校验（**未执行任何 Git 操作**，未修改任何业务代码）

---

## 1. 结论摘要

| 项目 | 结果 | 证据等级 |
|---|---|---|
| 备份已创建 | 是，位于项目目录之外 | `VERIFIED` |
| 文件数一致 | 131 / 131 | `VERIFIED` |
| 字节数一致 | 4,740,867 / 4,740,867 | `VERIFIED` |
| SHA-256 全量比对 | 0 缺失、0 多余、0 不匹配 | `VERIFIED` |
| `.git` 内容保留 | 两个仓库各 5 个文件，`HEAD = ref: refs/heads/master` | `VERIFIED` |
| 大文件可读性抽检 | `model.safetensors` 头部读取成功（19464 B JSON 头） | `VERIFIED` |
| 备份位置冗余 | **同物理磁盘**（仅防误操作，不防磁盘故障） | `PARTIAL` |

---

## 2. 项目总量盘点

命令：

```powershell
$all = Get-ChildItem -Recurse -File -Force
($all | Measure-Object Length -Sum).Sum
```

结果：

| 指标 | 数值 |
|---|---|
| 项目文件总数 | 10,614 |
| 项目总字节 | 3,756,552,092 B（3.5 GB） |
| 其中 `target/` | 3,572.3 MB（构建缓存，占 95%） |
| 备份集文件数 | 131 |
| 备份集字节 | 4,740,867 B（4.52 MB） |

### 2.1 排除清单（可重新生成，不进入备份）

| 排除项 | 大小 | 理由 |
|---|---|---|
| `target/` | 3,572.3 MB | Cargo 构建缓存，`cargo build` 可重建 |
| `android/app/build/` | 约 1 MB | Gradle 输出，`assembleDebug` 可重建 |
| `android/build/` | 同上 | Gradle 根输出 |
| `android/.gradle/` | 同上 | Gradle 本地状态 |
| `ai/temp/` | 小 | HuggingFace 下载缓存标记 |
| `apk_extract/` | 空目录 | 解包残留 |
| `bidirectional_log.txt`、`install_logs.txt`、`launch_log.txt`、`launch_log_fresh.txt`、`logcat_output.txt`、`check.out`、`node1.log`、`node2.log` | 约 1.06 MB | 运行日志；`check.out` 含主机策略错误，无保留价值 |

**未排除且已备份（按要求）：** 全部 Rust 源码、Python 适配器、Android 源码与 Gradle 配置、`Cargo.toml`/`Cargo.lock`、模型 manifest 与配置、全部 `.md` 文档、两个 `.git` 目录、模型权重 `model.safetensors`。

---

## 3. 备份执行

### 3.1 目标位置

```
源：C:\Users\qixua\Desktop\OCN
备：C:\Users\qixua\OCN_BACKUPS\OCN_20261123_1930
```

位于项目目录之外，未使用任何符号链接或硬链接。

### 3.2 命令

```powershell
robocopy "C:\Users\qixua\Desktop\OCN" "C:\Users\qixua\OCN_BACKUPS\OCN_20261123_1930" `
  /E /COPY:DAT /DCOPY:DAT /R:1 /W:1 /NFL /NDL /NP `
  /XD "...\target" "...\android\app\build" "...\android\build" "...\android\.gradle" "...\ai\temp" "...\apk_extract" `
  /XF bidirectional_log.txt install_logs.txt launch_log.txt launch_log_fresh.txt logcat_output.txt check.out node1.log node2.log
```

- `/COPY:DAT` + `/DCOPY:DAT`：保留数据、属性、时间戳（不复制 ACL/所有者）。
- `/R:1 /W:1`：失败只重试 1 次，避免长时间挂起。
- 退出码 `1` = "已复制文件"，属**成功**；`0` 才表示"无变化"。

### 3.3 Robocopy 统计（原文）

```
Dirs :  65 total  59 copied   6 skipped   0 failed
Files: 139 total 131 copied   8 skipped   0 failed
Bytes: 5.58 MB total 4.52 MB copied 1.06 MB skipped
Ended: Monday, 23 November, 2026 7:45:12 PM
```

`0 failed` 表示无复制错误。

---

## 4. 完整性校验

### 4.1 方法

对源与备份分别枚举文件，使用**相同排除规则**，逐文件计算 SHA-256，按相对路径建立映射后三方比对：

- 源有、备无 → `MISSING_IN_BACKUP`
- 备有、源无 → `EXTRA_IN_BACKUP`
- 路径相同但大小或哈希不同 → `HASH_OR_SIZE_MISMATCH`

使用 `[System.IO.Path]` 相对路径（去根前缀），避免大小写/分隔符差异误判。

### 4.2 结果

```
SRC files=131 bytes=4740867
DST files=131 bytes=4740867
MISSING_IN_BACKUP=0  EXTRA_IN_BACKUP=0  HASH_OR_SIZE_MISMATCH=0
```

### 4.3 `.git` 校验

```
Test-Path "$dst\ocn-core\.git\HEAD"  -> True
Test-Path "$dst\ocn-node\.git\HEAD"  -> True
Get-Content "$dst\ocn-core\.git\HEAD" -> ref: refs/heads/master
backup .git files: core=5  node=5
```

与源侧 `.git` 文件数一致（各 5 个）。两个仓库均为 0 提交状态，故 `.git` 体积很小。

> 说明：本报告只验证 `.git` **文件内容被完整复制**，不代表仓库已有历史。当前两个仓库仍为 0 提交，备份不改变这一点。

### 4.4 大文件抽检

`model.safetensors`（3,240,592 B）在备份中以二进制方式打开并读取前 16 字节：

```
read bytes=16
head = 08 4c 00 00 00 00 00 00 7b 22 5f 5f 6d 65 74 61
```

解析：前 8 字节小端整数 `0x4C08 = 19464`，即 safetensors JSON 头长度；随后为 `{"__meta` 。文件未损坏，与源文件哈希比对亦一致。

---

## 5. 恢复方法（预案，未执行）

```powershell
# 1) 校验备份完整性（可复用本报告 §4.1 脚本）
# 2) 将备份复制回目标目录
robocopy "C:\Users\qixua\OCN_BACKUPS\OCN_20261123_1930" "C:\Users\qixua\Desktop\OCN" /E /COPY:DAT /DCOPY:DAT
# 3) 重建可再生产物
cargo build --workspace
cd android; .\gradlew.bat assembleDebug
```

恢复不包含 `target/`、Gradle 构建目录与日志，需要重新生成；这不影响源码与配置的完整性。

---

## 6. 残余风险（必须知晓）

| # | 风险 | 影响 | 等级 | 缓解 |
|---|---|---|---|---|
| R-1 | 备份与源在同一块物理磁盘 | 磁盘故障会同时丢失两者 | **高** | 追加一份到外部盘/移动硬盘或云端（需另行授权） |
| R-2 | 未加密 | 备份含私钥 `*.key.der` 与自签证书 | 中 | 存放于受控目录；如需外传须先加密 |
| R-3 | 节点身份不在项目内 | `ocn-node/src/main.rs:41` 使用 `std::env::temp_dir()/ocn_identities`；该目录**不在**备份范围 | 中 | 单独备份 `%TEMP%\ocn_identities`，或将身份目录改为项目内/应用私有目录（后续设计项） |
| R-4 | 未保留 ACL/所有者 | 跨机恢复后权限可能变化 | 低 | 恢复后按需重设 |
| R-5 | 两份 0 提交仓库无历史可回滚 | 备份是**唯一**恢复点 | **高** | 这正是版本管理方案 A 要解决的问题 |
| R-6 | 备份时项目处于静止状态 | 若有外部进程正在写文件，可能捕获到中间态 | 低 | 本次备份期间无构建/测试运行 |

---

## 7. 合规声明

- 未执行 `git init` / `add` / `commit` / `reset` / `clean` / `checkout`。
- 未修改任何 Rust / Python / Android / 配置文件。
- 未删除、未移动任何文件；未覆盖任何既有报告。
- 备份写入目标为项目目录之外的新目录，原项目只读访问。

---

## 8. 元数据

| 项 | 值 |
|---|---|
| 备份 ID | `OCN_20261123_1930` |
| 文件数 | 131 |
| 字节数 | 4,740,867 |
| 源哈希基准 | 逐文件 SHA-256（全部一致） |
| 模型权重 SHA-256 | `a706726a7eb01bbcb42994b7dcb3c06ea9557898dbae8d480eb04fe8ccb89710` |
