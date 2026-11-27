# OCN Pretrained Model Research Report

## 概述

本报告分析了两个候选的预训练计算资源调度模型，评估其与 OCN 的兼容性和集成可能性。

## Candidate A: Decima

### 基本信息
- **GitHub**: https://github.com/hongzimao/decima-sim
- **Stars**: 330
- **Forks**: 94
- **License**: 未明确指定

### 模型架构
- **类型**: 基于强化学习的 Spark 作业调度器
- **算法**: Actor-Critic + Graph Neural Network (GCN)
- **框架**: TensorFlow 1.x
- **环境**: Spark 模拟器

### 技术特点
- 使用消息传递机制处理任务依赖关系
- 支持动态任务到达和随机任务终止
- 实现了差异化奖励机制
- 支持多代理协作

### 预训练权重状态
❌ **无预训练权重文件**
- 只有训练脚本和测试脚本
- 需要从零开始训练模型
- 训练数据：Google 集群工作负载 traces

### 训练要求
- **数据量**: 需要 Google 集群 traces (约 50GB)
- **训练时间**: 数小时到数天不等
- **硬件要求**: GPU 推荐
- **依赖**: TensorFlow 1.x, Spark

### OCN 兼容性分析

#### ✅ 兼容部分
1. **输入数据**: 可以转换为 OCN TaskDescriptor
2. **输出**: 可以映射到 SchedulingRecommendation
3. **任务依赖**: 支持复杂任务关系
4. **动态环境**: 支持节点动态变化

#### ❌ 不兼容部分
1. **平台依赖**: 专为 Spark 设计，难以适配 P2P 网络
2. **网络模型**: 未考虑网络延迟和带宽
3. **异构节点**: 主要针对同构集群
4. **移动设备**: 不支持移动设备场景
5. **前台保护**: 无 Gaming Assist 支持

### 集成复杂度: **高**

---

## Candidate B: HWOA-RL-XAI Cloud Scheduler

### 基本信息
- **GitHub**: https://github.com/bnreddy6968-a11y/HWOA-RL-XAI-Cloud-Scheduler
- **Stars**: 0
- **Forks**: 0
- **License**: MIT

### 模型架构
- **类型**: 混合云工作负载调度框架
- **算法**: 
  - LSTM-Autoencoder (预测)
  - Whale Optimization Algorithm (优化)
  - PPO-RL (调度)
  - SHAP (可解释性)
- **框架**: PyTorch + Stable-Baselines3
- **环境**: CloudSim / Kubernetes

### 技术特点
- 四层混合架构
- 多目标优化：makespan、SLA、能源效率
- 支持 500 VM 规模
- 实时可解释性分析

### 预训练权重状态
❌ **声称有预训练权重但实际不存在**
- README 声称有 4 个预训练模型文件
- 实际访问 `pretrained/` 目录返回 404
- 无 `download_pretrained.sh` 脚本
- 项目结构不完整，多个目录无法访问

### 声称的预训练模型
| Scale | Checkpoint | 声称大小 | 实际状态 |
| --- | --- | --- | --- |
| 50 VM | `pretrained/hwoa_rl_xai_50vm.pt` | 48 MB | ❌ 不存在 |
| 200 VM | `pretrained/hwoa_rl_xai_200vm.pt` | 48 MB | ❌ 不存在 |
| 500 VM | `pretrained/hwoa_rl_xai_500vm.pt` | 48 MB | ❌ 不存在 |
| LSTM-AE | `pretrained/lstm_ae_google.pt` | 12 MB | ❌ 不存在 |

### 训练要求
- **数据量**: Google 集群 traces + RUBiS 基准
- **训练时间**: 35 GPU-hours (A100)
- **硬件要求**: GPU 推荐
- **依赖**: PyTorch, Kubernetes, CloudSim

### OCN 兼容性分析

#### ✅ 兼容部分
1. **多目标优化**: 支持 makespan、SLA、能源效率
2. **异构环境**: 设计用于异构云基础设施
3. **可解释性**: SHAP 提供决策解释
4. **规模扩展**: 支持 500 VM 规模

#### ❌ 不兼容部分
1. **Kubernetes 依赖**: 强依赖 Kubernetes 部署
2. **CloudSim 依赖**: 需要特定模拟环境
3. **网络模型**: 主要关注计算资源，网络考虑不足
4. **移动设备**: 不支持移动场景
5. **前台保护**: 无 Gaming Assist 支持
6. **P2P 网络**: 设计为集中式调度

### 集成复杂度: **极高**

---

## 总体评估

### 真实可用性
- **Decima**: ❌ 需要从零训练，无预训练模型
- **HWOA-RL-XAI**: ❌ 声称的预训练模型不存在

### 实际文件大小
- **Decima**: 约 2MB (代码文件)
- **HWOA-RL-XAI**: 约 1MB (代码文件)

### 运行环境要求
- **Decima**: Python 3.x, TensorFlow 1.x, Spark
- **HWOA-RL-XAI**: Python 3.9+, PyTorch, Kubernetes, Docker

### OCN 适配挑战

#### 1. 架构差异
- 两个项目都设计为集中式调度器
- OCN 需要分布式 P2P 调度
- 缺乏移动设备支持

#### 2. 网络考虑
- 主要关注计算资源调度
- 网络延迟和带宽建模不足
- 缺乏动态网络拓扑处理

#### 3. 前台保护
- 无 Gaming Assist 机制
- 缺乏前台任务优先级处理
- 无实时性能保护

#### 4. 模型可用性
- 无实际预训练权重
- 需要大量训练资源和时间
- 依赖特定的数据集和环境

## 建议方案

### 阶段 1: 基础集成
1. **保留 RuleBasedProvider** 作为基础调度器
2. **创建模型适配器接口**，支持未来集成
3. **实现基本的 Schema 转换**

### 阶段 2: 研究验证
1. **在小规模数据上训练简化模型**
2. **验证 OCN 特定场景的适用性**
3. **建立 OCN 专用的评估基准**

### 阶段 3: 逐步集成
1. **从简单的资源预测模型开始**
2. **逐步添加复杂调度策略**
3. **保持与现有 OCN Core 的兼容性**

## 结论

两个候选项目都无法直接用于 OCN，主要原因：
1. **无预训练权重** - 需要从零训练
2. **架构不匹配** - 设计为集中式云调度
3. **OCN 特性缺失** - 缺乏 P2P、移动设备、前台保护支持
4. **部署复杂度高** - 依赖复杂的运行环境

建议继续使用 RuleBasedProvider 作为基础，同时为未来模型集成建立完善的接口和框架。
