# OCN AI Compute Architecture

## 1. 概述
AI Compute 是 OCN 的高级调度任务负载。模型作为计算节点上的一等公民，通过 `MindProvider` Trait 暴露能力。

## 2. 接口设计
- **AI Task Adapter**: 封装推理任务，适配 OCN 任务描述符。
- **Inference Runtime**: Python 侧运行 PyTorch/ONNX，Rust 侧通过 IPC 通信。
- **Resource Discovery**: 在 `NodeSnapshot` 中增加 AI 算力标签。

## 3. 分布式推理策略
- **负载分担**: 任务分析在源节点 (Windows)，资源趋势预测在邻近节点 (Android)。
- **决策**: Scheduler 根据 `inference_latency_ms` 与 `network_latency_ms` 计算综合 Cost。

## 4. 交付清单 (待完成)
- [ ] 真实推理实现
- [ ] 分布式推理基准对比
- [ ] Android ARM64 实机运行验证
