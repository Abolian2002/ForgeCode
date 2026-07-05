use std::collections::HashMap;
use std::sync::Arc;
use chrono::Utc;
use uuid::Uuid;

use crate::isolated_agent::run_isolated_subagent;
use crate::types::*;

const MAX_SUBAGENTS: usize = 5;
const MAX_RETRIES: u32 = 2;

#[derive(Clone)]
pub struct TeamOrchestrator {
    session: Option<TeamSession>,
    event_callback: Option<Arc<dyn Fn(TeamEvent) + Send + Sync>>,
}

#[derive(Debug, Clone)]
pub struct TeamSession {
    pub session_id: String,
    pub task: String,
    pub status: TeamStatus,
    pub plan: Option<TaskPlan>,
    pub results: HashMap<String, SubTaskResult>,
    pub created_at: String,
    pub is_continuation: bool,
}

impl Default for TeamOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl TeamOrchestrator {
    pub fn new() -> Self {
        Self {
            session: None,
            event_callback: None,
        }
    }

    pub fn with_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(TeamEvent) + Send + Sync + 'static,
    {
        self.event_callback = Some(Arc::new(callback));
        self
    }

    fn emit(&self, kind: TeamEventKind, task_id: Option<&str>, message: impl Into<String>) {
        if let Some(cb) = &self.event_callback {
            cb(TeamEvent {
                kind,
                task_id: task_id.map(|s| s.to_string()),
                message: message.into(),
            });
        }
    }

    pub fn session(&self) -> Option<&TeamSession> {
        self.session.as_ref()
    }

    pub fn plan(&self) -> Option<&TaskPlan> {
        self.session.as_ref().and_then(|s| s.plan.as_ref())
    }

    pub fn format_plan(&self) -> String {
        let Some(session) = &self.session else {
            return "没有活动的团队任务".to_string();
        };
        let Some(plan) = &session.plan else {
            return "任务计划尚未生成".to_string();
        };

        let mut out = String::new();
        out.push_str(&format!("┌─ MiniCode 团队模式 ──────────────────────────────\n"));
        out.push_str(&format!("│ 任务：{}\n", plan.original_task));
        out.push_str(&format!("│ 子任务数：{}（最多 {}）\n", plan.subtasks.len(), MAX_SUBAGENTS));
        out.push_str(&format!("│ 执行阶段：{}\n", plan.phases.len()));
        out.push_str(&format!("├──────────────────────────────────────────────────\n"));

        for (phase_idx, phase) in plan.phases.iter().enumerate() {
            let parallel_note = if phase.len() > 1 {
                format!("（并行 {} 个）", phase.len())
            } else {
                String::new()
            };
            out.push_str(&format!("│ 阶段 {}{}：\n", phase_idx + 1, parallel_note));
            for tid in phase {
                if let Some(st) = plan.subtasks.iter().find(|s| &s.id == tid) {
                    let tpl = st.template.as_deref().unwrap_or("custom");
                    let deps = if st.depends_on.is_empty() {
                        String::new()
                    } else {
                        format!("（依赖：{}）", st.depends_on.join(", "))
                    };
                    out.push_str(&format!(
                        "│   [{}] {} - {} [{}] {}\n",
                        st.id, st.name, st.description, tpl, deps
                    ));
                }
            }
        }
        out.push_str(&format!("└──────────────────────────────────────────────────\n"));
        out
    }

    pub async fn start_team(&mut self, task: &str) -> Result<TaskPlan, String> {
        let task_id = format!("team-{}", Uuid::new_v4().simple());
        let mut session = TeamSession {
            session_id: task_id.clone(),
            task: task.to_string(),
            status: TeamStatus::Analyzing,
            plan: None,
            results: HashMap::new(),
            created_at: Utc::now().to_rfc3339(),
            is_continuation: false,
        };

        self.emit(TeamEventKind::AnalysisProgress, None, "正在分析任务结构...");

        let plan = self.generate_plan(task, &task_id).await?;

        session.plan = Some(plan.clone());
        session.status = TeamStatus::AwaitingConfirmation;
        self.session = Some(session);

        Ok(plan)
    }

    pub async fn run_team_task(&mut self, task: &str) -> Result<String, String> {
        self.emit(TeamEventKind::PlanningProgress, None, "正在分析任务并生成计划...");
        let _plan = self.start_team(task).await?;

        let plan_text = self.format_plan();
        self.emit(TeamEventKind::PlanningProgress, None, plan_text);

        self.execute_plan().await
    }

    async fn generate_plan(&self, task: &str, task_id: &str) -> Result<TaskPlan, String> {
        let task_lower = task.to_lowercase();

        let mut subtasks = Vec::new();
        let mut phases = Vec::new();

        let (base_templates, extra_subtasks) = analyze_task(task, &task_lower);

        for (idx, (tpl_name, name, desc)) in base_templates.into_iter().enumerate() {
            let tid = format!("T{}", idx + 1);
            let config = build_config_from_template(&tpl_name, &name, &desc);
            subtasks.push(SubTask {
                id: tid.clone(),
                name,
                description: desc,
                template: Some(tpl_name.to_string()),
                config,
                depends_on: Vec::new(),
                status: SubTaskStatus::Pending,
                retry_count: 0,
                error: None,
            });
            phases.push(vec![tid]);
        }

        let offset = subtasks.len();
        for (idx, (dep_name, dep_desc, dep_tasks)) in extra_subtasks.into_iter().enumerate() {
            let tid = format!("T{}", offset + idx + 1);
            let (tpl_name, config) = if dep_desc.contains("测试") || dep_desc.contains("test") {
                ("test-expert", build_config_from_template("test-expert", &dep_name, &dep_desc))
            } else if dep_desc.contains("审查") || dep_desc.contains("review") {
                ("code-reviewer", build_config_from_template("code-reviewer", &dep_name, &dep_desc))
            } else {
                ("code-modifier", build_config_from_template("code-modifier", &dep_name, &dep_desc))
            };
            let depends_on: Vec<String> = dep_tasks
                .iter()
                .map(|i| format!("T{}", *i + 1))
                .collect();
            subtasks.push(SubTask {
                id: tid.clone(),
                name: dep_name,
                description: dep_desc,
                template: Some(tpl_name.to_string()),
                config,
                depends_on: depends_on.clone(),
                status: SubTaskStatus::Pending,
                retry_count: 0,
                error: None,
            });
            phases.push(vec![tid]);
        }

        if subtasks.len() > MAX_SUBAGENTS {
            subtasks.truncate(MAX_SUBAGENTS);
            let last_phase = vec![subtasks.last().unwrap().id.clone()];
            phases.truncate(subtasks.len());
            if phases.is_empty() || phases.last().map_or(true, |p| p[0] != last_phase[0]) {
                phases.push(last_phase);
            }
        }

        if subtasks.is_empty() {
            let tid = "T1".to_string();
            let config = build_config_from_template("code-modifier", "执行任务", task);
            subtasks.push(SubTask {
                id: tid.clone(),
                name: "执行任务".to_string(),
                description: task.to_string(),
                template: Some("code-modifier".to_string()),
                config,
                depends_on: Vec::new(),
                status: SubTaskStatus::Pending,
                retry_count: 0,
                error: None,
            });
            phases.push(vec![tid]);
        }

        Ok(TaskPlan {
            task_id: task_id.to_string(),
            original_task: task.to_string(),
            subtasks,
            phases,
        })
    }

    pub async fn execute_plan(&mut self) -> Result<String, String> {
        let session = self.session.as_mut().ok_or("没有活动的团队会话")?;
        session.status = TeamStatus::Executing;

        let plan = session.plan.clone().ok_or("任务计划不存在")?;
        let mut results = HashMap::new();

        for (phase_idx, phase) in plan.phases.iter().enumerate() {
            self.emit(
                TeamEventKind::PhaseStarted,
                None,
                format!("开始执行阶段 {}/{}", phase_idx + 1, plan.phases.len()),
            );

            for tid in phase {
                let subtask = plan
                    .subtasks
                    .iter()
                    .find(|s| &s.id == tid)
                    .ok_or_else(|| format!("子任务 {} 不存在", tid))?;

                self.emit(
                    TeamEventKind::SubTaskStarted,
                    Some(tid),
                    format!("[{}] 开始执行: {}", subtask.name, subtask.description),
                );

                let mut result = self.run_subtask_with_retry(subtask).await;
                result.task_id = tid.clone();
                results.insert(tid.clone(), result.clone());

                if result.success {
                    self.emit(
                        TeamEventKind::SubTaskCompleted,
                        Some(tid),
                        format!(
                            "[{}] 完成。修改文件：{}，执行命令：{}",
                            subtask.name,
                            result.modified_files.len(),
                            result.commands_executed.len()
                        ),
                    );
                } else {
                    self.emit(
                        TeamEventKind::SubTaskFailed,
                        Some(tid),
                        format!("[{}] 失败: {}", subtask.name, result.summary),
                    );
                }
            }

            self.emit(
                TeamEventKind::PhaseCompleted,
                None,
                format!("阶段 {} 完成", phase_idx + 1),
            );
        }

        self.emit(TeamEventKind::Aggregating, None, "正在汇总所有子任务结果...");

        let all_success = results.values().all(|r| r.success);
        let summary = self.build_summary(&plan, &results);

        let session = self.session.as_mut().unwrap();
        session.results = results;
        session.status = if all_success {
            TeamStatus::Completed
        } else {
            TeamStatus::Failed
        };

        self.emit(
            TeamEventKind::Completed,
            None,
            format!(
                "团队任务执行{}",
                if all_success { "完成" } else { "部分失败" }
            ),
        );

        Ok(summary)
    }

    async fn run_subtask_with_retry(&self, subtask: &SubTask) -> SubTaskResult {
        let mut last_result = None;

        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                self.emit(
                    TeamEventKind::SubTaskRetry,
                    Some(&subtask.id),
                    format!("[{}] 第 {} 次重试...", subtask.name, attempt),
                );
            }

            let result = run_isolated_subagent(&subtask.config, &subtask.description).await;

            if result.success || attempt == MAX_RETRIES {
                return result;
            }
            last_result = Some(result);
        }

        last_result.unwrap_or_else(|| SubTaskResult {
            task_id: subtask.id.clone(),
            name: subtask.name.clone(),
            success: false,
            modified_files: Vec::new(),
            commands_executed: Vec::new(),
            issues: vec!["重试次数耗尽".to_string()],
            summary: "子任务执行失败，重试次数耗尽".to_string(),
            final_message: None,
        })
    }

    fn build_summary(&self, plan: &TaskPlan, results: &HashMap<String, SubTaskResult>) -> String {
        let mut out = String::new();
        let total = plan.subtasks.len();
        let succeeded = results.values().filter(|r| r.success).count();
        let failed = total - succeeded;

        out.push_str("## 团队任务执行报告\n\n");
        out.push_str(&format!("**任务**: {}\n\n", plan.original_task));
        out.push_str(&format!(
            "**状态**: {} 成功，{} 失败，共 {} 个子任务\n\n",
            succeeded, failed, total
        ));

        let mut all_modified = Vec::new();
        for r in results.values() {
            for f in &r.modified_files {
                if !all_modified.contains(f) {
                    all_modified.push(f.clone());
                }
            }
        }

        if !all_modified.is_empty() {
            out.push_str("### 修改的文件\n\n");
            for f in &all_modified {
                out.push_str(&format!("- `{}`\n", f.display()));
            }
            out.push('\n');
        }

        out.push_str("### 各子任务结果\n\n");
        for st in &plan.subtasks {
            if let Some(r) = results.get(&st.id) {
                let status_icon = if r.success { "✅" } else { "❌" };
                out.push_str(&format!(
                    "{} **[{}] {}** - {}\n",
                    status_icon, st.id, st.name, st.description
                ));
                if !r.issues.is_empty() {
                    for issue in &r.issues {
                        out.push_str(&format!("  - ⚠️ {}\n", issue));
                    }
                }
                if let Some(msg) = &r.final_message {
                    let preview: String = msg.chars().take(500).collect();
                    out.push_str(&format!("  {}\n", preview.replace('\n', "\n  ")));
                }
                out.push('\n');
            }
        }

        out
    }
}

fn analyze_task(task: &str, task_lower: &str) -> (Vec<(&'static str, String, String)>, Vec<(String, String, Vec<usize>)>) {
    let mut base_templates = Vec::new();
    let mut extra_subtasks = Vec::new();

    let needs_test = task_lower.contains("测试")
        || task_lower.contains("test")
        || task_lower.contains("单元测试");
    let needs_review = task_lower.contains("审查")
        || task_lower.contains("review")
        || task_lower.contains("代码审查");
    let is_refactor = task_lower.contains("重构") || task_lower.contains("refactor");
    let is_fix = task_lower.contains("修复") || task_lower.contains("fix") || task_lower.contains("bug");
    let is_feature = task_lower.contains("添加") || task_lower.contains("实现") || task_lower.contains("开发")
        || task_lower.contains("add") || task_lower.contains("implement");

    if is_fix {
        base_templates.push((
            "debugger",
            "debug-fix".to_string(),
            format!("定位并修复问题：{}", task),
        ));
    } else if is_refactor {
        base_templates.push((
            "code-modifier",
            "refactor-code".to_string(),
            format!("执行代码重构：{}", task),
        ));
    } else if is_feature {
        base_templates.push((
            "code-modifier",
            "implement-feature".to_string(),
            format!("实现功能：{}", task),
        ));
    } else {
        base_templates.push((
            "code-modifier",
            "execute-task".to_string(),
            task.to_string(),
        ));
    }

    let base_idx = base_templates.len() - 1;

    if needs_test {
        extra_subtasks.push((
            "run-tests".to_string(),
            format!("编写并运行测试：{}", task),
            vec![base_idx],
        ));
    }

    if needs_review || is_refactor || is_feature {
        let dep: Vec<usize> = if needs_test {
            vec![base_idx, base_idx + 1]
        } else {
            vec![base_idx]
        };
        extra_subtasks.push((
            "code-review".to_string(),
            format!("审查代码修改：{}", task),
            dep,
        ));
    }

    (base_templates, extra_subtasks)
}

fn build_config_from_template(template_name: &str, name: &str, description: &str) -> SubAgentConfig {
    let tpl = crate::templates::get_template(template_name);

    match tpl {
        Some(t) => SubAgentConfig {
            name: name.to_string(),
            description: description.to_string(),
            system_prompt: format!(
                "{}\n\n你的具体任务：{}",
                t.system_prompt, description
            ),
            tools: t.default_tools,
            model: t.default_model,
            working_dir: None,
            max_steps: 40,
        },
        None => SubAgentConfig {
            name: name.to_string(),
            description: description.to_string(),
            system_prompt: format!("你是一个高级软件工程师。你的任务：{}", description),
            tools: vec![
                "list_files".to_string(),
                "read_file".to_string(),
                "grep_files".to_string(),
                "edit_file".to_string(),
                "write_file".to_string(),
                "run_command".to_string(),
            ],
            model: "inherit".to_string(),
            working_dir: None,
            max_steps: 40,
        },
    }
}
