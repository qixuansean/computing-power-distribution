# OCN - Decima 兼容性报告

## 项目概述

Decima 是一个基于强化学习的 Spark 作业调度器，使用 Actor-Critic 和图神经网络 (GCN) 来优化数据处理集群的调度算法。

## 技术规格

| 组件 | 描述 |
| --- | --- |
| **算法** | Actor-Critic + GCN + 消息传递 |
| **框架** | TensorFlow 1.x |
| **环境** | Spark 模拟器 |
| **数据** | Google 集群工作负载 traces |
| **规模** | 支持 50 个执行器，200+ 流式作业 |

## OCN Schema 兼容性

### ✅ TaskDescriptor 映射

| Decima 概念 | OCN TaskDescriptor | 兼容性 |
| --- | --- | --- |
| DAG 任务 | `task_id` | ✅ 可映射 |
| 任务类型 | `task_type` | ✅ 需要标准化 |
| 优先级 | `priority` | ✅ 可直接映射 |
| 截止时间 | `deadline` | ✅ 可添加 |
| 输入大小 | `input_size` | ✅ 需要估算 |
| 计算需求 | `required_capabilities` | ✅ 可映射 |
| 执行模式 | `allowed_execution_modes` | ✅ 需要定义 |

**转换示例**:
```rust
// Decima DAG -> OCN TaskDescriptor
fn convert_decima_dag_to_ocn_task(dag: &DAG) -> TaskDescriptor {
    TaskDescriptor {
        task_id: dag.id,
        task_type: "SPARK_JOB".to_string(),
        priority: dag.priority.unwrap_or(1),
        deadline: Utc::now() + Duration::from_secs(3600),
        input_size: dag.estimated_input_size,
        required_capabilities: vec![
            "spark_executor".to_string(),
            "memory_gb_4".to_string(),
        ],
        allowed_execution_modes: vec!["local".to_string(), "distributed".to_string()],
        schema_version: "1.0".to_string(),
    }
}
```

### ✅ NodeSnapshot 映射

| Decima 概念 | OCN NodeSnapshot | 兼容性 |
| --- | --- | --- |
| 执行器状态 | `cpu_available` | ✅ 可映射 |
| 内存使用 | `memory_available` | ✅ 可映射 |
| 网络延迟 | `network_latency` | ⚠️ 需要添加 |
| 网络带宽 | `network_bandwidth` | ⚠️ 需要添加 |
| 热状态 | `thermal_state` | ✅ 可添加 |
| 电源状态 | `power_state` | ✅ 可添加 |

**转换示例**:
```rust
// Decima Executor -> OCN NodeSnapshot
fn convert_decima_executor_to_ocn_node(executor: &Executor) -> NodeSnapshot {
    NodeSnapshot {
        node_id: executor.id,
        platform: "spark".to_string(),
        cpu_available: 1.0 - executor.cpu_usage,
        memory_available: executor.available_memory,
        gpu_available: None, // Decima 不支持 GPU
        network_latency: 10.0, // 静态值，需要动态获取
        network_bandwidth: 1000.0, // 静态值
        thermal_state: "normal".to_string(),
        power_state: "ac".to_string(),
        timestamp: Utc::now(),
        schema_version: "1.0".to_string(),
    }
}
```

## OCN 特性支持评估

### ❌ 不支持的特性

#### 1. P2P 网络支持
- **问题**: Decima 设计为集中式调度器
- **影响**: 无法处理节点间的直接通信
- **适配难度**: 极高
- **建议**: 需要重新设计网络模型

#### 2. 动态节点加入/退出
- **问题**: 模拟器环境，不支持真实动态性
- **影响**: 无法处理移动设备的频繁变化
- **适配难度**: 高
- **建议**: 需要添加动态节点管理机制

#### 3. 异构节点支持
- **问题**: 主要针对同构 Spark 集群
- **影响**: 无法处理不同 CPU/GPU 架构
- **适配难度**: 高
- **建议**: 需要扩展节点能力描述

#### 4. 移动设备支持
- **问题**: 无移动设备相关考虑
- **影响**: 无法适配 Android 设备
- **适配难度**: 极高
- **建议**: 需要重新设计设备模型

#### 5. 前台保护 (Gaming Assist)
- **问题**: 无前台任务优先级机制
- **影响**: 无法保护游戏等前台应用
- **适配难度**: 高
- **建议**: 需要添加前台检测和保护机制

#### 6. 网络延迟和带宽建模
- **问题**: 主要关注计算资源，网络建模简单
- **影响**: 无法优化网络敏感任务
- **适配难度**: 中等
- **建议**: 需要增强网络模型

### ✅ 支持的特性

#### 1. 任务依赖关系
- **支持**: 通过 DAG 和消息传递机制
- **优势**: 可以处理复杂任务依赖
- **OCN 适配**: 需要映射到 OCN 的任务关系模型

#### 2. 多目标优化
- **支持**: 通过差异化奖励机制
- **优势**: 可以平衡多个调度目标
- **OCN 适配**: 需要适配 OCN 的特定目标

#### 3. 强化学习框架
- **支持**: 完整的 RL 实现
- **优势**: 可以通过学习优化调度策略
- **OCN 适配**: 需要重新定义状态空间和奖励函数

## 集成架构设计

### 1. 适配器接口
```rust
pub struct DecimaAdapter {
    model_path: String,
    decima_env: DecimaSimulator,
    ocn_converter: DecimaOCNConverter,
}

#[async_trait]
impl MindProvider for DecimaAdapter {
    async fn predict_scheduling(&self, task: &TaskDescriptor, nodes: &[NodeSnapshot]) -> Result<SchedulingRecord, MindError> {
        // 1. 转换 OCN 数据为 Decima 格式
        let decima_task = self.ocn_converter.to_decima_task(task);
        let decima_nodes = self.ocn_converter.to_decima_nodes(nodes);
        
        // 2. 运行 Decima 调度
        let decima_decision = self.decima_env.schedule(decima_task, decima_nodes).await?;
        
        // 3. 转换回 OCN 格式
        let ocn_decision = self.ocn_converter.from_decima_decision(decima_decision);
        
        Ok(ocn_decision)
    }
}
```

### 2. 数据转换层
```rust
pub struct DecimaOCNConverter {
    // 任务转换
    task_type_mapping: HashMap<String, String>,
    capability_mapping: HashMap<String, String>,
    
    // 节点转换
    resource_unit_converter: ResourceUnitConverter,
    
    // 决策转换
    decision_mapper: DecisionMapper,
}
```

### 3. 运行时管理
```rust
pub struct DecimaRuntimeManager {
    adapter: DecimaAdapter,
    fallback: RuleBasedProvider,
    current_mode: RuntimeMode,
}

impl DecimaRuntimeManager {
    pub async fn schedule(&mut self, task: &TaskDescriptor, nodes: &[NodeSnapshot]) -> Result<SchedulingRecord, MindError> {
        match self.current_mode {
            RuntimeMode::Full => self.adapter.predict_scheduling(task, nodes).await,
            RuntimeMode::Compact => self.adapter.predict_scheduling(task, nodes).await,
            RuntimeMode::RuleOnly => self.fallback.predict_scheduling(task, nodes).await,
            RuntimeMode::Unloaded => self.fallback.predict_scheduling(task, nodes).await,
        }
    }
}
```

## 实施挑战

### 1. 训练数据需求
- **问题**: 需要 Google 集群 traces (约 50GB)
- **解决方案**: 使用 OCN 模拟数据生成训练数据
- **时间**: 预计 2-3 周数据收集

### 2. TensorFlow 1.x 依赖
- **问题**: TensorFlow 1.x 已过时，维护困难
- **解决方案**: 迁移到 TensorFlow 2.x 或 PyTorch
- **时间**: 预计 1-2 周迁移

### 3. Spark 模拟器适配
- **问题**: 模拟器与真实 OCN 环境差异大
- **解决方案**: 创建 OCN 专用模拟器
- **时间**: 预计 3-4 周开发

### 4. 性能优化
- **问题**: 推理延迟可能影响实时调度
- **解决方案**: 模型量化和异步推理
- **时间**: 预计 1-2 周优化

## 兼容性评分

| 维度 | 评分 | 说明 |
| --- | --- | --- |
| **架构兼容性** | 3/10 | 集中式 vs 分布式 |
| **数据格式兼容性** | 7/10 | 可以映射但有差异 |
| **网络支持** | 2/10 | 缺乏 P2P 和移动网络 |
| **移动设备支持** | 1/10 | 无移动设备考虑 |
| **前台保护** | 1/10 | 无 Gaming Assist |
| **部署复杂度** | 4/10 | 需要大量适配工作 |
| **维护成本** | 3/10 | TensorFlow 1.x 过时 |
| **扩展性** | 5/10 | 可以扩展但需要重构 |

**总体评分: 3.2/10**

## 建议方案

### 短期 (1-2 个月)
1. **保留 RuleBasedProvider** 作为主要调度器
2. **创建 Decima 适配器接口** 作为研究原型
3. **使用 OCN 模拟数据** 进行小规模实验

### 中期 (3-6 个月)
1. **迁移到现代框架** (PyTorch)
2. **创建 OCN 专用训练数据**
3. **实现基础调度功能**

### 长期 (6-12 个月)
1. **完整集成到 OCN Core**
2. **性能优化和量化**
3. **生产环境部署**

## 结论

Decima 与 OCN 的兼容性较低，主要原因：
1. **架构不匹配**: 集中式 vs 分布式
2. **缺乏移动设备支持**
3. **网络模型过于简单**
4. **技术栈过时** (TensorFlow 1.x)

建议将其作为研究原型，而不是直接的生产集成方案。优先考虑开发 OCN 专用的调度模型。
