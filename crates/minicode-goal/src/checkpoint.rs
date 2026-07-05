use crate::types::{goal_dir, goals_dir, Checkpoint, GoalState};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub fn ensure_goal_dir(goal_id: &str) -> Result<std::path::PathBuf> {
    let dir = goal_dir(goal_id);
    fs::create_dir_all(&dir).with_context(|| format!("创建goal目录失败: {}", dir.display()))?;
    fs::create_dir_all(dir.join("checkpoints"))
        .with_context(|| "创建checkpoints目录失败")?;
    Ok(dir)
}

pub fn save_state(state: &GoalState) -> Result<()> {
    let dir = ensure_goal_dir(&state.goal_id)?;
    let state_path = dir.join("goal.json");
    let json = serde_json::to_string_pretty(state)?;
    fs::write(&state_path, json).with_context(|| format!("写入goal.json失败: {}", state_path.display()))?;

    let progress_md = dir.join("progress.md");
    fs::write(&progress_md, state.format_progress_md())
        .with_context(|| format!("写入progress.md失败: {}", progress_md.display()))?;

    Ok(())
}

pub fn load_state(goal_id: &str) -> Result<GoalState> {
    let path = goal_dir(goal_id).join("goal.json");
    let content = fs::read_to_string(&path)
        .with_context(|| format!("读取goal.json失败: {}", path.display()))?;
    let state: GoalState = serde_json::from_str(&content)
        .with_context(|| format!("解析goal.json失败: {}", path.display()))?;
    Ok(state)
}

pub fn save_checkpoint(state: &GoalState, summary: &str) -> Result<Checkpoint> {
    let dir = goal_dir(&state.goal_id);
    let cp_dir = dir.join("checkpoints");
    fs::create_dir_all(&cp_dir)?;

    let cp = Checkpoint {
        id: format!("cp-{:04}", state.current_iteration),
        iteration: state.current_iteration,
        timestamp: state.updated_at.clone(),
        git_commit: None,
        todo_snapshot: state.todos.clone(),
        tokens_used: state.tokens_used,
        summary: summary.to_string(),
    };

    let cp_path = cp_dir.join(format!("{}.json", cp.id));
    fs::write(&cp_path, serde_json::to_string_pretty(&cp)?)?;

    Ok(cp)
}

pub fn list_goals() -> Result<Vec<GoalState>> {
    let root = goals_dir();
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    for entry in fs::read_dir(&root).with_context(|| format!("读取goals目录失败: {}", root.display()))? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let goal_json = path.join("goal.json");
        if goal_json.exists() {
            if let Ok(state) = load_state_from_path(&goal_json) {
                results.push(state);
            }
        }
    }

    results.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(results)
}

fn load_state_from_path(path: &Path) -> Result<GoalState> {
    let content = fs::read_to_string(path)?;
    let state: GoalState = serde_json::from_str(&content)?;
    Ok(state)
}

pub fn goal_exists(goal_id: &str) -> bool {
    goal_dir(goal_id).join("goal.json").exists()
}

pub fn list_recent_goals(limit: usize) -> Result<Vec<GoalState>> {
    let mut goals = list_goals()?;
    goals.truncate(limit);
    Ok(goals)
}
