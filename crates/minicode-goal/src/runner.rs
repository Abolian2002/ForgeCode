use crate::budget::BudgetManager;
use crate::checkpoint::{ensure_goal_dir, save_checkpoint, save_state};
use crate::types::*;
use crate::watchdog::Watchdog;
use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;

#[derive(Clone)]
pub struct GoalRunner {
    pub state: GoalState,
    pub budget: BudgetManager,
    watchdog: Watchdog,
    event_callback: Option<Arc<dyn Fn(GoalEvent) + Send + Sync>>,
    goal_dir: Option<std::path::PathBuf>,
}

impl GoalRunner {
    pub fn new(objective: impl Into<String>, config: GoalConfig) -> Self {
        let mut cfg = config;
        let objective = objective.into();
        cfg.objective = objective.clone();
        let goal_id = generate_goal_id();
        let state = GoalState::new(goal_id, cfg);
        let budget = BudgetManager::new(state.config.clone());
        GoalRunner {
            state,
            budget,
            watchdog: Watchdog::new(),
            event_callback: None,
            goal_dir: None,
        }
    }

    pub fn with_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(GoalEvent) + Send + Sync + 'static,
    {
        self.event_callback = Some(Arc::new(callback));
        self
    }

    fn emit(&self, kind: GoalEventKind, message: impl Into<String>) {
        if let Some(cb) = &self.event_callback {
            cb(GoalEvent::new(kind, message));
        }
    }

    fn emit_with_task(&self, kind: GoalEventKind, task_id: &str, message: impl Into<String>) {
        if let Some(cb) = &self.event_callback {
            let mut evt = GoalEvent::new(kind, message);
            evt.task_id = Some(task_id.to_string());
            evt.iteration = Some(self.state.current_iteration);
            cb(evt);
        }
    }

    pub fn state(&self) -> &GoalState {
        &self.state
    }

    pub fn goal_id(&self) -> &str {
        &self.state.goal_id
    }

    pub fn config(&self) -> &GoalConfig {
        &self.state.config
    }

    pub fn format_system_prompt_suffix(&self) -> String {
        self.state.format_todo_markdown()
    }

    pub fn format_progress(&self) -> String {
        self.state.format_progress_md()
    }

    pub fn progress_bar(&self) -> String {
        let pct = self.state.progress_percent() as usize;
        let width = 20;
        let filled = (pct as f64 / 100.0 * width as f64) as usize;
        let empty = width - filled;
        format!(
            "[{}{}] {}/{} ({}%) iter:{}/{} time:{}",
            "█".repeat(filled),
            "░".repeat(empty),
            self.state.completed_count,
            self.state.total_todos,
            pct,
            self.state.current_iteration,
            self.state.config.max_iterations,
            self.budget.elapsed_display(),
        )
    }

    pub fn start(&mut self) -> Result<()> {
        let dir = ensure_goal_dir(&self.state.goal_id)?;
        self.goal_dir = Some(dir.clone());
        self.state.status = GoalStatus::Planning;
        save_state(&self.state)?;
        self.emit(
            GoalEventKind::GoalStart,
            format!("🎯 Goal 启动: {}\n\n正在分析目标并制定初始计划...", self.state.config.objective),
        );
        Ok(())
    }

    pub fn set_initial_plan(&mut self, todos: Vec<TodoItem>, criteria: Option<Vec<String>>) {
        self.state.todos = todos;
        self.state.config.success_criteria = criteria;
        self.state.status = GoalStatus::Running;
        self.state.refresh_counts();
        self.state.current_iteration = 0;
        self.budget.iterations = 0;
        self.watchdog.record_completion();
        if let Err(e) = save_state(&self.state) {
            self.emit(GoalEventKind::Failed, format!("保存状态失败: {}", e));
        }
        self.emit(
            GoalEventKind::PlanReady,
            format!(
                "📋 初始计划已生成，共 {} 个子任务，开始执行。\n\n{}",
                self.state.total_todos,
                self.format_todo_list()
            ),
        );
    }

    pub fn format_todo_list(&self) -> String {
        let mut out = String::new();
        for todo in &self.state.todos {
            let icon = match &todo.status {
                TodoStatus::Done => "✅",
                TodoStatus::InProgress => "🔄",
                TodoStatus::Blocked { .. } => "⚠️",
                TodoStatus::Skipped { .. } => "⏭️",
                TodoStatus::Pending => "⬜",
            };
            out.push_str(&format!("{} {}: {}\n", icon, todo.id, todo.title));
        }
        out
    }

    pub fn begin_iteration(&mut self) -> Option<&TodoItem> {
        self.budget.tick();
        self.state.current_iteration = self.budget.iterations;
        self.watchdog.record_iteration();

        if let Some(warn) = self.budget.check_warning() {
            self.emit(GoalEventKind::BudgetWarning, format!("⚠️ Budget warning: {:?}", warn));
        }

        if let Some(exceeded) = self.budget.check_exceeded() {
            self.state.status = GoalStatus::BudgetExceeded;
            self.emit(GoalEventKind::BudgetExceeded, format!("⏹️ Budget exceeded: {}", exceeded));
            if let Err(e) = self.persist() {
                eprintln!("persist error: {}", e);
            }
            return None;
        }

        let next = self.state.next_pending_task().cloned();
        if let Some(task) = &next {
            if let Some(t) = self.state.get_todo_mut(&task.id) {
                t.status = TodoStatus::InProgress;
                self.state.current_task_id = Some(t.id.clone());
            }
            self.state.refresh_counts();
            self.emit_with_task(
                GoalEventKind::TaskStart,
                &task.id,
                format!("▶ [{}] {}", task.id, task.title),
            );
            self.persist_ok();
            return self.state.todos.iter().find(|t| t.id == task.id);
        }

        None
    }

    pub fn get_think_prompt(&self, task: &TodoItem) -> String {
        format!(
            "你正在执行Goal模式的一个子任务。\n\n\
             ## Goal\n{}\n\n\
             ## 当前任务\n- ID: {}\n- 标题: {}\n{}\n\n\
             ## 进度\n{}\n\n\
             ## 指令\n请完成当前任务。修改代码后运行cargo check验证。\
             如果遇到无法解决的问题，在回复开头输出 [BLOCKED: <原因>]。\
             如果任务完成，在回复开头输出 [DONE: <完成摘要>]。\
             不要一次性修改太多文件，一步一步来。\
             不要问用户问题，自主解决尽可能多的问题。",
            self.state.config.objective,
            task.id,
            task.title,
            task.description.as_deref().map(|d| format!("- 描述: {}", d)).unwrap_or_default(),
            self.format_todo_list(),
        )
    }

    pub fn record_task_result(
        &mut self,
        task_id: &str,
        success: bool,
        summary: &str,
        tokens_used: usize,
    ) {
        self.budget.add_tokens(tokens_used);
        self.state.tokens_used = self.budget.tokens_used;
        self.state.record_action(format!("[{}] {}", task_id, summary));

        if success {
            if let Some(t) = self.state.get_todo_mut(task_id) {
                t.status = TodoStatus::Done;
                t.completed_at = Some(Utc::now().to_rfc3339());
                t.notes.push(summary.to_string());
            }
            self.budget.record_success();
            self.watchdog.record_completion();
            self.state.consecutive_failures = 0;
            self.emit_with_task(
                GoalEventKind::TaskComplete,
                task_id,
                format!("✅ [{}] 完成: {}", task_id, summary.chars().take(200).collect::<String>()),
            );
        } else {
            self.budget.record_failure();
            self.state.consecutive_failures = self.budget.consecutive_failures;
            self.watchdog.record_error(summary);
            self.emit_with_task(
                GoalEventKind::TaskFailed,
                task_id,
                format!("❌ [{}] 失败: {}", task_id, summary.chars().take(200).collect::<String>()),
            );
            if self.state.consecutive_failures >= self.config().max_consecutive_failures {
                self.state.status = GoalStatus::Failed(
                    format!("连续失败 {} 次，暂停执行", self.state.consecutive_failures)
                );
            }
        }

        self.state.current_task_id = None;
        self.state.refresh_counts();

        if let Some(stall) = self.watchdog.detect_stall(&self.state.recent_actions) {
            self.emit(GoalEventKind::Stalled, format!("🐛 检测到停滞: {}，触发反思...", stall));
            self.watchdog.record_completion();
            self.state.add_reflection(format!("Stall detected - trying different approach"));
        }

        self.maybe_checkpoint();
        self.persist_ok();
    }

    pub fn add_todo(&mut self, title: impl Into<String>) -> String {
        let id = format!("T{}", self.state.todos.len() + 1);
        let mut todo = TodoItem::new(id.clone(), title);
        todo.status = TodoStatus::Pending;
        self.state.todos.push(todo);
        self.state.refresh_counts();
        id
    }

    pub fn mark_all_remaining_skipped(&mut self, reason: &str) {
        for t in &mut self.state.todos {
            if matches!(t.status, TodoStatus::Pending | TodoStatus::InProgress) {
                t.status = TodoStatus::Skipped { reason: reason.to_string() };
            }
        }
        self.state.refresh_counts();
    }

    pub fn complete_goal(&mut self, summary: &str) {
        self.state.status = GoalStatus::Completed;
        if let Err(e) = save_checkpoint(&self.state, summary) {
            eprintln!("checkpoint error: {}", e);
        }
        self.state.add_reflection(format!("Goal completed: {}", summary));
        self.persist_ok();
        self.emit(
            GoalEventKind::Completed,
            format!("🎉 Goal 达成！\n\n{}\n\n{}", summary, self.format_progress()),
        );
    }

    pub fn fail_goal(&mut self, reason: &str) {
        self.state.status = GoalStatus::Failed(reason.to_string());
        self.persist_ok();
        self.emit(
            GoalEventKind::Failed,
            format!("💥 Goal 失败: {}\n\n{}", reason, self.format_progress()),
        );
    }

    pub fn pause(&mut self) {
        self.state.status = GoalStatus::Paused;
        self.persist_ok();
        self.emit(GoalEventKind::Paused, "⏸️ Goal 已暂停".to_string());
    }

    pub fn resume(&mut self) {
        self.state.status = GoalStatus::Running;
        self.persist_ok();
        self.emit(GoalEventKind::Resumed, "▶️ Goal 已恢复".to_string());
    }

    pub fn cancel(&mut self) {
        self.state.status = GoalStatus::Cancelled;
        self.persist_ok();
        self.emit(GoalEventKind::Cancelled, "🚫 Goal 已取消".to_string());
    }

    pub fn is_finished(&self) -> bool {
        matches!(
            self.state.status,
            GoalStatus::Completed | GoalStatus::Failed(_) | GoalStatus::Cancelled | GoalStatus::BudgetExceeded
        )
    }

    pub fn is_running(&self) -> bool {
        matches!(self.state.status, GoalStatus::Running)
    }

    pub fn is_blocked(&self) -> bool {
        matches!(self.state.status, GoalStatus::Blocked | GoalStatus::AwaitingApproval)
    }

    pub fn all_todos_done(&self) -> bool {
        self.state.todos.iter().all(|t| t.is_done()) && !self.state.todos.is_empty()
    }

    pub fn pending_tasks_exist(&self) -> bool {
        self.state.todos.iter().any(|t| matches!(t.status, TodoStatus::Pending))
    }

    fn maybe_checkpoint(&mut self) {
        if self.state.current_iteration > 0
            && self.state.current_iteration % self.state.config.checkpoint_interval == 0
        {
            let summary = format!(
                "Checkpoint at iteration {}: {}/{} tasks done",
                self.state.current_iteration,
                self.state.completed_count,
                self.state.total_todos,
            );
            match save_checkpoint(&self.state, &summary) {
                Ok(cp) => {
                    self.state.last_checkpoint = Some(cp.clone());
                    self.emit(
                        GoalEventKind::CheckpointSaved,
                        format!("💾 检查点已保存 [{}] - {}", cp.id, summary),
                    );
                }
                Err(e) => {
                    eprintln!("checkpoint save error: {}", e);
                }
            }
        }
    }

    fn persist_ok(&mut self) {
        self.state.refresh_counts();
        if let Err(e) = save_state(&self.state) {
            eprintln!("goal state persist error: {}", e);
        }
    }

    pub fn persist(&self) -> Result<()> {
        save_state(&self.state)
    }
}
