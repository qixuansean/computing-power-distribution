# OCN Model Integration Architecture Specification

## 1. 架构概述

本架构定义了 OCN 统一 AI 层（OCN Mind）与外部预训练模型、本地规则基线之间的集成规范。

核心原则：
1. **模型与 OCN Core 分离**: 模型运行在独立进程或容器中，通过版本化 API 通信。
2. **确定性兜底**: 模型崩溃、超时或未加载时，自动回退到 RuleBasedProvider。
3. **安全隔离**: 模型不能直接执行系统命令，不能绕过确定性 Scheduler。
4. **统一接口**: 所有模型必须实现统一的 `MindProvider` Trait。

---

## 2. 系统架构图

```
+---------------------------------------------------------------+
|                        OCN Node (Rust)                        |
|                                                               |
|  +------------------+     +-------------------------------+   |
|  | Deterministic    | <---| Scheduler (Final Verification)|   |
|  | Scheduler        |     +-------------------------------+   |
|  +------------------+                    ^                    |
|           ^                              |                    |
|           | (Fallback)          (Structural Recommendation)   |
|           v                              |                    |
|  +---------------------------------------------------------+  |
|  |                   Runtime Manager                       |  |
|  |  - RuleOnly   - Compact   - Full   - Unloaded           |  |
|  +---------------------------------------------------------+  |
|                              ^                                |
|                              | gRPC / IPC                     |
|                              v                                |
+------------------------------|--------------------------------+
                               |
+------------------------------v--------------------------------+
|                   External Model Runtime                      |
|                                                               |
|  +--------------------+  +------------------+  +-----------+  |
|  | DecimaAdapter      |  | HwoaRlAdapter    |  | Future... |  |
|  | (GCN / RL)         |  | (PPO / LSTM-AE)  |  |           |  |
|  +--------------------+  +------------------+  +-----------+  |
+---------------------------------------------------------------+
```

---

## 3. 核心组件规范

### 3.1 MindProvider Trait
所有调度提供者必须实现以下 Rust 异步接口：

```rust
#[async_trait]
pub trait MindProvider: Send + Sync {
    async fn predict_scheduling(
        &self, 
        task: &TaskDescriptor, 
        nodes: &[NodeSnapshot]
    ) -> Result<SchedulingRecord, MindError>;
    
    fn name(&self) -> &str;
}
```

### 3.2 运行时管理器 (Runtime Manager)
负责根据系统资源、前台状态和用户配置自动切换运行模式：

- **Unloaded**: 未加载模型，仅使用规则。
- **RuleOnly**: 显式指定或回退状态，仅使用规则。
- **Compact**: 加载轻量级预测模型（如小型 MLP）。
- **Full**: 加载完整模型（如 PPO / GCN 适配器）。

### 3.3 故障回退与安全机制
1. **推理超时**: 若模型推理超过 100ms，触发超时异常并自动回退。
2. **模型崩溃**: IPC 连接断裂或服务崩溃时，隔离故障并切换至 `RuleOnly`。
3. **输出验证**: 确定性 Scheduler 重新验证节点身份、资源预算、网络约束和截止时间。

---

## 4. 存储与配额管理

- **初始配额**: 2GB 模型缓存上限。
- **原子替换**: 下载新模型时写入临时文件，校验 SHA256 后原子重命名。
- **清理机制**: 自动清理未使用的临时文件，永不自动删除用户数据或唯一模型版本。
