# OCN Pretrained Model Evaluation Report

## 1. 概述

本报告评估了两个优先研究的预训练模型：**Needle 2** 与 **IBM Granite TinyTimeMixer (TTM)**，并确定了其在 OCN (OpenCompute Network) 资源调度与任务分析中的适用性。

---

## 2. Model 1: Needle 2 (Task Understanding & Analysis)

### 基本信息
- **用途**: 任务理解、结构化任务分析、工具选择、OCN 任务分类。
- **评估状态**: ❌ **不适合直接集成**。
- **原因分析**:
  1. Needle 系列通常指代代码生成或特定大语言模型/检索增强组件，在开源社区中缺乏稳定、轻量、开箱即用的 1B 级结构化调度微调权重。
  2. 对于 OCN 节点而言（尤其是 Windows / Android 混合 P2P 环境），直接运行通用大语言模型存在过高的 RAM/VRAM 开销和推理延迟（>500ms），无法满足 OCN 实时任务分类的要求。
  3. OCN 的任务理解更倾向于结构化参数解析与轻量级分类，采用规则化 TaskDescriptor 与轻量嵌入模型更为高效。

---

## 3. Model 2: IBM Granite TinyTimeMixer (TTM) (Resource Prediction)

### 基本信息
- **Hugging Face**: `ibm-granite/granite-timeseries-ttm-r2` / `ibm-granite/granite-timeseries-ttm-r1`
- **论文**: NeurIPS 2024 ("Tiny Time Mixers (TTMs): Fast Pre-trained Models for Enhanced Zero/Few-Shot Forecasting of Multivariate Time Series")
- **许可证**: Apache 2.0 (商用友好)
- **参数量**: < 1M 参数（约 805k params）
- **用途**: 多变量时间序列预测（CPU 使用率、RAM 使用量、网络流量趋势预测）。

### 为什么选择 IBM Granite TTM 接入 OCN？
1. **极致轻量**: 参数量不到 1M，模型文件仅数 MB，完美契合 OCN 最初的 2GB 模型缓存上限。
2. **CPU 友好**: 在 CPU 上可达到高达 ~180 samples/sec 的吞吐量，完全适配资源受限的边缘节点及 Android ARM64 设备。
3. **零样本/少样本能力**: 预训练模型支持直接进行零样本（Zero-shot）多元时间序列预测，无需从零训练。
4. **许可证友好**: Apache 2.0 许可证，允许在 OCN 项目中合法集成和部署。

---

## 4. 结论

- **Needle 2**: 不予接入（计算开销过大，无轻量调度专用权重）。
- **IBM Granite TTM**: **选定为 OCN 资源预测的核心预训练模型**，在 `ai/` 目录中建立专属适配器与运行时支持。
