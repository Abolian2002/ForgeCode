# MiniCode `/goal` 长时自主执行模式设计文档

## 一、设计背景与目标

### 1.1 背景

MiniCode 目前已有两种执行模式：

| 模式 | 特点 | 适用场景 | 运行时长 |
|------|------|---------|---------|
| **普通模式** | 单 Agent TAOR 循环，用户每次发指令执行一轮 | 简单问答、小修改、交互式开发 | 几分钟 |
| **团队模式 `/team`** | 主 Agent 编排多个子 Agent 分阶段并行执行，单轮结束 | 中大型任务、需要专业分工 | 几十分钟 |

这两种模式的共同局限：**每一轮执行完毕后必须等待用户下一条指令**。对于大型目标（如"实现一个完整的用户认证系统"、"把整个项目从 Actix Web 迁移到 Axum"、"为所有模块补全单元测试到 80% 覆盖率"），用户需要反复发指令、监控进度、纠正方向，体验不连贯。

2026 年初，Claude Code 推出了 `/goal`（自主目标模式），Codex 推出了 Goal Mode，核心思路一致：**用户给出一个高层目标，Agent 自主连续工作数小时甚至十几小时，自动规划、执行、反思、纠错、推进，直至目标达成或遇到无法自主解决的阻塞**。

### 1.2 参考产品分析

#### Claude Code `/goal` 核心特征（基于源码泄露和实测分析）

1. **TAOR Loop (Think-Act-Observe-Repeat)**：运行时极其精简（~50行），所有智能决策交给模型
2. **Task 状态机**：pending → in_progress → blocked → done，跨会话持久化
3. **Auto-Compaction**：上下文到 ~50% 自动摘要压缩，防止 Context Collapse
4. **Self-Reflection**：每完成一个子任务后自动反思进度、调整计划
5. **Watchdog（看门狗）**：检测死循环/停滞，超过 N 次相似操作自动触发反思或暂停
6. **Checkpoint 机制**：每个里程碑自动保存检查点（包括 git commit），可随时回滚
7. **Budget 控制**：Token 预算、时间预算、最大轮次，防止无限烧钱
8. **后台运行**：可以 detach 到后台，用户关闭 TUI 后继续执行（Daemon 模式）
9. **Resume 能力**：中断后可从检查点恢复继续执行
10. **子 Agent 卸载**：重型探索/执行任务派发给子 Agent，保护主上下文

#### Codex Goal Mode 核心特征

1. **目标分解为 Todo 列表**：Agent 自己维护结构化的待办清单
2. **自动验证**：每个子任务完成后自动运行测试验证
3. **交付物导向**：用户明确交付物，Agent 自主规划到交付
4. **手机控制台**：通过手机端监控进度、接收通知、审批关键操作
5. **远程执行**：可以在远程服务器上持续运行

### 1.3 设计目标

在现有 MiniCode 架构基础上，新增 `/goal` 长时自主执行模式，实现：

- **持续自主工作**：用户给出一个高层目标后，Agent 自主连续工作，无需用户逐步指令
- **可运行数小时**：通过上下文管理、检查点、自反思等机制支持长时运行
- **安全可控**：多层预算控制、关键操作审批、随时可暂停/中断/回滚
- **跨会话恢复**：即使 TUI 关闭/崩溃/重启，也能从检查点恢复
- **渐进式实现**：从 MVP 开始，迭代增强，不破坏现有模式

### 1.4 设计原则

| 原则 | 说明 |
|------|------|
| **运行时"愚笨"，模型智能** | 核心循环尽量精简，决策交给模型（参考 Claude Code 的 TAOR 设计哲学） |
| **上下文是稀缺资源** | 主动管理：自动压缩、子 Agent 卸载、摘要替换，防止 Context Collapse |
| **记忆是索引不是存储** | 能从代码库重新推导的信息不存储，Goal 状态只记录关键决策和进度 |
| **安全网层层叠加** | Token 预算 + 时间预算 + 轮次限制 + Watchdog + 关键操作审批 |
| **检查点优先** | 每个里程碑持久化状态，支持随时暂停/恢复/回滚 |
| **可组合的权限光谱** | Goal 模式有独立的权限档位，适配不同信任级别 |
| **最小侵入** | 在现有架构上扩展，不破坏普通模式和团队模式 |

---

## 二、与现有模式的关系

### 2.1 三种模式对比

```
┌─────────────────────────────────────────────────────────────────────┐
│                         MiniCode 执行模式全景                         │
├──────────────┬──────────────┬──────────────────┬────────────────────┤
│   维度       │  普通模式     │  /team 团队模式   │  /goal 目标模式     │
├──────────────┼──────────────┼──────────────────┼────────────────────┤
│ 触发方式      │ 默认         │ /team <task>     │ /goal <objective>  │
│ 执行时长      │ 分钟级       │ 几十分钟          │ 小时~十几小时       │
│ 自主性        │ 低(每轮等用户)│ 中(单轮自主)      │ 高(连续自主)        │
│ 架构          │ 单Agent      │ 主Agent+子Agent   │ GoalRunner+子Agent │
│ 上下文管理    │ 手动/自动压缩 │ 子Agent隔离       │ 主动管理+压缩+卸载  │
| 检查点        │ 无           │ 无               │ 有(自动保存)        │
│ 可恢复        │ /resume会话  │ 否               │ 是(从checkpoint)   │
│ 后台运行      │ 否           │ 否               │ 是(Daemon模式)     │
│ 自反思        │ 无           │ 汇总阶段简单反思   │ 每轮系统反思        │
│ Watchdog     │ 无           │ 重试机制          │ 停滞检测+死循环检测 │
│ 预算控制      │ 无           │ 子Agent数量限制   │ 多层预算+自动停止   │
│ Git集成      │ 无           │ 无               │ 自动commit里程碑   │
│ 用户介入      │ 每轮必须     │ 开始/结束         │ 可随时介入/审批     │
└──────────────┴──────────────┴──────────────────┴────────────────────┘
```

### 2.2 模式间的调用关系

```
/goal 模式内部可以调用：
  ├── 普通 Agent Loop（做具体的工具调用和推理）
  ├── /team 模式（当某个阶段需要多Agent协作时派发团队任务）
  └── 子 Agent（将重型探索/研究任务卸载给独立上下文的子Agent）

普通模式和 /team 模式不能调用 /goal（避免嵌套混乱）
```

### 2.3 用户心智模型

```
普通对话：
  用户: "帮我看看这个函数" → Agent: [执行一轮] → 返回结果 → 等待用户

/team 单次协作：
  用户: "/team 重构auth模块" → Agent编排→[子A1 子A2 并行]→[子A3测试]→[审查]→返回报告→等待用户

/goal 长时自主：
  用户: "/goal 将项目错误处理统一迁移到 anyhow，覆盖率达到80%，所有测试通过"
    → Agent: [制定计划] → [执行子任务1] → [反思调整] → [执行子任务2] →
      [检测到停滞→重新规划] → [派发子Agent做探索] → [执行子任务3] →
      [里程碑→git commit+checkpoint] → [继续] → ... →
      [目标达成/预算耗尽/遇到阻塞] → 通知用户
```

---

## 三、核心架构

### 3.1 架构总览

```
用户输入: /goal <objective>
    │
    ▼
┌──────────────────────────────────────────────────────────────────┐
│                     GoalRunner（目标运行器）                       │
│                                                                  │
│  ┌────────────┐  ┌─────────────┐  ┌──────────────┐              │
│  │ Goal状态机  │  │ 进度追踪器   │  │ 预算管理器    │              │
│  │ (核心驱动)  │  │ (Todo List) │  │ (Token/时间) │              │
│  └─────┬──────┘  └──────┬──────┘  └──────┬───────┘              │
│        │                │                │                       │
│  ┌─────▼──────┐  ┌──────▼──────┐  ┌──────▼───────┐              │
│  │ 反思引擎    │  │ Checkpoint  │  │ Watchdog     │              │
│  │ (Self-Ref) │  │ 管理器      │  │ (停滞检测)    │              │
│  └─────┬──────┘  └──────┬──────┘  └──────┬───────┘              │
│        │                │                │                       │
│        └────────────────┼────────────────┘                       │
│                         ▼                                        │
│              ┌─────────────────────┐                             │
│              │   TAOR 主循环        │                             │
│              │   Think→Act→Observe │                             │
│              │   →Repeat/Reflect   │                             │
│              └──────┬──────────────┘                             │
│                     │                                            │
│         ┌───────────┼────────────┐                              │
│         ▼           ▼            ▼                              │
│   ┌──────────┐ ┌──────────┐ ┌──────────┐                        │
│   │ 主Agent  │ │ 子Agent  │ │ /team    │                        │
│   │ (上下文) │ │ (隔离)   │ │ 派发     │                        │
│   └──────────┘ └──────────┘ └──────────┘                        │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │                    共享持久层                             │    │
│  │  .mini-code/goals/<goal-id>/                             │    │
│  │    ├── goal.json          (目标状态、进度、配置)           │    │
│  │    ├── plan.md            (当前执行计划)                  │    │
│  │    ├── progress.md        (进度日志)                      │    │
│  │    ├── checkpoints/       (检查点快照)                    │    │
│  │    └── sessions/          (Agent消息历史)                 │    │
│  └─────────────────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────────────────┘
```

### 3.2 新建 crate：`minicode-goal`

遵循项目"最小侵入"原则，新建独立 crate `minicode-goal`，不修改现有核心逻辑。

```
crates/minicode-goal/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── types.rs          # 核心数据结构
    ├── runner.rs         # GoalRunner - 核心运行器
    ├── loop_engine.rs    # TAOR 循环引擎
    ├── reflection.rs     # 自反思引擎
    ├── watchdog.rs       # 看门狗（停滞/死循环检测）
    ├── budget.rs         # 预算管理器（Token/时间/轮次）
    ├── checkpoint.rs     # 检查点管理（持久化/恢复）
    ├── progress.rs       # 进度追踪（Todo List 状态机）
    ├── planner.rs        # 初始计划生成与调整
    └── git_helper.rs     # Git 里程碑提交辅助
```

### 3.3 核心数据结构

```rust
// types.rs

/// Goal 的唯一标识
pub type GoalId = String;

/// Goal 运行状态
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GoalStatus {
    Planning,        // 正在制定初始计划
    Running,         // 正在执行
    Reflecting,      // 正在反思/调整计划
    Paused,          // 用户暂停
    AwaitingApproval,// 等待用户审批关键操作
    Blocked,         // 遇到阻塞，需要用户介入
    Completed,       // 目标达成
    Failed(String),  // 失败（附原因）
    Cancelled,       // 用户取消
    BudgetExceeded,  // 预算耗尽
}

/// Goal 配置（启动时设定）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalConfig {
    /// 目标描述（用户输入）
    pub objective: String,
    /// 成功标准（可选，由模型或用户定义）
    pub success_criteria: Option<Vec<String>>,
    /// Token 预算上限（美元或token数）
    pub max_budget_tokens: Option<usize>,
    /// 最大运行时长（秒），默认 4 小时 = 14400
    pub max_duration_secs: u64,
    /// 最大循环轮次，默认 200
    pub max_iterations: usize,
    /// 最大连续失败次数，默认 5
    pub max_consecutive_failures: usize,
    /// 权限模式：plan / default / acceptEdits / dontAsk
    pub permission_mode: PermissionMode,
    /// 是否自动 git commit 里程碑
    pub auto_git_commit: bool,
    /// 检查点间隔（轮次），默认每 5 轮一次
    pub checkpoint_interval: usize,
    /// 是否允许派发 /team 多Agent任务
    pub allow_team_dispatch: bool,
    /// 是否允许派发到后台Daemon
    pub allow_daemon: bool,
}

impl Default for GoalConfig {
    fn default() -> Self {
        Self {
            objective: String::new(),
            success_criteria: None,
            max_budget_tokens: None,
            max_duration_secs: 4 * 60 * 60,  // 4小时
            max_iterations: 200,
            max_consecutive_failures: 5,
            permission_mode: PermissionMode::Default,
            auto_git_commit: true,
            checkpoint_interval: 5,
            allow_team_dispatch: true,
            allow_daemon: false, // MVP 默认关闭daemon
        }
    }
}

/// 权限档位
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PermissionMode {
    Plan,         // 只读，完全不写入
    Default,      // 编辑和shell都需要询问
    AcceptEdits,  // 自动批准文件编辑，shell需询问
    DontAsk,      // 白名单内自动批准
}

/// Todo 项（进度追踪的核心单元）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: TodoStatus,
    pub depends_on: Vec<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Blocked { reason: String },
    Done,
    Skipped { reason: String },
}

/// 检查点快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub iteration: usize,
    pub timestamp: String,
    pub git_commit: Option<String>,
    pub todo_snapshot: Vec<TodoItem>,
    pub tokens_used: usize,
    pub summary: String,
}

/// Goal 运行时状态（完整可序列化）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalState {
    pub goal_id: GoalId,
    pub config: GoalConfig,
    pub status: GoalStatus,
    pub todos: Vec<TodoItem>,
    pub current_iteration: usize,
    pub tokens_used: usize,
    pub started_at: String,
    pub updated_at: String,
    pub last_checkpoint: Option<Checkpoint>,
    pub consecutive_failures: usize,
    /// 最近 N 轮的动作摘要（用于Watchdog检测重复）
    pub recent_actions: Vec<String>,
    /// 反思笔记（跨轮次保持的关键决策）
    pub reflection_notes: Vec<String>,
    /// 阻塞原因（当status==Blocked时）
    pub blocked_reason: Option<String>,
}
```

### 3.4 文件持久化布局

```
.mini-code/goals/
└── <goal-id>/                    # 每个goal一个目录（goal-id = goal-YYYYMMDD-HHMMSS）
    ├── goal.json                 # GoalState 完整序列化
    ├── plan.md                   # 当前执行计划（模型可读）
    ├── progress.md               # 进度日志（人可读）
    ├── reflections.md            # 累积的反思笔记
    ├── checkpoints/
    │   ├── cp-0001.json          # 第1个检查点
    │   ├── cp-0005.json          # 第5轮检查点
    │   └── ...
    └── messages/
        ├── messages.jsonl         # 主Agent消息历史（可用于恢复）
        └── subagent-messages/
            └── <subagent-id>.jsonl
```

---

## 四、核心执行循环

### 4.1 TAOR 循环设计

参考 Claude Code 的设计哲学：**运行时越笨，架构越稳定**。核心循环保持精简：

```rust
// loop_engine.rs - 核心循环伪代码

pub async fn run_goal_loop(runner: &mut GoalRunner) -> Result<GoalStatus> {
    // 1. 初始化：生成初始计划
    runner.initialize().await?;  // Think: 分析目标，创建TodoList
    runner.save_checkpoint().await?;

    loop {
        // 2. 预算检查（每层都有安全网）
        if runner.budget.exceeded() {
            runner.save_checkpoint().await?;
            runner.emit(GoalEvent::BudgetExceeded);
            return Ok(GoalStatus::BudgetExceeded);
        }

        // 3. Watchdog 检查（检测停滞/死循环）
        if runner.watchdog.detected_stall(&runner.state.recent_actions) {
            runner.emit(GoalEvent::StallDetected);
            // 触发强制反思
            let reflection = runner.reflect_and_replan(StallReason::RepetitiveActions).await?;
            if reflection.should_terminate {
                return Ok(GoalStatus::Failed("检测到无法突破的停滞".into()));
            }
            runner.state.recent_actions.clear();
            continue;
        }

        // 4. 获取当前待执行任务
        let next_task = match runner.pick_next_task() {
            Some(task) => task,
            None => {
                // 所有任务完成 → 最终验证
                if runner.verify_goal_completion().await? {
                    runner.finalize().await?;
                    return Ok(GoalStatus::Completed);
                } else {
                    // 验证不通过 → 新增修复任务继续
                    runner.add_verification_fix_tasks().await?;
                    continue;
                }
            }
        };

        // 5. Think：规划当前任务的执行步骤
        runner.emit(GoalEvent::TaskStart { task_id: next_task.id.clone() });
        let plan = runner.think_next_steps(&next_task).await?;

        // 6. Act：执行（调用工具/派生子Agent/使用/team）
        let result = runner.execute_step(&plan).await?;

        // 7. Observe：观察结果
        runner.state.tokens_used += result.tokens_used;
        runner.state.recent_actions.push(result.action_summary.clone());
        if runner.state.recent_actions.len() > 20 {
            runner.state.recent_actions.remove(0);
        }

        // 8. 处理执行结果
        match result.outcome {
            StepOutcome::Success { summary } => {
                runner.mark_task_done(&next_task.id, &summary).await?;
                runner.state.consecutive_failures = 0;
            }
            StepOutcome::ToolError { error } => {
                runner.state.consecutive_failures += 1;
                runner.note_failure(&next_task.id, &error).await?;
            }
            StepOutcome::NeedApproval { request } => {
                runner.state.status = GoalStatus::AwaitingApproval;
                let approved = runner.wait_for_approval(request).await?;
                if approved {
                    // 用户批准，重试
                    continue;
                } else {
                    runner.mark_task_blocked(&next_task.id, "用户拒绝了操作").await?;
                }
            }
            StepOutcome::NeedSubAgent { task_type, description } => {
                // 卸载给子Agent（保护主上下文）
                let sub_result = runner.dispatch_subagent(task_type, &description).await?;
                runner.incorporate_subagent_result(sub_result).await?;
            }
            StepOutcome::NeedTeam { description } => {
                // 派发/team任务
                runner.dispatch_team(&description).await?;
            }
        }

        // 9. 定期反思（每N轮或每次失败后）
        runner.state.current_iteration += 1;
        let should_reflect = runner.state.current_iteration % 3 == 0
            || matches!(result.outcome, StepOutcome::ToolError { .. })
            || runner.state.consecutive_failures > 0;

        if should_reflect {
            let reflection = runner.reflect_on_progress().await?;
            runner.apply_reflection(reflection).await?;
        }

        // 10. 定期检查点
        if runner.state.current_iteration % runner.config.checkpoint_interval == 0 {
            runner.save_checkpoint().await?;
            if runner.config.auto_git_commit {
                runner.git_commit_milestone().await?;
            }
        }
    }
}
```

### 4.2 Think 阶段：决定下一步做什么

核心是让模型基于当前状态自主决策，而不是硬编码流程：

```
Think 阶段的系统提示：
---
你正在自主执行一个长期目标。当前状态：

目标：{objective}
成功标准：{success_criteria}
当前进度：{completed}/{total} 个子任务
已用Token：{tokens_used}
最近操作：
{recent_actions}

待办事项：
{todo_list}

反思笔记：
{reflection_notes}

请决定下一步行动，必须选择以下之一：
1. 执行某个具体的Todo项（指定task_id，说明你要做什么）
2. 修改Todo列表（新增/拆分/删除/调整优先级某个任务）
3. 请求派发子Agent（说明需要子Agent做什么）
4. 请求派发团队任务（说明需要多Agent协作的复杂任务）
5. 标记目标完成（说明你认为已达成目标，给出证据）
6. 报告阻塞（说明遇到什么无法自主解决的问题）

严格按照JSON格式输出你的决策。
---
```

### 4.3 Act 阶段：执行动作

运行时根据模型的决策执行对应动作，执行完后返回结构化结果：

| 决策类型 | 执行方式 |
|---------|---------|
| 执行工具 | 调用现有工具注册表，经过权限检查 |
| 派生子Agent | 使用现有 `minicode-team` 的隔离执行引擎 |
| 派发团队任务 | 调用 TeamOrchestrator |
| 修改Todo | 更新 GoalState.todos |
| 标记完成 | 进入最终验证 |
| 报告阻塞 | 暂停执行，通知用户 |

### 4.4 Observe 阶段：记录结果

关键是将执行结果**结构化地**记录到状态中：
- 更新 TodoItem 状态
- 记录操作摘要到 `recent_actions`（用于Watchdog）
- 累计 token 使用量
- 记录错误信息（用于失败计数）

### 4.5 Reflect 阶段：自反思

每 3 轮或遇到失败后触发，是长时间运行的关键：

```
Reflect 阶段的系统提示：
---
请反思当前的执行进展：

目标：{objective}
已完成：{completed_todos}
进行中：{in_progress_todos}
遇到的问题：{recent_errors}
最近操作序列：{recent_actions}

请从以下维度反思：
1. 进度评估：整体进度如何？是否在正轨上？
2. 问题识别：是否在重复同样的错误？是否卡在某个地方？
3. 计划调整：Todo列表是否需要调整？是否有遗漏的任务？
4. 策略调整：当前执行策略是否有效？是否需要换个思路？
5. 上下文管理：是否有可以卸载给子Agent的重型任务？

输出JSON格式的反思结果，包含：
- assessment: "on_track" | "needs_adjustment" | "stalled"
- plan_adjustments: 对Todo列表的修改建议
- strategy_notes: 策略调整说明（存入reflection_notes）
- should_offload: 是否建议派生子Agent
- should_stop: 是否应该终止（说明原因）
---
```

---

## 五、关键子系统设计

### 5.1 Watchdog（看门狗）系统

长时间运行最常见的失败模式是**Agent陷入死循环**：反复做同样的操作、反复犯同样的错误、原地打转。

```rust
// watchdog.rs

pub struct Watchdog {
    /// 最近N次动作的相似度阈值
    similarity_threshold: f64,
    /// 连续无进展的轮次上限
    max_no_progress_rounds: usize,
    /// 连续相同错误上限
    max_repeated_errors: usize,
}

pub enum StallReason {
    RepetitiveActions,       // 连续重复相似动作
    NoProgress,              // 多轮没有Todo被标记为Done
    RepeatedErrors,          // 连续相同类型错误
    TokenSpike,              // Token消耗异常快
    EndlessToolLoop,         // 工具调用链过长无产出
}

impl Watchdog {
    /// 检测是否停滞
    pub fn detect_stall(&self, recent_actions: &[String], todos: &[TodoItem]) -> Option<StallReason> {
        // 1. 检测重复动作：最近5次动作相似度 > 80%
        if recent_actions.len() >= 5 {
            let window = &recent_actions[recent_actions.len()-5..];
            if self.are_actions_similar(window) {
                return Some(StallReason::RepetitiveActions);
            }
        }

        // 2. 检测无进展：最近10轮没有Todo完成
        // (由调用方传入已完成计数)

        // 3. 检测重复错误：最近3个错误类型相同

        // 4. 检测异常Token消耗

        None
    }

    fn are_actions_similar(&self, actions: &[String]) -> bool {
        // 简单实现：比较动作的关键词重叠度
        // 进阶实现：可以用embedding相似度
        let mut all_words = std::collections::HashSet::new();
        let mut common_words = std::collections::HashSet::new();

        for (i, action) in actions.iter().enumerate() {
            let words: std::collections::HashSet<_> = action
                .split_whitespace()
                .map(|w| w.to_lowercase())
                .collect();
            if i == 0 {
                common_words = words.clone();
            } else {
                common_words = common_words.intersection(&words).cloned().collect();
            }
            all_words.extend(words);
        }

        if all_words.is_empty() { return false; }
        let similarity = common_words.len() as f64 / all_words.len() as f64;
        similarity > self.similarity_threshold
    }
}
```

检测到停滞后的处理策略：

| 停滞类型 | 第一次触发 | 第二次触发 | 第三次触发 |
|---------|-----------|-----------|-----------|
| 重复动作 | 强制反思+换策略 | 派生子Agent探索 | 暂停等待用户 |
| 无进展 | 强制反思+重规划 | 回滚到上个检查点 | 暂停等待用户 |
| 重复错误 | 反思错误原因 | 尝试完全不同的方法 | 暂停等待用户 |

### 5.2 Budget（预算）系统

多层安全网防止无限烧钱：

```rust
// budget.rs

pub struct BudgetManager {
    pub config: BudgetConfig,
    pub tokens_used: usize,
    pub start_time: std::time::Instant,
    pub iterations: usize,
    pub consecutive_failures: usize,
}

pub struct BudgetConfig {
    pub max_tokens: Option<usize>,     // None = 不限制（但有其他上限兜底）
    pub max_duration: std::time::Duration,
    pub max_iterations: usize,
    pub max_consecutive_failures: usize,
    pub max_cost_usd: Option<f64>,    // 美元成本上限
}

impl BudgetManager {
    pub fn exceeded(&self) -> bool {
        // 多层检查，任一层触顶即停止
        if self.iterations >= self.config.max_iterations {
            return true;
        }
        if self.start_time.elapsed() >= self.config.max_duration {
            return true;
        }
        if self.consecutive_failures >= self.config.max_consecutive_failures {
            return true;
        }
        if let Some(max_tok) = self.config.max_tokens {
            if self.tokens_used >= max_tok {
                return true;
            }
        }
        false
    }

    pub fn remaining_warning(&self) -> Option<&'static str> {
        // 使用量达80%时发出警告
        if self.iterations as f64 / self.config.max_iterations as f64 > 0.8 {
            return Some("接近轮次上限");
        }
        // ... 其他维度的警告
        None
    }
}
```

默认预算配置：

| 维度 | 默认值 | 说明 |
|------|-------|------|
| 最大轮次 | 200 | 每轮是一次Think-Act-Observe |
| 最大时长 | 4小时 | 防止意外运行过夜 |
| 最大连续失败 | 5次 | 连续失败5次暂停 |
| Token上限 | 不设硬限制 | 但其他三层兜底 |

用户可通过 `/goal` 参数自定义，如：
```
/goal --max-hours 8 --max-iters 500 实现完整的博客系统
```

### 5.3 Checkpoint（检查点）系统

支持持久化和恢复的关键：

```rust
// checkpoint.rs

impl GoalRunner {
    /// 保存检查点
    pub async fn save_checkpoint(&mut self) -> Result<()> {
        let cp = Checkpoint {
            id: format!("cp-{:04}", self.state.current_iteration),
            iteration: self.state.current_iteration,
            timestamp: Utc::now().to_rfc3339(),
            git_commit: self.get_current_git_commit().await?,
            todo_snapshot: self.state.todos.clone(),
            tokens_used: self.state.tokens_used,
            summary: self.generate_checkpoint_summary().await?,
        };

        // 序列化到文件
        let cp_path = self.goal_dir.join("checkpoints").join(format!("{}.json", cp.id));
        std::fs::create_dir_all(cp_path.parent().unwrap())?;
        std::fs::write(&cp_path, serde_json::to_string_pretty(&cp)?)?;

        // 同时更新 goal.json（最新状态）
        self.state.updated_at = Utc::now().to_rfc3339();
        self.state.last_checkpoint = Some(cp);
        let goal_path = self.goal_dir.join("goal.json");
        std::fs::write(&goal_path, serde_json::to_string_pretty(&self.state)?)?;

        // 写入人类可读的进度文件
        self.write_progress_md().await?;

        Ok(())
    }

    /// 从检查点恢复
    pub async fn resume_from_checkpoint(goal_id: &str) -> Result<Self> {
        let goal_dir = goals_dir().join(goal_id);
        let state: GoalState = serde_json::from_str(
            &std::fs::read_to_string(goal_dir.join("goal.json"))?
        )?;

        // 加载消息历史
        let messages = load_messages_from_dir(&goal_dir.join("messages"))?;

        Ok(Self {
            state,
            messages,
            goal_dir,
            // ... 其他字段初始化
        })
    }

    /// Git 里程碑提交
    pub async fn git_commit_milestone(&self) -> Result<()> {
        let message = format!(
            "[minicode-goal] 里程碑: 迭代 {}, 完成 {}/{} 任务\n\n{}",
            self.state.current_iteration,
            self.completed_todos().len(),
            self.state.todos.len(),
            self.last_checkpoint.as_ref().map(|cp| cp.summary.clone()).unwrap_or_default()
        );
        // 调用 git add -A && git commit
        run_git_commit(&message).await?;
        Ok(())
    }
}
```

检查点保存时机：
1. 每 N 轮（默认5轮）
2. 每个 TodoItem 标记为 Done 后
3. 反思后有重大计划调整时
4. 预算达到80%警告线时
5. 用户手动暂停时

### 5.4 Progress（进度追踪）系统

基于 Todo List 的状态机，参考 Claude Code 的 Task 系统：

```
Todo 状态转换：
  Pending ──→ InProgress ──→ Done
     │            │
     │            └──→ Blocked {reason} ──→ InProgress（解除阻塞后）
     │
     └──→ Skipped {reason}（反思后决定跳过）
```

关键设计：
- Todo 列表本身是**模型可读写的**——模型在 Think 和 Reflect 阶段可以增删改 Todo
- 运行时不硬编码任务，只维护状态机的合法性
- Todo 的粒度由模型决定（太粗就拆分，太细就合并）
- progress.md 人类可读文件方便用户随时查看进度

人类可读 progress.md 示例：

```markdown
# Goal: 将项目错误处理统一迁移到 anyhow

**状态**: Running | **轮次**: 23/200 | **用时**: 45m/4h
**进度**: 7/12 完成 | **Token**: ~32k

## 已完成
- [x] T1: 分析项目中所有错误处理模式 (迭代3)
- [x] T2: 制定迁移计划 (迭代5)
- [x] T3: 迁移 src/auth/ 模块 (迭代12)
- [x] T4: 迁移 src/api/ 模块 (迭代18)
- [x] T5: cargo check 验证 (迭代19)
- [x] T7: 更新文档中的示例 (迭代22)

## 进行中
- [~] T6: 迁移 src/utils/ 模块 (迭代20开始，正在处理)

## 待处理
- [ ] T8: 为迁移后的代码补充单元测试
- [ ] T9: cargo clippy 检查
- [ ] T10: 运行完整测试套件

## 阻塞
(无)

## 最近反思
- 发现 src/utils/ 有3处自定义错误类型被外部依赖使用，需要特殊处理
- 策略调整：先保持这些类型的兼容层，后续独立PR移除
```

### 5.5 上下文管理策略

长时间运行最大的技术挑战是 **Context Collapse**（上下文窗口被填满导致模型退化）。采用多层防御。

> **重要：不需要修改现有压缩算法。** 现有 [compact.rs](file:///Users/scm/code/MiniCode-rs/crates/minicode-agent-core/src/compact.rs) 已经支持参数化调用（自定义阈值、保留条数），且 System 消息被无条件保留（不会被压缩掉）。我们利用这两个特性来保护 Goal 状态，不修改任何现有模式的行为。

#### 第零层：Goal 状态作为 System 消息注入（根本保证）

这是最重要的一层，也是 Todo 等结构化状态"不会丢"的核心机制。

Todo 列表、反思笔记、目标描述**不存放在普通消息历史里**，而是存放在 `GoalState` 结构体中（持久化在 `goal.json`）。在每轮 Think 之前，将最新的 Goal 状态序列化为一条 **System 消息**，动态注入到消息列表最前面：

```
每轮循环开始时构造的消息列表：
┌───────────────────────────────────────────────────────────┐
│ System: 基础系统提示词（工具说明、编码规范等）                │ ← 原有，永久保留
│ System: [Goal 状态注入]  ← 每轮更新，永远在最前              │ ← 关键！
│   ## Goal                                                 │
│   目标：将项目错误处理统一迁移到 anyhow                      │
│   成功标准：cargo check通过，所有测试通过，无clippy警告       │
│                                                           │
│   ## 进度 (7/12)                                          │
│   [x] T1 分析项目错误处理模式                               │
│   [x] T2 制定迁移计划                                      │
│   [x] T3 迁移 src/auth/ 模块                               │
│   [~] T6 迁移 src/utils/ 模块 (进行中)                      │
│   [ ] T8 补充单元测试                                      │
│   ...                                                     │
│                                                           │
│   ## 反思笔记                                              │
│   - src/utils/ 中CompatError被外部依赖使用，需保留兼容层     │
│   - 发现3种错误处理模式，已处理2种                          │
│                                                           │
│ ContextSummary: "早期对话摘要..."     ← 压缩产生              │
│ ... 最近10条普通消息 ...               ← 压缩保留              │
└───────────────────────────────────────────────────────────┘
```

**为什么这条消息不会被压缩掉？** 看 [compact.rs:136-139](file:///Users/scm/code/MiniCode-rs/crates/minicode-agent-core/src/compact.rs#L136-L139)：压缩时先把所有 `ChatMessage::System` 过滤出来单独保留，它们永远不会进入"待压缩"池。所以无论压缩多少次，Goal 状态始终在上下文中。

每轮循环我们都用 GoalState 的最新内容**替换**这条 System 消息（而不是追加），保证：
- Todo 状态永远是最新的
- 已完成的任务不会重复出现在 System 消息里浪费 token
- 反思笔记只保留关键的几条（比如最近5条），持续淘汰过时的

可选优化：给 `ChatMessage` 枚举加一个 `GoalState { content: String }` 变体，语义更明确，在 compact.rs 的过滤条件里也保留它。但用 System 消息已经能工作，这不是必须的。

#### 第一层：渐进式压缩管线（Progressive Compaction Pipeline）

项目已有**三级渐进式压缩**（见 [agent_loop.rs:108-152](file:///Users/scm/code/MiniCode-rs/crates/minicode-agent-core/src/agent_loop.rs#L108-L152)），按代价从低到高依次触发：

```
上下文利用率增长 →
0% ────────── 30% ────────── 50% ────────── 70% ────────── ~95%（溢出崩溃）
  (Goal模式)     (普通micro)    (普通snip)     (普通auto)
     │              │              │              │
     ├─ microcompact 启动 ─┤      │              │
     │              ├─ snip_compact 启动 ─┤      │
     │              │              ├─ auto_compact 启动 ─┤
     │              │              │              │
  Goal模式三级更早触发，保持上下文始终"干净"
```

| 压缩级别 | 机制 | 代价 | 普通模式触发点 | Goal 模式触发点 | 做什么 |
|---------|------|------|--------------|----------------|--------|
| **microcompact** | 纯规则，不调模型 | 近乎零 | ≥50% 利用率 | **≥30% 利用率** | 清空旧的 read_file/list_files/grep_files 等只读工具结果，保留最近3条，替换为 `[Content cleared]` |
| **snip_compact** | 纯规则，不调模型 | 低（删消息段） | ≥70% 利用率 | **≥50% 利用率** | 找中间"安全区间"（不含文件编辑、不含错误消息）整段删除，插入 SnipBoundary 标记 |
| **auto_compact** | 调模型生成摘要 | 高（一次API调用） | 128k tokens (~50%) | **65k tokens (~25%)** | 把早期消息交给模型摘要为 ContextSummary，保留最近10条 |

**为什么要更早触发？三个关键原因：**

1. **长时运行不能承受溢出**：普通对话上下文溢出只是当前轮截断，用户可以重试；Goal 模式运行几小时如果溢出崩溃，可能丢失大量进度。必须留足安全余量。

2. **避免"压缩雪崩"**：如果等到 70% 才压缩，压缩本身的输出（摘要文本、边界标记）也占 token，压缩后 Agent 因信息丢失需要重新 read_file/grep，新结果又迅速填满，形成"压缩→重新探索→再填满→再压缩"的雪崩循环，急剧消耗 token。小步渐进地在 30% 开始 microcompact，每次只清很少内容，Agent 还"记得"最近的文件内容不需要重读，效率最高。

3. **Goal 模式有结构化记忆兜底**：Todo 列表+反思笔记以 System 消息永久存在，不怕早期消息被压缩。Agent 不需要依赖完整消息历史来"记住"做过什么。

**实现方式**：三级压缩函数本身都不需要修改。Goal 模式在调用 agent turn 时传入不同的利用率计算/阈值参数即可。microcompact 和 snip_compact 目前硬编码了阈值常量（[microcompact.rs:4](file:///Users/scm/code/MiniCode-rs/crates/minicode-agent-core/src/microcompact.rs#L4) MICROCOMPACT_UTILIZATION=0.50，[snip_compact.rs:4](file:///Users/scm/code/MiniCode-rs/crates/minicode-agent-core/src/snip_compact.rs#L4) SNIP_COMPACT_THRESHOLD=0.70），实现时有两个选择：
- 方案A（推荐）：为 microcompact/snip_compact 添加阈值参数（类似 auto_compact 已有的 threshold_tokens 参数），保持默认值不变，Goal 模式传参覆盖
- 方案B（快速）：Goal 模式在自己的循环中提前调用这些压缩函数（传自己的利用率计算）

MVP 阶段推荐方案A，改动极小：只是给两个纯函数加个 Option<f64> 参数。

**理想效果**：因为 microcompact 持续在 30% 就清理旧工具结果，理想情况下上下文根本不会涨到 50%，snip_compact 和 auto_compact 几乎不会被触发——这就是最理想的状态：**用最便宜的方式持续维护上下文健康。**

#### 第二层：子 Agent 卸载（Sub-agent Offloading）

将消耗大量 Token 的探索性、研究性任务派发给独立上下文的子 Agent：
- 代码库探索（"找出所有使用了 ? 运算符的地方"）
- 大型文件阅读和理解
- 测试编写和运行
- 代码审查

子 Agent 在自己的隔离上下文中运行（见 [isolated_agent.rs](file:///Users/scm/code/MiniCode-rs/crates/minicode-team/src/isolated_agent.rs)），完成后只返回**结构化摘要**（通常几百 tokens）到主上下文，不会污染主对话。这本质上是"用计算换上下文空间"。

#### 第三层：记忆是索引，不是存储

不依赖完整消息历史来"记住"之前做了什么，而是通过外部状态来承载记忆：
- **Todo 列表**（GoalState.todos）：精确的进度状态，存放在 System 消息里
- **Reflection Notes**（GoalState.reflection_notes）：关键决策和经验教训
- **progress.md**（文件系统）：人类可读的完整进度日志，模型需要时可以 read_file 查阅
- **代码库本身**：所有已做的修改都在文件里，模型可以随时 grep/read 重新获取

核心原则（参考 Claude Code 源码设计哲学）：**能从代码库和文件系统重新推导的信息，不需要保存在上下文中。** 上下文只放"正在进行中的工作状态"。

#### 第四层：里程碑摘要替换

在检查点（每5轮）时，除了持久化状态，还可以主动将当前上下文中除了 Goal State System 消息和最近几条消息之外的内容，通过一次 summarize_conversation 调用替换为里程碑摘要：

```
[里程碑 #4 迭代15-20]
- 完成了 T6 迁移 src/utils/（保持 CompatError 兼容层）
- 完成了 T7 更新文档示例
- cargo check 0 warning 0 error
- 遇到的问题：CompatError 的 Display trait 实现需要保持向后兼容
- 下一步：T8 补充单元测试
```

这比等自动压缩触发更可控，能保证摘要质量。实现上可以在 checkpoint 时主动调用一次 compact，而不是等到阈值触发。

### 5.6 子 Agent 与 /team 派发

Goal 模式可以在执行过程中自主决定使用子 Agent 或团队模式：

```
Think阶段模型决策：
"这个任务需要同时修改5个独立模块，而且后续还要写测试和审查，
 应该派发/team任务让多Agent协作完成"
```

派发机制：
- **子Agent派发**：复用 `minicode-team/src/isolated_agent.rs` 的隔离执行
- **/team派发**：复用 `TeamOrchestrator`，Goal 模式作为 super-orchestrator
- 派发出去的任务在独立上下文中执行，结果摘要返回主循环

### 5.7 权限模式

Goal 模式有独立的权限档位，因为长时运行不可能每次都问用户：

| 档位 | 文件编辑 | Shell命令 | 网络请求 | 适用场景 |
|------|---------|----------|---------|---------|
| `--plan` | ❌ 禁止 | ❌ 禁止 | ❌ 禁止 | 只做规划，不做任何修改 |
| `default` | ⚠️ 每次询问 | ⚠️ 每次询问 | ⚠️ 每次询问 | 高度谨慎（默认） |
| `--yolo` (acceptEdits) | ✅ 自动批准 | ⚠️ 危险命令询问 | ⚠️ 询问 | 信任编辑，盯着shell |
| `--auto` (dontAsk) | ✅ 自动批准 | ✅ 白名单自动 | ⚠️ 询问 | 完全信任（个人项目） |

危险命令（任何模式都需审批）：
- `rm -rf`、`git push --force`、`dropdb`、`sudo`、`chmod 777` 等
- 网络请求到未知域名
- 修改 `.env`、密钥文件、配置文件中的敏感字段

---

## 六、TUI 集成

### 6.1 命令入口

```
/goal <objective>              启动goal模式（使用默认配置）
/goal --yolo <objective>       使用自动批准编辑模式
/goal --plan <objective>       仅规划不执行
/goal --max-hours 8 <obj>      自定义最大时长
/goal --resume <goal-id>       恢复一个中断/暂停的goal
/goal --list                   列出所有goal及其状态
/goal --stop                   停止当前goal
/goal --pause                  暂停当前goal
/goal --status                 显示当前goal详细状态
```

### 6.2 Goal 模式 TUI 界面

```
┌─ Session ──────────────────────────────────────────────────────────────┐
│ 🎯 Goal: 将项目错误处理统一迁移到 anyhow                                 │
│ 状态: Running | 轮次: 23/200 | 用时: 45:23 / 4:00:00 | Token: ~32k     │
│ 进度: ████████████░░░░░░░░ 7/12 (58%)                                  │
├────────────────────────────────────────────────────────────────────────┤
│                                                                        │
│  [主Agent] 正在修改 src/utils/errors.rs...                             │
│  $ cargo check  (通过，0 warning)                                       │
│                                                                        │
│  ─── 最近里程碑 ───                                                    │
│  ✅ T5: cargo check 验证通过 (迭代19)                                   │
│  ✅ T4: 迁移 src/api/ 模块 (迭代18)                                     │
│  ✅ T3: 迁移 src/auth/ 模块 (迭代12)                                    │
│                                                                        │
│  ─── 进行中 ───                                                       │
│  🔄 T6: 迁移 src/utils/ 模块...                                        │
│                                                                        │
│  ─── Todo 列表 ───                                                    │
│  [x] T1 分析项目错误处理模式                                            │
│  [x] T2 制定迁移计划                                                   │
│  [x] T3 迁移 src/auth/                                                 │
│  [x] T4 迁移 src/api/                                                  │
│  [x] T5 cargo check 验证                                               │
│  [~] T6 迁移 src/utils/  (3/5 文件)                                    │
│  [ ] T7 更新文档示例                                                   │
│  [ ] T8 补充单元测试                                                   │
│  [ ] T9 cargo clippy                                                   │
│  [ ] T10 运行完整测试套件                                              │
│                                                                        │
├────────────────────────────────────────────────────────────────────────┤
│ Input │ mini-code>                                                     │
│ status: 🎯 Goal Running | T6 in progress | ✅7 🔄1 ⏳5 | Ctrl+C暂停   │
└────────────────────────────────────────────────────────────────────────┘
```

### 6.3 TUI 状态扩展

在 `ScreenState` 中新增：

```rust
pub(crate) struct ScreenState {
    // ... 现有字段 ...

    /// Goal 模式状态
    pub(crate) goal_state: Option<GoalState>,
    /// Goal 运行器（运行时持有）
    pub(crate) goal_runner: Option<GoalRunnerHandle>,
    /// 是否在goal模式中
    pub(crate) goal_mode_active: bool,
}
```

新增 TurnEvent 类型：
```rust
pub(crate) enum TurnEvent {
    // ... 现有 ...
    GoalStart { goal_id: String, objective: String, total_todos: usize },
    GoalProgress { iteration: usize, total: usize, message: String },
    GoalTodoUpdated { todos: Vec<TodoItem> },
    GoalTaskStart { task_id: String, title: String },
    GoalTaskComplete { task_id: String, success: bool, summary: String },
    GoalReflection { summary: String },
    GoalCheckpoint { iteration: usize },
    GoalStallDetected { reason: String },
    GoalAwaitingApproval { request: String },
    GoalMilestone { git_commit: Option<String>, message: String },
    GoalComplete { summary: String },
    GoalFailed { reason: String },
    GoalPaused { reason: String },
    GoalBudgetWarning { warning: String },
}
```

### 6.4 Daemon 后台模式（MVP后版本）

未来版本支持 detach 到后台：
1. 用户按 Ctrl+D 或输入 `/goal --detach` 将当前 goal 移到后台
2. TUI 退出，goal 在后台 daemon 进程中继续运行
3. 用户随时重新打开 TUI，执行 `/goal --resume` 重新attach
4. 里程碑/阻塞/完成时可发送桌面通知
5. （远期）支持手机端远程查看进度和审批

MVP **不实现** daemon 模式，但在架构上预留接口。

---

## 七、错误处理与安全网

### 7.1 失败模式与应对

| 失败模式 | 检测方式 | 应对策略 |
|---------|---------|---------|
| **Context Collapse** | 上下文长度>80%、回复开始退化 | 自动压缩、子Agent卸载、里程碑摘要替换 |
| **死循环** | Watchdog检测重复动作 | 强制反思→换策略→派生子Agent→暂停等用户 |
| **工具调用错误** | ToolResult.is_error | 记录错误→反思原因→调整后重试(最多5次) |
| **编译错误持续** | cargo check连续失败3次 | 反思错误→尝试不同修复方式→实在不行暂停 |
| **测试不通过** | cargo test失败 | 分析失败原因→修复→重试(纳入Todo) |
| **API错误/网络中断** | 请求异常 | 指数退避重试(最多10次)→持久化状态→退出等待恢复 |
| **预算耗尽** | BudgetManager | 保存检查点→git commit→通知用户→优雅停止 |
| **TUI崩溃** | 信号处理 | SIGINT/SIGTERM时保存检查点 |
| **系统重启** | 检查点持久化 | 重启后 /goal --resume 恢复 |

### 7.2 优雅停止流程

```
收到停止信号（用户Ctrl+C或预算耗尽）
    │
    ├─ 1. 完成当前正在执行的工具调用（不要中途打断工具操作）
    ├─ 2. 保存检查点（goal.json + messages）
    ├─ 3. 如果auto_git_commit，提交一个WIP commit
    ├─ 4. 更新goal状态为Paused/Stopped
    ├─ 5. 写入最终progress.md
    └─ 6. 通知用户已安全停止，显示如何恢复
```

### 7.3 信号处理

需要处理以下OS信号：
- **SIGINT (Ctrl+C)**：第一次按 → 暂停Goal（保存检查点）；第二次按 → 强制退出
- **SIGTERM**：优雅停止（同停止流程）
- **SIGHUP**：（daemon模式下）忽略，继续运行
- 窗口Resize：已由现有TUI处理

---

## 八、与现有模块的集成

### 8.1 复用的模块

| 现有模块 | Goal 模式中的使用 |
|---------|-----------------|
| `minicode-tool` 工具注册表 | 主Agent和子Agent都通过工具注册表调用工具 |
| `minicode-agent-core` Agent Loop | 作为TAOR中Act阶段的执行核心 |
| `minicode-team` 子Agent隔离 | 派生子Agent和团队任务 |
| `minicode-history` 消息持久化 | Goal消息独立存储在goal目录中 |
| `minicode-permissions` 权限系统 | Goal模式扩展权限档位 |
| `minicode-config` 配置 | Goal默认配置从settings读取 |
| `minicode-context` / 压缩 | 更积极的自动压缩 |
| `minicode-tui` TUI | 扩展TurnEvent和界面 |
| `minicode-cli-commands` Slash命令 | 注册/goal系列命令 |

### 8.2 需要新增/修改的模块

| 模块 | 类型 | 说明 |
|------|------|------|
| `crates/minicode-goal/` | **新增crate** | Goal模式核心实现 |
| `crates/minicode-tui/src/state.rs` | 修改 | 添加goal相关状态字段和TurnEvent |
| `crates/minicode-tui/src/turn/mod.rs` | 修改 | 添加/goal命令处理和事件循环集成 |
| `crates/minicode-tui/src/turn/event_apply.rs` | 修改 | 处理Goal事件 |
| `crates/minicode-cli-commands/src/lib.rs` | 修改 | 注册/goal系列slash命令 |
| `Cargo.toml` (workspace) | 修改 | 注册minicode-goal crate |
| `apps/minicode/` | 修改 | 如果需要daemon模式，可能需要调整二进制入口 |

### 8.3 不修改的模块（保持兼容性）

| 模块 | 原因 |
|------|------|
| `minicode-agent-core/agent_loop.rs` | Goal模式有自己的外层循环，内部复用run_agent_turn |
| `minicode-team/` | 通过调用TeamOrchestrator和run_isolated_subagent复用，不修改 |
| `minicode-tool/` | 不需要新增工具，Goal是编排层而非工具层 |
| 普通模式的所有逻辑 | Goal是新模式，不影响默认交互 |

---

## 九、实现路线图

### 阶段一：MVP（~2周）

**目标**：跑通核心TAOR循环，支持基本的长时自主运行

**功能清单**：
- [ ] 新建 `minicode-goal` crate
- [ ] 核心数据结构（GoalState, GoalConfig, TodoItem, Checkpoint）
- [ ] GoalRunner 框架和文件持久化（.mini-code/goals/）
- [ ] 简化版 TAOR 循环（Think→Act→Observe，暂不包含Reflect）
- [ ] 基础Todo管理（模型可读写Todo列表）
- [ ] 基础预算控制（轮次+时间限制）
- [ ] 基础检查点（每5轮自动保存，SIGINT时保存）
- [ ] `/goal <obj>` 命令启动
- [ ] `/goal --stop` `/goal --status` `/goal --list` 命令
- [ ] TUI基础展示（进度、当前任务、状态）
- [ ] 简单的自动压缩（复用现有compaction，阈值30%）

**MVP限制（明确告知用户）**：
- 不支持子Agent/team派发（单Agent运行）
- 不支持daemon后台模式（关闭TUI即暂停）
- 不支持复杂的Watchdog（只有轮次/时间/失败次数限制）
- 不支持恢复（`/goal --resume` 预留但未完全实现）
- 权限模式只有default（每次询问）

### 阶段二：核心能力完善（~2周）

**目标**：加入反思、Watchdog、子Agent卸载、恢复能力

**功能清单**：
- [ ] Reflect自反思引擎（每3轮+失败后触发）
- [ ] Watchdog停滞检测（重复动作/无进展检测）
- [ ] 子Agent派发能力（将重型任务卸载给隔离Agent）
- [ ] `/goal --resume <goal-id>` 从检查点恢复
- [ ] `/goal --pause` 暂停（可resume）
- [ ] Git里程碑自动提交
- [ ] 上下文多层管理（压缩+摘要替换+子Agent卸载）
- [ ] 权限档位支持（--yolo, --plan, --auto）
- [ ] `/goal:continue` 在普通对话中继续之前的goal
- [ ] 完整的人类可读progress.md输出
- [ ] TUI完整的Todo列表展示

### 阶段三：高级特性（~2周）

**目标**：团队派发、Daemon后台模式、体验优化

**功能清单**：
- [ ] `/team` 派发能力（Goal内自主决定使用多Agent协作）
- [ ] Daemon后台模式（Ctrl+D detach，后台继续运行）
- [ ] 桌面通知（里程碑/阻塞/完成时通知用户）
- [ ] 成本估算和美元预算限制
- [ ] 回滚功能（回滚到任意检查点 + git reset）
- [ ] Goal 历史列表和管理（`/goal --list` 详情）
- [ ] 自定义Goal模板（用户预定义常用goal配置）
- [ ] 阻塞时智能建议（告诉用户可以怎么帮它）

### 阶段四：生态扩展（持续迭代）

**目标**：与外部工具集成、协作功能

**功能清单**：
- [ ] 手机端远程监控（Codex Remote风格）
- [ ] GitHub Integration（基于PR/Issue自动创建goal）
- [ ] Goal 分享/导出
- [ ] 多Goal并行
- [ ] 自定义系统提示词扩展
- [ ] 统计面板（token消耗、成功率、平均时长等）

---

## 十、系统提示词设计

### 10.1 Goal 模式系统提示词（外层循环）

Goal 模式的系统提示词不是给单次turn用的，而是给整个Goal运行过程用的，结构如下：

```
你是一个自主AI编程助手，正在执行一个长期目标。

# 你的角色
你不是在聊天，你在执行一个需要多轮才能完成的工程目标。
每一轮你都要自主决定下一步做什么，推进目标向前。

# 目标
{objective}

# 成功标准
{success_criteria}
当你认为目标达成时，需要提供证据（测试通过、命令输出、文件修改列表）。

# 当前状态
- 已执行轮次：{iteration}/{max_iterations}
- Todo进度：{completed}/{total}
- 已用Token：{tokens_used}
- 连续失败：{consecutive_failures}

# 待办事项
{todo_list_markdown}

# 反思笔记
{reflection_notes}

# 工作规则
1. 每次回复必须是JSON格式，包含"action"字段
2. action类型：execute_task / update_todos / spawn_subagent / spawn_team / complete / block
3. 不要重复已经完成的任务
4. 遇到连续失败时，必须换一个完全不同的方法
5. 修改代码后必须运行cargo check验证
6. 不要做目标之外的事情
7. 如果确实无法继续（缺少信息/权限/遇到矛盾），使用block报告阻塞
8. 每完成一个子任务，更新todo状态
9. 保持todo粒度适中：一个todo应该在1-3轮内可完成
10. 大任务要主动拆分为小todo

# 上下文管理
- 如果发现自己在做大量文件探索/阅读，考虑spawn_subagent
- 如果任务涉及多个独立模块并行修改，考虑spawn_team
- 不要在上下文中保存大段代码，用文件作为存储
```

### 10.2 Reflect 反思提示词

见第四章 4.5 节。

### 10.3 初始规划提示词

Goal 启动时的第一轮，用于生成初始Todo列表：

```
用户给你一个长期目标，请分析并制定执行计划。

目标：{objective}

请执行以下步骤：
1. 先用只读工具快速扫描项目结构（list_files, read_file关键文件）
2. 理解项目的技术栈、代码组织、现有模式
3. 制定一个分步骤的执行计划，将目标分解为具体的Todo项
4. 每个Todo应该是可在1-3轮内完成的具体任务
5. 设置合理的依赖关系
6. 定义成功标准（怎么算目标达成）

输出JSON格式的初始计划。
```

---

## 十一、成本估算与控制

### 11.1 Token 成本估算

基于 Claude Code 的实测数据和单轮token消耗：

| Goal 类型 | 预计轮次 | 预计Token | 预计成本（Sonnet） | 预计耗时 |
|----------|---------|----------|------------------|---------|
| 中模块重构 | 50-80轮 | 200k-500k | $0.6-$1.5 | 1-2小时 |
| 完整功能开发 | 100-150轮 | 500k-1.5M | $1.5-$4.5 | 2-4小时 |
| 大型重构/迁移 | 150-200轮 | 1M-3M | $3-$9 | 4-8小时 |
| 全项目测试覆盖 | 80-120轮 | 400k-1M | $1.2-$3 | 2-3小时 |

### 11.2 成本控制措施

1. **明确预算**：用户启动时可设 `--max-cost` 美元上限
2. **80%警告线**：达到预算80%时保存检查点并通知用户
3. **子Agent使用小模型**：探索/研究类子Agent可使用Haiku级模型（速度快、成本低）
4. **Prompt缓存优化**：系统提示词+Goal状态保持稳定，利用prompt caching
5. **避免冗余工具调用**：Watchdog检测重复无效操作
6. **渐进式自动批准**：`--yolo` 模式减少审批等待，加快执行

---

## 十二、风险与应对

| 风险 | 严重程度 | 应对策略 |
|------|---------|---------|
| Token成本失控 | 高 | 多层预算+80%警告+默认4小时上限+保守默认值 |
| 长时运行质量下降 | 高 | 自反思+Watchdog+上下文压缩+检查点 |
| 自主决策出错 | 中 | 权限档位+关键操作审批+git commit里程碑可回滚 |
| 实现复杂度高 | 高 | 分4阶段MVP渐进式实现 |
| TUI响应性 | 中 | Goal在tokio task中运行，事件通道通信 |
| API限流/网络错误 | 中 | 指数退避重试+检查点保存+resume恢复 |
| 上下文溢出 | 高 | 30%阈值压缩+子Agent卸载+里程碑摘要 |
| Daemon模式进程管理 | 中 | MVP不实现daemon，后续用pid文件+信号处理 |
| 用户对自主执行不信任 | 中 | 默认高权限档位需要审批+进度透明显示+随时可干预 |
| 与现有模式冲突 | 低 | 独立crate+独立事件+不修改核心loop |

---

## 十三、总结

`/goal` 长时自主执行模式是 MiniCode 从"交互式助手"向"自主工程Agent"演进的关键功能。核心设计借鉴了 Claude Code TAOR Loop、Harness 工程化、KAIROS 常驻Agent 等先进理念，同时结合 MiniCode 现有架构进行最小侵入式扩展。

**核心设计哲学**：
1. **运行时"愚笨"，模型智能** — 核心循环精简，决策交给LLM
2. **多层安全网** — 预算+Watchdog+检查点+权限+审批，层层叠加
3. **检查点优先** — 状态可持久化、可恢复、可回滚
4. **主动管理上下文** — 压缩、卸载、摘要，防止Context Collapse
5. **自反思闭环** — Think-Act-Observe-Reflect 持续改进
6. **渐进式实现** — MVP先跑通核心循环，逐步增加高级特性

**与/team模式的关系**：
- `/team` 是**单次多Agent协作**：一次性编排→执行→返回
- `/goal` 是**持续自主运行**：连续TAOR循环，可自主派发/team和子Agent
- `/goal` 是更高层级的抽象，可以在内部调用 `/team`
