use std::collections::HashMap;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TeamStatus {
    Pending,
    Analyzing,
    Planning,
    AwaitingConfirmation,
    Executing,
    Aggregating,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for TeamStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TeamStatus::Pending => write!(f, "待启动"),
            TeamStatus::Analyzing => write!(f, "分析中"),
            TeamStatus::Planning => write!(f, "制定计划中"),
            TeamStatus::AwaitingConfirmation => write!(f, "等待确认"),
            TeamStatus::Executing => write!(f, "执行中"),
            TeamStatus::Aggregating => write!(f, "汇总中"),
            TeamStatus::Completed => write!(f, "已完成"),
            TeamStatus::Failed => write!(f, "失败"),
            TeamStatus::Cancelled => write!(f, "已取消"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SubTaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

impl std::fmt::Display for SubTaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubTaskStatus::Pending => write!(f, "等待中"),
            SubTaskStatus::Running => write!(f, "执行中"),
            SubTaskStatus::Completed => write!(f, "已完成"),
            SubTaskStatus::Failed => write!(f, "失败"),
            SubTaskStatus::Skipped => write!(f, "已跳过"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentConfig {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub tools: Vec<String>,
    pub model: String,
    pub working_dir: Option<PathBuf>,
    pub max_steps: usize,
}

impl Default for SubAgentConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            system_prompt: String::new(),
            tools: Vec::new(),
            model: "inherit".to_string(),
            working_dir: None,
            max_steps: 50,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTask {
    pub id: String,
    pub name: String,
    pub description: String,
    pub template: Option<String>,
    pub config: SubAgentConfig,
    pub depends_on: Vec<String>,
    pub status: SubTaskStatus,
    pub retry_count: u32,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPlan {
    pub task_id: String,
    pub original_task: String,
    pub subtasks: Vec<SubTask>,
    pub phases: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommandRecord {
    pub command: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTaskResult {
    pub task_id: String,
    pub name: String,
    pub success: bool,
    pub modified_files: Vec<PathBuf>,
    pub commands_executed: Vec<CommandRecord>,
    pub issues: Vec<String>,
    pub summary: String,
    pub final_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentTemplate {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub default_tools: Vec<String>,
    pub default_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamProgress {
    pub phase_index: usize,
    pub total_phases: usize,
    pub completed_subtasks: usize,
    pub total_subtasks: usize,
    pub current_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TeamEvent {
    pub kind: TeamEventKind,
    pub task_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum TeamEventKind {
    PhaseStarted,
    PhaseCompleted,
    SubTaskStarted,
    SubTaskProgress,
    SubTaskCompleted,
    SubTaskFailed,
    SubTaskRetry,
    AnalysisProgress,
    PlanningProgress,
    Aggregating,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(crate) struct FileLockManager {
    locks: HashMap<PathBuf, String>,
}

#[allow(dead_code)]
impl FileLockManager {
    pub fn new() -> Self {
        Self {
            locks: HashMap::new(),
        }
    }

    pub fn try_lock(&mut self, path: PathBuf, owner: String) -> bool {
        if self.locks.contains_key(&path) {
            return false;
        }
        self.locks.insert(path, owner);
        true
    }

    pub fn unlock(&mut self, path: &PathBuf) {
        self.locks.remove(path);
    }

    pub fn is_locked(&self, path: &PathBuf) -> bool {
        self.locks.contains_key(path)
    }

    pub fn unlock_all_for_owner(&mut self, owner: &str) {
        self.locks.retain(|_, o| o != owner);
    }
}
