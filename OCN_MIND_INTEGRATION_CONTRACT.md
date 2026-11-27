# OCN Mind Integration Contract

## 1. 概述
本文件定义了 `GraniteTTMProvider` 与 OCN 主线系统间的契约。该组件旨在提供任务资源消耗的实时预测，同时确保系统在极端情况下的确定性回退。

## 2. 预测数据语义契约

| 字段 | 含义 | 单位 | 范围 |
|---|---|---|---|
| Input Channel | CPU Usage | Ratio | [0.0, 1.0] |
| Context Window | 512 points | Seconds | 512s (1s per point) |
| Forecast Horizon | 96 points | Seconds | 96s (1s per point) |

### 归一化公式 (TTM-R2 官方实现)
- **Preprocess**: $x_{norm} = \frac{x - \mu}{\sigma + 1e-6}$
- **Postprocess**: $x_{real} = x_{pred} \cdot \sigma + \mu$
- **参数存储**: 均值 $\mu$ 和标准差 $\sigma$ 实时计算于输入窗口。

### 边界处理
- 输入超出 [0.0, 1.0] 的数据会被 Runtime 逻辑拦截。
- 预测输出通过 `max(0.0, min(1.0, v))` 强制截断至资源边界。

---

## 3. IPC 稳定性契约

- **正常退出**: `exit` 指令关闭 Python 子进程。
- **异常退出**: Rust 子进程管理器监控 `Child::wait()`，检测到状态异常后重置 `python_process` 并更新 `AtomicBool` 状态。
- **超时处理**: 配置 `timeout_ms: 5000`。若推理超过此阈值，Provider 返回 `MindError::Timeout`，触发 `SchedulerBridge` 自动切换至 `RuleBasedProvider`。
- **Invalid JSON**: Python 侧在 JSON 解析失败时输出 `{"status": "error", "message": "..."}`。Rust 侧以此作为故障信号，不引发 panic，而是返回 `MindError`。

---

## 4. MindProvider 契约接口定义

| 对象 | 字段 | 单位 | 约束 |
|---|---|---|---|
| **ResourceEstimate** | `cpu_usage` | Ratio | [0.0, 1.0] |
| | `memory_mb` | MB | [128, 8192] |
| | `confidence` | N/A | [0.0, 1.0] |
| **SchedulingRecommendation** | `score` | N/A | [0.0, 1.0] |

- **失败行为**: 当模型不可用或分析超时，必须在 <1ms 内切换至 `RuleBasedProvider`。
- **权限限制**: AI 仅通过 `TaskAnalysisResult` 建议资源配额。最终节点分配与任务拒绝由 `PolicyEngine` (Rule-Based) 强制执行，AI 无法绕过安全校验。
