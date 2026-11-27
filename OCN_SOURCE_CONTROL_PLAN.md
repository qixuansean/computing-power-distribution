# OCN 源码版本管理方案（Source Control Plan）

**日期：** 2026-11-23  
**阶段：** Phase 3.1 前置整理（仅检查与规划）  
**状态：** 本文件不包含任何已执行的写操作。**未运行** `git init` / `git add` / `git commit` / `git rm`，**未删除**、**未移动**、**未重命名**任何文件或目录，**未创建**任何备份。下文所有"建议操作"均待确认。

---

## 1. 现状实测（命令输出摘要）

### 1.1 Git 仓库清单

递归扫描 `C:\Users\qixua\Desktop\OCN` 下的 `.git` 目录，共发现 **2 个**仓库，根目录**不是**仓库：

| 仓库路径 | 分支 | 提交数 | 远端 | 工作区状态 | 未跟踪内容 |
|---|---|---|---|---|---|
| `C:\Users\qixua\Desktop\OCN\ocn-core` | `master` | **0**（`does not have any commits yet`） | 无 | 干净，仅未跟踪 | `.gitignore`、`Cargo.toml`、`src/` |
| `C:\Users\qixua\Desktop\OCN\ocn-node` | `master` | **0** | 无 | 干净，仅未跟踪 | `.gitignore`、`Cargo.toml`、`src/` |
| `C:\Users\qixua\Desktop\OCN`（根） | — | — | — | `fatal: not a git repository`（Exit 1） | 全部内容均无版本管理 |

补充事实：

- 无 `.gitmodules` 文件；**不存在子模块**。
- `git worktree list` 两个仓库均只有主工作树，无附加 worktree。
- 两个仓库的 `core.ignorecase=true`、`filemode=false`（Windows 默认）。
- 无全局 `core.excludesfile`；`.git/info/exclude` 为空。
- 两个仓库的 `.gitignore` 内容均为 `/target`，但目标目录实际在**根目录**（`C:\...\OCN\target`），因此该规则对各自仓库**不生效**（且各自目录下并无 `target`）。

### 1.2 目录与文件盘点

| 类别 | 内容 | 版本状态 |
|---|---|---|
| 已纳入版本管理 | **无**（0 提交、0 跟踪文件） | 不存在任何恢复点 |
| 无版本管理的业务代码 | `ocn-compute/`（7 源文件 + 1 测试 + Cargo.toml）、`ocn-mind/`（11 个 `.rs` + Cargo.toml） | 未跟踪 |
| 无版本管理的构建入口 | 根 `Cargo.toml`、`Cargo.lock` | 未跟踪 |
| 无版本管理的 AI 层 | `ai/adapters/granite_ttm_adapter/`（4 `.py` + 1 `.rs`）、`ai/scripts/`（2 `.py`）、`ai/manifests/model_checksums.json`、`ai/models/granite-ttm/model/model.safetensors`（3.1 MB） | 未跟踪 |
| 无版本管理的 Android 层 | `android/app/src/main/java/...`（2 `.java`）、3 个 `.gradle`、`AndroidManifest.xml`、`gradlew(.bat)`、`gradle-wrapper.jar`、`jniLibs/arm64-v8a/libocn_core.so` | 未跟踪 |
| 无版本管理的文档 | 根目录 **48 个 `.md`**（含本计划与统一状态报告） | 未跟踪 |
| 无版本管理的脚本/日志 | 3 `.ps1`、5 `.bat`、6 `.txt`/`.log`/`.out` | 未跟踪 |

### 1.3 重复目录 / 空目录 / 特殊文件

**按内容哈希比对：`*.rs` 与 `*.py` 均无重复文件**（SHA-256 分组结果为空）。

但存在以下**空目录**（Git 无法跟踪空目录，提交后将消失）：

| 路径 | 推测用途 | 是否 workspace 成员 |
|---|---|---|
| `ocn-mind\interfaces\` | 与 `ocn-mind\src\interfaces.rs` 重名，疑似早期脚手架 | 否（`src/` 才是真实位置） |
| `ocn-mind\model-runtime\` | 空 | 否 |
| `ocn-mind\providers\` | 与 `ocn-mind\src\providers\` 重名 | 否 |
| `ocn-mind\scheduler-bridge\` | 与 `ocn-mind\src\scheduler_bridge.rs` 重名 | 否 |
| `ocn-gaming-assist\src\` | 空 crate 骨架 | 否（不在 members 列表） |
| `ocn-resource-overflow\src\` | 空 crate 骨架 | 否 |
| `apk_extract\` | APK 解包残留 | 否 |

> 这些目录**不是**重复代码（无文件），但会造成"目录结构重复"的错觉。是否保留需确认；本计划**不建议删除**。

**大体积 / 二进制 / 生成物：**

| 文件/目录 | 大小 | 性质 | 处理建议 |
|---|---|---|---|
| `target\` | **3,572.3 MB** | Cargo 构建产物 | 必须忽略 |
| `android\app\build\`、`android\build\`、`android\.gradle\` | 数 MB | Gradle 构建产物 | 必须忽略 |
| `android\app\src\main\jniLibs\arm64-v8a\libocn_core.so` | 二进制 | 由 `build_arm64.ps1` 生成的产物 | 建议忽略，记录构建步骤 |
| `android\gradle\wrapper\gradle-wrapper.jar` | 二进制 | Gradle Wrapper 的标准随仓库文件 | 建议跟踪（或 LFS） |
| `ai\models\granite-ttm\model\model.safetensors` | 3.1 MB | 真实模型权重 | 建议 Git LFS；或忽略 + 保留 `model_checksums.json` |
| `install_logs.txt` / `logcat_output.txt` / `launch_log_fresh.txt` / `bidirectional_log.txt` / `launch_log.txt` / `check.out` | 0.5 MB ~ 2.5 KB | 运行日志 | 忽略 |
| `ai\temp\.cache\huggingface\*` | 极小 | 下载缓存标记 | 忽略 |

### 1.4 关键文件是否被忽略

- 根目录**没有** `.gitignore`。
- 两个嵌套仓库的 `.gitignore` 仅含 `/target`，**未忽略任何关键文件**。
- `git status --ignored --short` 在两个仓库中均只显示 `??`（无 `!!`），即当前**没有任何关键文件处于被忽略状态**。
- 因此当前风险不是"被忽略"，而是"**全部未纳入版本管理**"。

---

## 2. 风险评估

| # | 风险 | 等级 | 说明 |
|---|---|---|---|
| R1 | **整个项目无任何提交，磁盘故障即全部丢失** | **严重** | 两个嵌套仓库 0 提交，根目录无仓库；`ocn-compute`、`ocn-mind`、`ai`、`android`、48 份文档均无恢复点。 |
| R2 | 在根目录直接 `git init` + `git add .` 会把 `ocn-core`、`ocn-node` 记录为 **gitlink（无内容）** | 高 | 克隆后这两个目录为空，且会与现有独立 `.git` 冲突；这是最常见的嵌套仓库事故。 |
| R3 | `target\` 3.57 GB 被误提交 | 高 | 一旦进入历史，无法轻易清除，仓库体积不可逆膨胀。 |
| R4 | 日志（约 1.1 MB）、`.so`、模型权重进入历史 | 中 | 可清理但需 `filter-repo` 重写历史，代价高。 |
| R5 | 两个仓库同名分支 `master`、无远端、无提交，后续可能被误合并 | 中 | 各自独立演化，合并语义不明确。 |
| R6 | 空目录提交后消失，结构意图丢失 | 低 | 需要 `.gitkeep` 才能保留。 |
| R7 | Windows `ignorecase=true` 导致仅大小写不同的文件重命名不被识别 | 低 | 牵连文件系统行为，暂不处理。 |
| R8 | `libocn_core.so` 与 `ocn-core` 源码不同步 | 中 | 二进制入库会造成"源码改了但 .so 没变"的假象。 |

---

## 3. 推荐方案

### 方案 A（推荐）：根目录 monorepo + 两个嵌入式独立仓库原样保留

**核心思想：** 根仓库跟踪"除 `ocn-core/`、`ocn-node/` 之外的一切"；这两个目录继续作为独立仓库存在，互不干扰。

**根目录 `.gitignore` 建议内容：**

```gitignore
# --- 嵌套独立仓库：整体排除，避免生成 gitlink ---
/ocn-core/
/ocn-node/

# --- 构建产物 ---
/target/
/android/build/
/android/app/build/
/android/.gradle/
/local.properties

# --- 二进制生成物（由 build_arm64.ps1 重新生成） ---
/android/app/src/main/jniLibs/**/libocn_core.so

# --- 运行日志与缓存 ---
*.log
*.out
install_logs.txt
logcat_output.txt
launch_log*.txt
bidirectional_log.txt
/ai/temp/

# --- 编辑器/系统 ---
.DS_Store
Thumbs.db
.idea/
.vscode/
```

**需要跟踪的内容：**

```
/Cargo.toml
/Cargo.lock                 # 保留以保证可复现构建
/ocn-compute/
/ocn-mind/
/ai/                        # 不含 temp/ 与 *.safetensors（见 LFS 决策）
/android/                   # 不含 build/、.gradle/、*.so
/*.md                       # 48 份文档
/*.ps1, *.bat
```

**各仓库关系：**

| 仓库 | 跟踪范围 | 是否含 `.git` | 与根仓库关系 |
|---|---|---|---|
| 根 `C:\...\OCN` | 除 `ocn-core/`、`ocn-node/` 外全部 | 新建（待确认） | 主仓库 |
| `ocn-core` | 自身 `.gitignore`、`Cargo.toml`、`src/` | 保留 | 被根仓库忽略，独立提交 |
| `ocn-node` | 自身 `.gitignore`、`Cargo.toml`、`src/` | 保留 | 被根仓库忽略，独立提交 |

**优点：** 零删除、零重命名；`ocn-core` 独立历史完整保留；根仓库不会产生 gitlink。  
**缺点：** 根 `Cargo.toml` 的 workspace `members` 引用了两个不在根仓库内的路径——可正常工作，但需要文档说明"克隆根仓库后需单独获取这两个 crate"。  
**副作用：** 若日后希望"一次克隆全部"，需要再评估方案 B。

### 方案 B（备选）：单一根仓库全量纳入

把两个 `.git` 目录**重命名**为 `.git.standalone-20261123`（保留而非删除），然后根仓库全量跟踪。  
**仅在明确接受"两个 crate 不再独立"时采用。** 本方案不涉及 `git submodule`，因此不违反"不得转为子模块"的约束，但会改变其独立仓库属性，风险高于方案 A。

### 方案 C（不推荐）：`git submodule`

将 `ocn-core`、`ocn-node` 注册为子模块。**与用户明确限制冲突，已排除。**

---

## 4. 需要您确认的操作（均未执行）

请逐项回复"同意 / 不同意 / 修改"，我再执行。

1. **根目录 `git init`**（在 `C:\Users\qixua\Desktop\OCN` 新建仓库，分支名建议 `main` 或沿用 `master`）。
2. **创建根目录 `.gitignore`**，内容按第 3 节方案 A（含 `/ocn-core/`、`/ocn-node/` 排除）。
3. **根仓库首次提交**（将提交 `ocn-compute/`、`ocn-mind/`、`ai/`（按第 5 项决策）、`android/`（按第 6 项决策）、根 `Cargo.toml`/`Cargo.lock`、全部文档与脚本）。
4. **`ocn-core` 首次提交**（提交其 `.gitignore`、`Cargo.toml`、`src/`），保持独立历史。
5. **`ocn-node` 首次提交**（同上）。
6. **模型权重策略**：三选一
   - (a) 使用 Git LFS 跟踪 `ai/models/granite-ttm/model/model.safetensors`（3.1 MB）；
   - (b) 忽略权重，仅保留 `ai/manifests/model_checksums.json` + `download_model.py`；
   - (c) 直接纳入普通 Git（3.1 MB 可接受）。
7. **`libocn_core.so` 策略**：忽略并由 `build_arm64.ps1` 重建（推荐），还是纳入 LFS。
8. **空目录保留**：是否为 `ocn-mind/{interfaces,model-runtime,providers,scheduler-bridge}`、`ocn-gaming-assist/src`、`ocn-resource-overflow/src` 添加 `.gitkeep`，还是确认其为废弃脚手架（**不建议删除**）。
9. **备份方式**：建议在执行任何 `git init` 前，先将整个 `OCN` 目录（排除 `target/`）打包到外部磁盘。备份位置请指定；我不会自行创建或声称已创建备份。
10. **远端仓库**：是否需要推送到私有远端（GitHub/Gitee/自建 Git）。当前两个仓库均**无 remote**。

---

## 5. 已执行的核对（仅读取）

- 递归查找 `.git`：2 个（`ocn-core`、`ocn-node`）。
- 两仓库 `branch / remote / status / log / worktree` 全部实查。
- `git status --ignored`：无任何被忽略项。
- `.gitmodules` 检索：0。
- `.rs` / `.py` SHA-256 去重：0 组重复。
- `OCN_UNIFIED_STATUS.md` 证据行号复核：发现并修正 1 处偏差（`ocn-node/src/main.rs:113-178` → `112-179`），其余路径与行号一致；已在报告中记录差异。

**本计划未修改任何 Rust / Android / Python 业务代码，未删除、未移动、未提交、未备份。**
