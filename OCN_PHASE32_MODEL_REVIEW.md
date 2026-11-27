# OCN Phase 3.2 模型与策略复审

**日期：** 2026-11-23
**状态：** 复审 + 计划。**未修改任何业务代码。**
**范围：** ① 线性趋势预测的定性标记；② Granite TTM 真实前向计算的可行性；③ 真实推理独立实施计划；④ Policy/TTM 三个既有缺陷的复核。

---

## 1. 结论先行

| 问题 | 结论 | 证据等级 |
|---|---|---|
| 当前"TTM 推理"是什么 | **线性趋势 + 正弦扰动的启发式模拟**，不是神经网络前向计算 | `VERIFIED`（源码注释 + 代码路径） |
| 权重是否真实 | 真实 TTM-R2 检查点，134 个张量，805k 参数，SHA-256 与 manifest 一致 | `VERIFIED` |
| 是否已具备真实前向计算 | **不具备**：缺前向实现、缺真实输入历史、缺正确的输出映射 | `FAILED` |
| 依赖是否就绪 | torch 2.15.0+cpu 可用；`transformers 5.24.0` **不含** TTM 类；`tsfm` **未安装** | `VERIFIED` |
| 是否可自行实现前向 | **可以**，架构已从权重形状完整还原，规模很小 | `VERIFIED`（形状解析） |
| Policy 三个缺陷 | 2 个逻辑缺陷（死字段、无回退）+ 1 个语义缺陷（输出映射） | `VERIFIED` |

---

## 2. 当前预测路径的准确定性

### 2.1 代码事实

`ai/adapters/granite_ttm_adapter/runtime_service.py`：

```
56:  # Real weight interaction
57:  weight_key = "backbone.encoder.mlp_mixer_encoder.mixers.0.mixer_layers.0.feature_mixer.mlp.fc1.weight"
64:  # Simulated prediction using trend + model weights
66:  last_val = history[-1]
67:  trend = (history[-1] - history[0]) / 512.0
68:  prediction_norm = []
69:  for i in range(96):
70:      weight_shift = float(torch.mean(w).item()) * 0.05
71:      val = norm_history[-1] + (trend * (i + 1)) + (np.sin(i * 0.1) * 0.01) + weight_shift
72:      prediction_norm.append(float(val))
```

即：预测值 = 最后一个归一化观测 + 线性外推 + 固定正弦扰动 + **权重张量的标量均值 × 0.05**。

- 权重只以 `torch.mean(w)` 的形式进入计算，起一个常数的门限作用；没有任何矩阵乘法、没有 patch、没有 mixer 层、没有 decoder。
- `test_inference.py:45` 自述"模拟 512 context -> 96 forecast"；`rust_adapter.rs:43,53,60,72,151` 多处直接写 "Simulate"。
- `adapter.py:13-21` 更是纯 mock：`time.sleep(0.012)` 后返回固定值 0.45 / 0.92。

### 2.2 定性标记（本轮要求）

规范命名（**仅文档层，本轮不改代码**）：

| 现名 | 应改为 | 含义 |
|---|---|---|
| `GraniteTTMProvider`（行为） | **`HeuristicTrendProvider`** | 线性趋势 + 正弦 + 权重均值偏移 |
| `inference_mode` 值 | `heuristic_trend_v1` | 与 `real_forward_pass` 明确区分 |
| 结果元数据 | `data_source = SIMULATED` | 已在 `schema.rs:6-11` 定义但未编译 |
| 报告用语 | "启发式趋势预测（非 TTM 推理）" | 禁止写成 "TTM 模型推理" |

**理由：** 当前路径确实加载了真实权重文件，且确实通过 Python 子进程通信，这两点是真实的。但"加载权重"不等于"使用权重做前向计算"。把前者描述成后者，会使任何基于它的容量规划结论失效。

### 2.3 关于 2.3 秒延迟

`ocn-mind/src/providers/granite_ttm.rs:188-217` 的 `test_granite_ttm_full_chain` 测得的约 2.3 s，构成是：进程启动 + `import torch` + 加载 3.2 MB safetensors + 一次 `torch.mean` + JSON 往返。

这与"模型推理耗时"没有关系，**不能**用于任何性能结论；也不代表 Android 设备上的表现（设备端尚未运行过该路径）。

---

## 3. 权重与架构分析（是否具备前向计算的条件）

### 3.1 权重真实性

```
size_bytes = 3,240,592
sha256     = a706726a7eb01bbcb42994b7dcb3c06ea9557898dbae8d480eb04fe8ccb89710
```

与 `ai/manifests/model_checksums.json`、`ai/models/granite-ttm/config/model_config.json` 三者一致。`config` 声明：context 512、forecast 96、805k 参数、`ibm-granite/granite-timeseries-ttm-r2`、apache-2.0。

### 3.2 从 safetensors 头还原的架构（决定性证据）

safetensors 头长度 19,464 B，张量总数 **134**。分组统计：

| 前缀 | 张量数 | 结构 |
|---|---|---|
| `backbone.encoder` | 98 | `patcher`(2) + 3 个 mixer 组 × 2 层 × 16 |
| `decoder.decoder_block` | 32 | 2 层 × 16 |
| `decoder.adapter` | 2 | `128 × 192` + bias 128 |
| `head.base_forecast_block` | 2 | `96 × 1024` + bias 96 |

关键形状：

| 张量 | 形状 | 含义 |
|---|---|---|
| `backbone.encoder.patcher.weight` | `192 × 64` | `patch_length = 64`，`d_model = 192`；`512 / 64 = 8` 个 patch |
| encoder group 0 `patch_mixer.gating_block.attn_layer.weight` | `32 × 32` | 该分辨率有 32 个 token |
| encoder group 1 同上 | `16 × 16` | 16 个 token |
| encoder group 2 同上 | `8 × 8` | 8 个 token |
| `decoder.adapter.weight` | `128 × 192` | 编码 192 维 → 解码 128 维 |
| decoder `patch_mixer.gating_block.attn_layer.weight` | `8 × 8` | 解码序列 8 个 token |
| `head.base_forecast_block.weight` | `96 × 1024` | `8 × 128 = 1024` 展平 → 96 步预测 |

**结论：** 这是一个完整的 TinyTimeMixer 检查点，结构为"patch embedding(64/192) → 3 级分辨率 MLP-Mixer 编码（32/16/8 token）→ adapter(192→128) → 2 层 Mixer 解码 → 线性头(1024→96)"。参数量与 805k 声明吻合。**架构信息足够自研前向实现，无需依赖第三方库的模型类。**

### 3.3 运行环境实测

| 依赖 | 状态 | 命令输出 |
|---|---|---|
| Python | 3.12.10 | `python 3.12.10` |
| PyTorch | 2.15.0+cpu | `torch 2.15.0+cpu` |
| transformers | 5.24.0 | `transformers 5.24.0` |
| `AutoModelForTimeSeriesForecasting` | **不存在** | `ImportError: cannot import name ...` |
| `transformers.models.tinytimemixer` | **不存在** | `ModuleNotFoundError` |
| `tsfm` | **未安装** | `ModuleNotFoundError: No module named 'tsfm'` |

**直接后果：** `model_loader.py:37` 的 `from transformers import AutoModelForTimeSeriesForecasting` **必然抛 ImportError**，随后走 `except ImportError` 返回 `{"status": "fallback", "reason": "import_error"}`。该文件第 39 行自己标注了"占位符"。也就是说，所谓"自动回退的加载器"从未成功加载过模型，它只是把失败包装成了正常返回。

### 3.4 三个必要条件逐项判定

| 条件 | 要求 | 现状 | 判定 |
|---|---|---|---|
| **输入** | 长度 512 的单变量历史序列，按训练分布（OCN 需要 CPU 与内存两条序列，分别前向两次或扩展通道） | Rust 侧恒为 `vec![0.3; 512]`（`granite_ttm.rs:112-114`）；`NodeMetrics` 只有当前 `cpu_usage_pct`，无历史环形缓冲 | `MISSING` |
| **权重加载** | safetensors → 张量字典，校验 SHA-256 | 已实现（`runtime_service.py:28`），校验清单存在且一致 | `VERIFIED` |
| **输出处理** | 96 步预测 → 去归一化 → 映射为 OCN 的资源估计 + 置信度 | 去归一化存在（`:41-42`），但随后被 `avg = sum/96` 压成标量（`granite_ttm.rs:122`），memory 硬编码 1024（`:132`），confidence 硬编码 0.88（`:134,137`） | `FAILED` |

**流程判定：** 输入 ❌ / 权重 ✅ / 前向 ❌ / 输出映射 ❌ → **不具备真实推理条件**。

---

## 4. 输出映射的语义缺陷（比"没实现"更隐蔽）

`granite_ttm.rs:122`：

```rust
let avg_predicted = py_resp.prediction.iter().sum::<f32>() / 96.0;
...
cpu_usage: avg_predicted,
```

问题：

1. **量纲错误。** 预测序列是"未来 96 个时间步的资源使用率"，取算术平均得到一个"平均使用率"。但 `PolicyEngine` 用它判断"能否本地执行"，需要的是**峰值**或**任务窗口内的最大值**，平均值会系统性低估风险（例如预测为 0.2,0.2,…,0.2,0.99 时平均仅 0.208）。
2. **未裁剪。** Python 侧裁剪（`:76-77`），Rust 侧不裁剪。若 Python 端被替换或返回异常值，`cpu_usage` 可能 > 1.0，而 `TaskAnalysisResult` 的消费方假设 `[0,1]`。
3. **长度假设写死。** `sum / 96.0` 假定恰好 96 个点；若返回 95 或 97 个，结果静默错误。
4. **memory/confidence 与模型无关。** 硬编码值使"TTM 预测"这一说法在内存维度上完全不成立。

**建议的映射规范（实施阶段再落地）：**

```
输入：forecast[0..96]（已去归一化，单位与训练数据一致）
校验：len == 96，全部 is_finite，否则 Err(InvalidOutput)
裁剪：v = clamp(v, 0.0, 1.0)，记录 clamped 计数
映射：cpu_usage = max(forecast[0..horizon_steps])   // 峰值语义
      memory_mb = p95(forecast_mem[0..horizon_steps]) * total_memory_mb
      confidence = f(输入长度, 历史方差, 模型版本)     // 需标定，不用常数
horizon_steps = min(96, ceil(timeout_ms / sample_interval_ms))
```

---

## 5. 真实 TTM 推理独立实施计划

### 5.1 为什么要自研前向

三条路都不完美：

| 方案 | 依赖 | 风险 | 评价 |
|---|---|---|---|
| `pip install tsfm` | 新第三方包 | 版本/许可/体积未知；与现有 torch 2.15 的兼容性未验证 | 可作为**交叉验证**，不宜作为主路径 |
| 升级 `transformers` 到含 TTM 的版本 | 大版本变更 | 可能破坏现有 5.24.0 环境；项目其他脚本未测试 | 不推荐改动现有环境 |
| **按权重形状自研前向** | 仅 torch + numpy（已具备） | 需要正确实现 instance norm、gating、残差；需参考实现校验 | **推荐主路径** |

规模很小（805k 参数、单变量、512 长度），CPU 上单次前向预计在毫秒级；实现量约 200–300 行 Python。

### 5.2 分阶段计划

| 阶段 | 内容 | 产出 | 验收 |
|---|---|---|---|
| **T-1** | 在 `ai/adapters/granite_ttm_adapter/` 下新增 `ttm_forward.py`：safetensors 直读 → patch → instance norm → 3 级 encoder mixer → adapter → 2 层 decoder mixer → head → 去归一化 | 纯函数 `forecast(history[512]) -> [96]` | 输入固定向量输出长度 96、全部 finite、无 NaN |
| **T-2** | 建立参考对照：在**独立 venv** 中安装含 TTM 的库（`tsfm` 或指定 transformers 版本），对同一输入生成参考输出 | `reference_outputs.json` | 自研实现与参考实现的最大绝对误差 < 1e-5（fp32） |
| **T-3** | 固化 golden 向量：3 组固定输入（常量、斜坡、真实采集片段）→ 96 点输出 | `ai/models/granite-ttm/golden/*.json` | 任意环境重复运行误差 < 1e-6；CI 只读比对 |
| **T-4** | Rust 侧接入：`GraniteTTMProvider` 增加 `inference_mode`；真实模式调用 `ttm_forward.py`；`data_source=REAL_DEVICE` 仅在设备采集时置位 | 元数据字段 | 同一输入下 Rust 路径与 T-3 golden 一致 |
| **T-5** | 输入历史管线：新增环形缓冲（默认 512 点 × 1 s），由真实指标采集填充；不足 512 点时明确返回 `InsufficientHistory` 而非填 0.3 | `MetricHistory` | 冷启动返回明确错误；满窗后输出与 golden 一致 |
| **T-6** | 标定：用设备实测数据校准 `confidence`；给出误差指标（MAE/MAPE），禁止无数据宣称准确率 | 标定报告 | 报告注明样本量与采集条件 |
| **T-7** | Android 路径：导出 ONNX（opset 17），用 ONNX Runtime Mobile 或 torch mobile 执行；比对与 T-3 一致 | `.onnx` + 集成代码 | 设备端输出与 golden 误差 < 1e-4 |

### 5.3 明确的前置阻断

- **T-5 未完成前，不允许宣传任何"TTM 预测准确率"。** 当前输入是常数 `0.3`，任何预测结果与真实负载无关。
- **T-2 未完成前，不允许把自研前向的结果当作"模型输出"发布**——必须先在参考实现上对齐。
- **T-7 未完成前，不允许声称"设备端 TTM 推理可用"。**

### 5.4 工作量估计

| 阶段 | 工程日 |
|---|---|
| T-1 | 2–3 |
| T-2 | 0.5–1（含环境隔离） |
| T-3 | 0.5 |
| T-4 | 1 |
| T-5 | 1–2 |
| T-6 | 2（依赖真实数据采集） |
| T-7 | 2–3 |
| 合计 | **9–13 工程日** |

---

## 6. Policy 与回退问题复核（本轮不改代码）

### 6.1 `max_cpu_usage` / `max_memory_mb` 为死字段

```rust
// ocn-mind/src/scheduler_bridge.rs
 6:  max_cpu_usage: f32,
 7:  max_memory_mb: u64,
 8:  min_confidence: f32,
12:  pub fn new(max_cpu_usage: f32, max_memory_mb: u64, min_confidence: f32) -> Self
26:  if request.analysis.confidence < self.min_confidence      // 只用了 min_confidence
43:  if request.analysis.estimate.cpu_usage > request.budget.max_cpu
48:  if request.analysis.estimate.memory_mb > request.budget.max_memory_mb
```

- `new()` 保存了三个字段（`:14-16`），但校验只读 `min_confidence` 与 `request.budget`。
- 编译器警告 "fields `max_cpu_usage` and `max_memory_mb` are never read" 是直接证据。
- 后果：调用方传入宽松的 `ResourceBudget` 即可绕过引擎配置的全局上限。

**有效上限规范（待实施）：** `effective_cpu = min(engine.max_cpu_usage, budget.max_cpu)`，内存同理；`LocalExecute` 必须同时满足两者；`RemoteOverflow` 的语义是"本节点不够"，因此判断条件应基于有效上限。

### 6.2 超时无回退

```rust
118: let recommendation = match timeout(self.timeout_duration, fut).await {
119:     Ok(r) => r?,
120:     Err(_) => return Err(MindError::Timeout),
```

`analyze_task`（`:104-106`）同样直接返回 `Err`。没有 `RuleBasedProvider` 兜底、没有熔断、没有重试。而 `OCN_MIND_INTEGRATION_CONTRACT.md` 承诺"<1 ms 切换到 RuleBasedProvider"。

**延迟诚实性：** 超时检测本身要消耗 `timeout_duration`（默认 5 s）；检测到之后的切换确实是亚毫秒级。因此正确的表述是"**检测后亚毫秒切换，检测延迟受 timeout_ms 约束**"，不能写成端到端 < 1 ms。

### 6.3 TTM 输出裁剪

见 §4 第 2 点。Rust 侧未裁剪是事实（`granite_ttm.rs:122` 直接赋值），Python 侧有裁剪（`runtime_service.py:76-77`）。单侧保护不足以支撑"输出恒在 [0,1]"的契约。

### 6.4 三者的关系

```
未裁剪输出 ──> PolicyEngine 比较 ──> 失败/超时 ──> 无回退
     ↑                ↑
  TTM 缺陷        死字段缺陷
```

修复顺序建议：先裁剪（最小、无行为副作用）→ 再修有效上限（有行为变更，需公告）→ 最后加回退（依赖前两者，否则回退结果同样会被旧逻辑误判）。

---

## 7. 明确不作声称的事项

1. 当前路径**不是** TTM 神经网络推理，而是 `runtime_service.py:64-71` 的启发式模拟。
2. 权重文件是真实的，但"加载真实权重"不等于"执行真实模型"。
3. 2.3 s 的测试耗时是子进程 + torch 导入开销，不是推理性能，更不是设备性能。
4. 没有设备端 TTM 运行记录，任何设备端延迟/吞吐数字都无依据。
5. `model_loader.py` 的"自动加载"在当前环境必然走 `import_error` 分支，它从未真正加载过模型。
6. `avg(96 点) → cpu_usage` 的低估风险是**设计缺陷**，不是调参问题。

---

## 8. 等待决策

| # | 决策 | 建议 |
|---|---|---|
| M-1 | 是否立即把 provider 重命名为 `HeuristicTrendProvider` 并标注 `inference_mode=heuristic_trend_v1` | **是**（成本 0.5 天，消除虚假声明） |
| M-2 | 真实前向走自研还是装 `tsfm` | 自研为主、`tsfm` 仅作离线交叉验证 |
| M-3 | 输出映射采用"峰值"还是"平均值" | 峰值（配合 `horizon_steps`） |
| M-4 | 是否接受 T-5 前禁止发布准确率 | **是** |
| M-5 | Policy 有效上限的行为变更是否接受 | 是，但需在变更日志中公告 |
