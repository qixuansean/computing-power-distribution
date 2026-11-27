# OCN IBM Granite TTM 真实模型集成计划

## 阶段 1：模型技术审计
IBM Granite TTM-R2 模型 (HuggingFace: `ibm-granite/granite-timeseries-ttm-r2`)：
- **许可**: Apache 2.0 (完全商用兼容)。
- **架构**: Patch-based Time Series Mixer，基于 MLP-Mixer 的轻量变体。
- **输入**: `(batch_size, context_length, num_variates)`。
- **输出**: `(batch_size, forecast_length, num_variates)`。
- **上下文要求**: TTM-R2 默认上下文长度为 512，预测长度为 96。
- **数据处理**: 需要外部 `StandardScaler` 进行数据标准化。
- **推理后端**: 建议使用 `huggingface/tsfm` (Time Series Foundation Models) 库进行轻量推理。
- **CPU/ARM兼容**: 模型极其轻量 (<1M 参数)，完美契合 CPU 推理及 Edge/Mobile 设备。

## 阶段 2：权重管理与校验
将在 `ai/scripts/manage_model.py` 中扩展以下功能：
- **模型来源**: `https://huggingface.co/ibm-granite/granite-timeseries-ttm-r2/resolve/main/model.safetensors`
- **下载与校验**: 使用 SHA256 确保模型权重完整性。
- **原子性**: 下载到 `temp/` 后进行校验，校验通过后覆盖 `ai/models/granite-ttm/model/`。

## 阶段 3 & 4：Windows CPU 推理与架构桥接
- **技术栈**: Rust `tokio` (主进程) + Python `PyTorch`/`tsfm` (推理子进程)。
- **通信**: 使用简单且高效的 Unix Domain Socket (UDS) / Named Pipes 进行跨进程 IPC，而非 gRPC 以减轻开销。
- **回退机制**: Rust 层处理 Python 进程心跳，超时或进程退出后直接切换回 `RuleBasedProvider`。

## 阶段 5 & 6：集成与测试
- **MindProvider 接入**: `GraniteTTMProvider` 将更新 `analyze_task` 实现，将 OCN 历史序列数据包装成 TTM 张量。
- **基准测试**: 对比 `RuleBased` 与 `TTM` 预测性能。

---

## 下一步行动方案
1. **完善下载脚本**: 实现 `ai/scripts/manage_model.py` 的下载与 SHA256 验证功能。
2. **构建推理服务**: 实现基于 Socket 的 Python 推理 Runtime。
3. **真实模型推理验证**: 加载真实权重并运行基准测试。

请确认是否批准上述技术路径，我将立即开始执行第一步权重管理工作。
