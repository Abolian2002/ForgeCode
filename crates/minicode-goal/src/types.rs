use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub type GoalId = String;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GoalStatus {
    Planning,
    Running,
    Reflecting,
    Paused,
    AwaitingApproval,
    Blocked,
    Completed,
    Failed(String),
    Cancelled,
    BudgetExceeded,
}

impl std::fmt::Display for GoalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GoalStatus::Planning => write!(f, "Planning"),
            GoalStatus::Running => write!(f, "Running"),
            GoalStatus::Reflecting => write!(f, "Reflecting"),
            GoalStatus::Paused => write!(f, "Paused"),
            GoalStatus::AwaitingApproval => write!(f, "AwaitingApproval"),
            GoalStatus::Blocked => write!(f, "Blocked"),
            GoalStatus::Completed => write!(f, "Completed"),
            GoalStatus::Failed(e) => write!(f, "Failed: {}", e),
            GoalStatus::Cancelled => write!(f, "Cancelled"),
            GoalStatus::BudgetExceeded => write!(f, "BudgetExceeded"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PermissionMode {
    Plan,
    Default,
    AcceptEdits,
    DontAsk,
}

impl Default for PermissionMode {
    fn default() -> Self {
        PermissionMode::Default
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalConfig {
    pub objective: String,
    pub success_criteria: Option<Vec<String>>,
    pub max_budget_tokens: Option<usize>,
    pub max_duration_secs: u64,
    pub max_iterations: usize,
    pub max_consecutive_failures: usize,
    pub permission_mode: PermissionMode,
    pub auto_git_commit: bool,
    pub checkpoint_interval: usize,
    pub allow_team_dispatch: bool,
    pub allow_subagent_dispatch: bool,
    pub microcompact_threshold: f64,
    pub snip_threshold: f64,
    pub snip_target: f64,
    pub autocompact_tokens: usize,
}

impl Default for GoalConfig {
    fn default() -> Self {
        Self {
            objective: String::new(),
            success_criteria: None,
            max_budget_tokens: None,
            max_duration_secs: 4 * 60 * 60,
            max_iterations: 200,
            max_consecutive_failures: 5,
            permission_mode: PermissionMode::Default,
            auto_git_commit: false,
            checkpoint_interval: 5,
            allow_team_dispatch: true,
            allow_subagent_dispatch: true,
            microcompact_threshold: 0.30,
            snip_threshold: 0.50,
            snip_target: 0.40,
            autocompact_tokens: 65_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Blocked { reason: String },
    Done,
    Skipped { reason: String },
}

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

impl TodoItem {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: None,
            status: TodoStatus::Pending,
            depends_on: Vec::new(),
            created_at: Utc::now().to_rfc3339(),
            completed_at: None,
            notes: Vec::new(),
        }
    }

    pub fn is_done(&self) -> bool {
        matches!(self.status, TodoStatus::Done | TodoStatus::Skipped { .. })
    }

    pub fn is_active(&self) -> bool {
        matches!(self.status, TodoStatus::InProgress)
    }
}

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
    pub recent_actions: Vec<String>,
    pub reflection_notes: Vec<String>,
    pub blocked_reason: Option<String>,
    pub completed_count: usize,
    pub total_todos: usize,
    pub current_task_id: Option<String>,
}

impl GoalState {
    pub fn new(goal_id: GoalId, config: GoalConfig) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            goal_id,
            config,
            status: GoalStatus::Planning,
            todos: Vec::new(),
            current_iteration: 0,
            tokens_used: 0,
            started_at: now.clone(),
            updated_at: now,
            last_checkpoint: None,
            consecutive_failures: 0,
            recent_actions: Vec::new(),
            reflection_notes: Vec::new(),
            blocked_reason: None,
            completed_count: 0,
            total_todos: 0,
            current_task_id: None,
        }
    }

    pub fn refresh_counts(&mut self) {
        self.completed_count = self.todos.iter().filter(|t| t.is_done()).count();
        self.total_todos = self.todos.len();
        self.updated_at = Utc::now().to_rfc3339();
    }

    pub fn next_pending_task(&self) -> Option<&TodoItem> {
        self.todos.iter().find(|t| {
            matches!(t.status, TodoStatus::Pending)
                && t.depends_on.iter().all(|dep_id| {
                    self.todos
                        .iter()
                        .any(|t2| &t2.id == dep_id && t2.is_done())
                })
        })
    }

    pub fn get_todo_mut(&mut self, id: &str) -> Option<&mut TodoItem> {
        self.todos.iter_mut().find(|t| t.id == id)
    }

    pub fn progress_percent(&self) -> u8 {
        if self.total_todos == 0 {
            return 0;
        }
        ((self.completed_count as f64 / self.total_todos as f64) * 100.0) as u8
    }

    pub fn format_todo_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("## Goal\n{}\n\n", self.config.objective));
        if let Some(criteria) = &self.config.success_criteria {
            out.push_str("## Success Criteria\n");
            for c in criteria {
                out.push_str(&format!("- [ ] {}\n", c));
            }
            out.push('\n');
        }
        out.push_str(&format!(
            "## Progress ({}/{}, {}%)\n",
            self.completed_count,
            self.total_todos,
            self.progress_percent()
        ));
        for todo in &self.todos {
            let marker = match &todo.status {
                TodoStatus::Done => "[x]",
                TodoStatus::InProgress => "[~]",
                TodoStatus::Blocked { .. } => "[!]",
                TodoStatus::Skipped { .. } => "[-]",
                TodoStatus::Pending => "[ ]",
            };
            out.push_str(&format!("{} {} {}", marker, todo.id, todo.title));
            if let TodoStatus::Blocked { reason } = &todo.status {
                out.push_str(&format!(" (blocked: {})", reason));
            }
            out.push('\n');
        }
        if !self.reflection_notes.is_empty() {
            out.push_str("\n## Reflection Notes\n");
            for note in self.reflection_notes.iter().take(5) {
                out.push_str(&format!("- {}\n", note));
            }
        }
        out
    }

    pub fn format_progress_md(&self) -> String {
        let elapsed = self.elapsed_secs();
        let hours = elapsed / 3600;
        let mins = (elapsed % 3600) / 60;
        format!(
            "# Goal: {}\n\n\
             **Status**: {} | **Iteration**: {}/{} | **Time**: {}h{}m/{}h\n\
             **Progress**: {}/{} ({}%) | **Tokens**: ~{}\n\n\
             ## Completed\n{}\n\n\
             ## In Progress\n{}\n\n\
             ## Pending\n{}\n\n\
             ## Blocked\n{}\n\n\
             ## Reflection Notes\n{}\n",
            self.config.objective,
            self.status,
            self.current_iteration,
            self.config.max_iterations,
            hours,
            mins,
            self.config.max_duration_secs / 3600,
            self.completed_count,
            self.total_todos,
            self.progress_percent(),
            self.tokens_used,
            self.format_todo_section(|t| matches!(t.status, TodoStatus::Done)),
            self.format_todo_section(|t| matches!(t.status, TodoStatus::InProgress)),
            self.format_todo_section(|t| matches!(t.status, TodoStatus::Pending)),
            self.format_blocked(),
            self.format_reflections(),
        )
    }

    fn format_todo_section<F>(&self, filter: F) -> String
    where
        F: Fn(&TodoItem) -> bool,
    {
        let items: Vec<_> = self.todos.iter().filter(|t| filter(t)).collect();
        if items.is_empty() {
            "(none)".to_string()
        } else {
            items
                .iter()
                .map(|t| format!("- [{}] {}: {}", status_icon(&t.status), t.id, t.title))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }

    fn format_blocked(&self) -> String {
        let items: Vec<_> = self
            .todos
            .iter()
            .filter_map(|t| {
                if let TodoStatus::Blocked { reason } = &t.status {
                    Some(format!("- [!] {}: {}", t.id, reason))
                } else {
                    None
                }
            })
            .collect();
        if let Some(reason) = &self.blocked_reason {
            if items.is_empty() {
                reason.clone()
            } else {
                format!("{}\n{}", reason, items.join("\n"))
            }
        } else if items.is_empty() {
            "(none)".to_string()
        } else {
            items.join("\n")
        }
    }

    fn format_reflections(&self) -> String {
        if self.reflection_notes.is_empty() {
            "(none)".to_string()
        } else {
            self.reflection_notes
                .iter()
                .rev()
                .take(5)
                .map(|n| format!("- {}", n))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }

    pub fn elapsed_secs(&self) -> u64 {
        let started = DateTime::parse_from_rfc3339(&self.started_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        (Utc::now() - started).num_seconds().max(0) as u64
    }

    pub fn add_reflection(&mut self, note: String) {
        self.reflection_notes.push(note);
        if self.reflection_notes.len() > 10 {
            self.reflection_notes.remove(0);
        }
        self.updated_at = Utc::now().to_rfc3339();
    }

    pub fn record_action(&mut self, action_summary: String) {
        self.recent_actions.push(action_summary);
        if self.recent_actions.len() > 20 {
            self.recent_actions.remove(0);
        }
    }
}

fn status_icon(status: &TodoStatus) -> &'static str {
    match status {
        TodoStatus::Done => "x",
        TodoStatus::InProgress => "~",
        TodoStatus::Blocked { .. } => "!",
        TodoStatus::Skipped { .. } => "-",
        TodoStatus::Pending => " ",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalEvent {
    pub kind: GoalEventKind,
    pub message: String,
    pub task_id: Option<String>,
    pub iteration: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalEventKind {
    GoalStart,
    PlanningProgress,
    PlanReady,
    IterationStart,
    TaskStart,
    TaskComplete,
    TaskFailed,
    Progress,
    Reflecting,
    ReflectionComplete,
    CheckpointSaved,
    Stalled,
    AwaitingApproval,
    BudgetWarning,
    Milestone,
    Completed,
    Failed,
    Paused,
    Resumed,
    Cancelled,
    BudgetExceeded,
}

impl GoalEvent {
    pub fn new(kind: GoalEventKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            task_id: None,
            iteration: None,
        }
    }

    pub fn with_task(mut self, task_id: impl Into<String>) -> Self {
        self.task_id = Some(task_id.into());
        self
    }

    pub fn with_iteration(mut self, iteration: usize) -> Self {
        self.iteration = Some(iteration);
        self
    }
}

pub fn goals_dir() -> PathBuf {
    let cwd = minicode_config::runtime_store().cwd.clone();
    cwd.join(".minicode").join("goals")
}

pub fn goal_dir(goal_id: &str) -> PathBuf {
    goals_dir().join(goal_id)
}

pub fn generate_goal_id() -> GoalId {
    format!("goal-{}", Utc::now().format("%Y%m%d-%H%M%S"))
}
