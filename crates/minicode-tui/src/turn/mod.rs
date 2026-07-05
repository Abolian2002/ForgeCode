use std::io::Stdout;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self};
use minicode_agent_core::{maybe_auto_compact_conversation, run_agent_turn_streaming};
use minicode_cli_commands::{find_matching_slash_commands, try_handle_local_command};
use minicode_goal::{create_rule_based_plan, GoalConfig, GoalRunner};
use minicode_history::{
    add_history_entry, append_runtime_message, estimate_context_tokens, load_history_entries,
    runtime_messages,
};
use minicode_permissions::get_permission_manager;
use minicode_prompt::build_system_prompt;
use minicode_team::TeamOrchestrator;
use minicode_tool::{get_tool_registry, parse_local_tool_shortcut};
use minicode_types::{ChatMessage, get_model_adapter};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;

use crate::render::render_screen;
use crate::state::{ChannelCallbacks, ScreenState, TurnEvent};

mod approval;
mod ask_user;
mod busy_input;
mod event_apply;
mod prompt_handler;

pub(crate) use approval::handle_approval_key;
pub(crate) use ask_user::{AskUserAction, handle_ask_user_key};
use busy_input::{BusyEventAction, handle_busy_event};
use event_apply::apply_turn_event;
use prompt_handler::build_prompt_handler;

const UI_POLL_MS: u64 = 16;

async fn handle_command_submission(state: &mut ScreenState, input: &str) {
    append_runtime_message(ChatMessage::runtime_display(
        "command",
        format!("> {input}"),
    ));
    match try_handle_local_command(input).await {
        Ok(Some(local)) => {
            append_runtime_message(ChatMessage::runtime_display("command:result", local));
        }
        Ok(None) => {
            let matches = find_matching_slash_commands(input);
            let msg = if matches.is_empty() {
                "未识别命令。输入 /help 查看可用命令。".to_string()
            } else {
                format!(
                    "未识别命令。你是不是想输入：\n{}",
                    matches
                        .iter()
                        .map(|(usage, _)| usage.clone())
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            };
            append_runtime_message(ChatMessage::runtime_display("command:error", msg));
        }
        Err(err) => {
            append_runtime_message(ChatMessage::runtime_display(
                "command:error",
                format!("local command failed: {err:#}"),
            ));
        }
    }
    state.transcript_scroll_offset = 0;
}

async fn queue_busy_submission(state: &mut ScreenState, raw: String) {
    let input = raw.trim().to_string();
    if input.is_empty() {
        return;
    }
    if input.starts_with('/') {
        handle_command_submission(state, &input).await;
        return;
    }
    let _ = add_history_entry(&input);
    state.history = load_history_entries();
    state.history_index = state.history.len();
    state.history_draft.clear();
    state.queued_busy_inputs.push(input);
    state.status = Some("新输入等待注入上下文...".to_string());
}

fn flush_queued_busy_inputs(state: &mut ScreenState) {
    if state.queued_busy_inputs.is_empty() {
        return;
    }
    let pending = std::mem::take(&mut state.queued_busy_inputs);
    for content in pending {
        append_runtime_message(ChatMessage::User { content });
    }
    state.context_tokens_estimate = estimate_context_tokens(&runtime_messages());
    state.transcript_scroll_offset = 0;
    if let Some(tool) = state.active_tool.as_ref() {
        state.status = Some(format!("Running {tool}..."));
    }
}

/// 异步处理 /compact 命令：先更新 UI 显示压缩中，再后台执行压缩。
async fn handle_compact_command(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &mut ScreenState,
    input: &str,
) -> Result<bool> {
    append_runtime_message(ChatMessage::runtime_display(
        "command",
        format!("> {input}"),
    ));

    // 立即更新 UI 状态
    state.is_busy = true;
    state.status = Some("压缩上下文中...".to_string());
    render_screen(terminal, state)?;

    // 收集当前消息（不含 system）
    let messages_without_system = minicode_history::runtime_messages_for_context();
    let mut messages = Vec::with_capacity(messages_without_system.len() + 1);
    messages.push(ChatMessage::System {
        content: build_system_prompt(),
    });
    messages.extend(messages_without_system);

    let count_before = messages.len();
    let model = get_model_adapter();

    // spawn 后台任务执行压缩
    let (tx, mut rx) = mpsc::unbounded_channel::<TurnEvent>();
    tokio::spawn(async move {
        let compacted = maybe_auto_compact_conversation(
            model.as_ref(),
            messages,
            Some(0), // 强制压缩，不检查阈值
            Some(2), // 保留最近 2 条
            None::<&(dyn Fn(&str) + Send + Sync)>,
        )
        .await;

        if compacted.len() < count_before {
            let arc = minicode_history::get_messages();
            let mut guard = match arc.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            let system_msgs: Vec<ChatMessage> = guard
                .iter()
                .filter(|m| matches!(m, ChatMessage::System { .. }))
                .cloned()
                .collect();
            guard.clear();
            guard.extend(system_msgs);
            for msg in &compacted {
                if !matches!(msg, ChatMessage::System { .. }) {
                    guard.push(msg.clone());
                }
            }
            drop(guard);
            minicode_history::persist_current_messages();
            let _ = tx.send(TurnEvent::Progress(format!(
                "上下文已压缩：{} 条消息 -> {} 条",
                count_before,
                compacted.len()
            )));
        } else {
            let _ = tx.send(TurnEvent::Progress(
                "当前上下文较短，无需压缩。".to_string(),
            ));
        }
        let _ = tx.send(TurnEvent::Done);
    });

    // 事件循环：等待压缩完成，同时保持 UI 响应
    let mut turn_done = false;
    while !turn_done {
        while let Ok(event) = rx.try_recv() {
            if apply_turn_event(state, event) {
                turn_done = true;
            }
        }
        render_screen(terminal, state)?;
        if !turn_done && event::poll(Duration::from_millis(UI_POLL_MS))? {
            let input_event = event::read()?;
            match handle_busy_event(state, input_event) {
                BusyEventAction::None => {}
                BusyEventAction::Submit(raw) => queue_busy_submission(state, raw).await,
                BusyEventAction::Interrupt => {
                    // 压缩无法中断，但可以标记完成
                    turn_done = true;
                }
            }
            render_screen(terminal, state)?;
        }
    }

    state.is_busy = false;
    state.status = None;
    state.context_tokens_estimate = estimate_context_tokens(&minicode_history::runtime_messages());
    state.transcript_scroll_offset = 0;
    render_screen(terminal, state)?;
    Ok(false)
}

async fn handle_team_command(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &mut ScreenState,
    task: String,
) -> Result<bool> {
    use minicode_team::TeamEventKind;

    append_runtime_message(ChatMessage::runtime_display(
        "command",
        format!("> /team {}", task),
    ));
    append_runtime_message(ChatMessage::runtime_display(
        "team:info",
        "🤖 团队模式启动，正在分析任务并制定计划...".to_string(),
    ));

    state.is_busy = true;
    state.team_mode_active = true;
    state.status = Some("团队模式: 分析中...".to_string());
    render_screen(terminal, state)?;

    let (tx, mut rx) = mpsc::unbounded_channel::<TurnEvent>();

    let task_clone = task.clone();
    tokio::spawn(async move {
        let tx_clone = tx.clone();
        let mut orchestrator = TeamOrchestrator::new()
            .with_callback(move |event: minicode_team::TeamEvent| {
                let turn_event = match event.kind {
                    TeamEventKind::AnalysisProgress | TeamEventKind::PlanningProgress => {
                        TurnEvent::TeamProgress(event.message)
                    }
                    TeamEventKind::PhaseStarted => TurnEvent::TeamPhaseStart {
                        phase: 0,
                        total: 0,
                    },
                    TeamEventKind::SubTaskStarted => TurnEvent::TeamSubTaskStart {
                        task_id: event.task_id.unwrap_or_default(),
                        name: event.message.clone(),
                        description: event.message,
                    },
                    TeamEventKind::SubTaskCompleted | TeamEventKind::PhaseCompleted => {
                        TurnEvent::TeamSubTaskComplete {
                            task_id: event.task_id.unwrap_or_default(),
                            name: event.message.clone(),
                            success: true,
                            summary: event.message,
                        }
                    }
                    TeamEventKind::SubTaskFailed | TeamEventKind::SubTaskRetry => {
                        TurnEvent::TeamSubTaskComplete {
                            task_id: event.task_id.unwrap_or_default(),
                            name: event.message.clone(),
                            success: false,
                            summary: event.message,
                        }
                    }
                    TeamEventKind::SubTaskProgress | TeamEventKind::Aggregating => {
                        TurnEvent::TeamProgress(event.message)
                    }
                    TeamEventKind::Completed | TeamEventKind::Failed => {
                        TurnEvent::TeamComplete(event.message)
                    }
                };
                let _ = tx_clone.send(turn_event);
            });

        match orchestrator.run_team_task(&task_clone).await {
            Ok(summary) => {
                let _ = tx.send(TurnEvent::TeamComplete(summary));
            }
            Err(e) => {
                let _ = tx.send(TurnEvent::TeamComplete(format!("团队任务执行失败: {}", e)));
            }
        }
    });

    let mut done = false;
    while !done {
        while let Ok(event) = rx.try_recv() {
            if matches!(event, TurnEvent::TeamComplete(_)) {
                done = true;
            }
            apply_turn_event(state, event);
        }
        render_screen(terminal, state)?;
        if !done && event::poll(Duration::from_millis(UI_POLL_MS))? {
            let input_event = event::read()?;
            match handle_busy_event(state, input_event) {
                BusyEventAction::None => {}
                BusyEventAction::Submit(raw) => queue_busy_submission(state, raw).await,
                BusyEventAction::Interrupt => {
                    append_runtime_message(ChatMessage::runtime_display(
                        "team:info",
                        "已中断团队任务。".to_string(),
                    ));
                    done = true;
                }
            }
            render_screen(terminal, state)?;
        }
    }

    flush_queued_busy_inputs(state);
    state.is_busy = false;
    state.team_mode_active = false;
    state.team_status = None;
    state.status = None;
    state.team_orchestrator = None;
    state.pending_team_task = None;
    state.context_tokens_estimate = estimate_context_tokens(&runtime_messages());
    state.transcript_scroll_offset = 0;
    render_screen(terminal, state)?;
    Ok(false)
}

enum GoalCommandAction {
    Start { config: GoalConfig, objective: String },
    ShowList,
    ShowStatus,
    Stop,
    Help,
}

fn parse_goal_args(input: &str) -> GoalCommandAction {
    let mut config = GoalConfig::default();
    let mut objective_parts: Vec<&str> = Vec::new();
    let mut action: Option<GoalCommandAction> = None;

    let args: Vec<&str> = input.split_whitespace().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i] {
            "--yolo" | "--accept-edits" => {
                config.permission_mode = minicode_goal::PermissionMode::AcceptEdits;
            }
            "--auto" | "--dont-ask" => {
                config.permission_mode = minicode_goal::PermissionMode::DontAsk;
            }
            "--plan" => {
                config.permission_mode = minicode_goal::PermissionMode::Plan;
            }
            "--max-hours" => {
                i += 1;
                if i < args.len() {
                    if let Ok(h) = args[i].parse::<f64>() {
                        config.max_duration_secs = (h * 3600.0) as u64;
                    }
                }
            }
            "--max-iters" => {
                i += 1;
                if i < args.len() {
                    if let Ok(n) = args[i].parse::<usize>() {
                        config.max_iterations = n;
                    }
                }
            }
            "--no-git" => {
                config.auto_git_commit = false;
            }
            "--list" => {
                action = Some(GoalCommandAction::ShowList);
            }
            "--status" => {
                action = Some(GoalCommandAction::ShowStatus);
            }
            "--stop" => {
                action = Some(GoalCommandAction::Stop);
            }
            "--help" | "-h" => {
                action = Some(GoalCommandAction::Help);
            }
            other if !other.starts_with("--") => {
                objective_parts.push(other);
            }
            _ => {}
        }
        i += 1;
    }

    if let Some(a) = action {
        return a;
    }

    let objective = objective_parts.join(" ");
    if objective.is_empty() {
        return GoalCommandAction::Help;
    }
    GoalCommandAction::Start { config, objective }
}

fn goal_command_help() -> String {
    "🎯 /goal - 长时自主执行模式\n\n\
     用法:\n\
     \u{0020} /goal <目标描述>          启动goal模式\n\
     \u{0020} /goal --yolo <目标>      自动批准文件编辑\n\
     \u{0020} /goal --auto <目标>      完全自动批准（白名单内）\n\
     \u{0020} /goal --max-hours 8 <目标> 设置最大时长（小时）\n\
     \u{0020} /goal --max-iters 500 <目标> 设置最大迭代轮次\n\
     \u{0020} /goal --list            列出最近的goals\n\
     \u{0020} /goal --status          查看当前goal状态\n\
     \u{0020} /goal --stop            停止当前goal\n\
     \u{0020} /goal --help            显示此帮助\n\n\
     示例:\n\
     \u{0020} /goal 将项目错误处理统一迁移到anyhow并跑通cargo check\n\
     \u{0020} /goal --max-hours 2 为 HTTP API 添加完整集成测试"
        .to_string()
}

/// 从最近的 assistant 消息中解析 [DONE:] 或 [BLOCKED:] 标记
fn parse_goal_result_from_last_message() -> (bool, bool, String) {
    let msgs = runtime_messages();
    for msg in msgs.iter().rev() {
        if let ChatMessage::Assistant { content } = msg {
            let c = content.trim();
            if c.to_uppercase().starts_with("[DONE:") {
                let summary = c.strip_prefix("[DONE:").unwrap_or(c).trim_start_matches(':').trim().trim_end_matches(']').to_string();
                return (true, true, summary);
            }
            if c.to_uppercase().starts_with("[DONE]") {
                let summary = c.strip_prefix("[DONE]").unwrap_or("").trim().to_string();
                return (true, true, if summary.is_empty() { "任务完成".to_string() } else { summary });
            }
            if c.to_uppercase().starts_with("[BLOCKED:") {
                let reason = c.strip_prefix("[BLOCKED:").unwrap_or(c).trim_start_matches(':').trim().trim_end_matches(']').to_string();
                return (true, false, reason);
            }
            if c.to_uppercase().starts_with("[BLOCKED]") {
                let reason = c.strip_prefix("[BLOCKED]").unwrap_or("需要用户介入").trim().to_string();
                return (true, false, reason);
            }
            if !c.is_empty() {
                let lower = c.to_lowercase();
                if (lower.contains("i've completed") || lower.contains("task completed") ||
                    lower.contains("任务完成") || lower.contains("done.") || lower.contains("已完成"))
                    && c.len() < 2000
                {
                    return (true, true, c.chars().take(300).collect());
                }
                return (false, true, c.chars().take(300).collect());
            }
        }
    }
    (false, true, "agent returned empty response".to_string())
}

fn inject_goal_system_goal_state(runner: &GoalRunner) {
    let suffix = runner.format_system_prompt_suffix();
    let arc = minicode_history::get_messages();
    let mut guard = match arc.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    // Remove any existing GoalState system message to avoid stacking
    guard.retain(|m| !matches!(m, ChatMessage::Minicode { content } if content.starts_with("## Goal")));
    guard.push(ChatMessage::Minicode {
        content: suffix,
    });
    drop(guard);
}

async fn handle_goal_command(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &mut ScreenState,
    input: &str,
) -> Result<bool> {
    append_runtime_message(ChatMessage::runtime_display("command", format!("> {}", input)));

    if state.goal_mode_active {
        append_runtime_message(ChatMessage::runtime_display(
            "goal:info",
            "已有goal在运行中。使用 /goal --stop 先停止当前goal，或 /goal --status 查看进度。".to_string(),
        ));
        return Ok(false);
    }

    let action = parse_goal_args(input);

    match action {
        GoalCommandAction::Help => {
            append_runtime_message(ChatMessage::runtime_display(
                "command:result",
                goal_command_help(),
            ));
            return Ok(false);
        }
        GoalCommandAction::ShowList => {
            let goals = minicode_goal::list_recent_goals(10).unwrap_or_default();
            if goals.is_empty() {
                append_runtime_message(ChatMessage::runtime_display(
                    "goal:info",
                    "没有历史goal记录。".to_string(),
                ));
            } else {
                let mut list = String::from("📋 最近的 Goals:\n");
                for g in &goals {
                    let status_icon = match g.status {
                        minicode_goal::GoalStatus::Completed => "✅",
                        minicode_goal::GoalStatus::Running => "🔄",
                        minicode_goal::GoalStatus::Failed(_) => "❌",
                        minicode_goal::GoalStatus::Paused => "⏸️",
                        minicode_goal::GoalStatus::Cancelled => "🚫",
                        minicode_goal::GoalStatus::BudgetExceeded => "⏹️",
                        _ => "📝",
                    };
                    list.push_str(&format!(
                        "{} {} [{:?}] {}/{} - {}\n",
                        status_icon,
                        g.goal_id,
                        g.status,
                        g.completed_count,
                        g.total_todos,
                        g.config.objective.chars().take(60).collect::<String>()
                    ));
                }
                append_runtime_message(ChatMessage::runtime_display("goal:list", list));
            }
            return Ok(false);
        }
        GoalCommandAction::ShowStatus => {
            if let Some(runner) = &state.goal_runner {
                append_runtime_message(ChatMessage::runtime_display(
                    "goal:status",
                    runner.format_progress(),
                ));
            } else {
                append_runtime_message(ChatMessage::runtime_display(
                    "goal:info",
                    "当前没有活动的goal。".to_string(),
                ));
            }
            return Ok(false);
        }
        GoalCommandAction::Stop => {
            if let Some(mut runner) = state.goal_runner.take() {
                runner.cancel();
            }
            state.goal_mode_active = false;
            state.is_busy = false;
            append_runtime_message(ChatMessage::runtime_display(
                "goal:info",
                "已发送取消信号。".to_string(),
            ));
            return Ok(false);
        }
        GoalCommandAction::Start { config, objective } => {
            start_goal_runner(terminal, state, config, objective).await?;
            return Ok(false);
        }
    }
}

async fn start_goal_runner(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &mut ScreenState,
    config: GoalConfig,
    objective: String,
) -> Result<()> {
    let (tx, rx) = mpsc::unbounded_channel::<TurnEvent>();

    let mut runner = GoalRunner::new(&objective, config);

    let event_tx = tx.clone();
    runner = runner.with_callback(move |event: minicode_goal::GoalEvent| {
        let turn_event = match event.kind {
            minicode_goal::GoalEventKind::PlanReady => TurnEvent::GoalPlanReady {
                total: 0,
                plan_text: event.message,
            },
            minicode_goal::GoalEventKind::TaskStart => TurnEvent::GoalTaskStart {
                task_id: event.task_id.unwrap_or_default(),
                title: event.message,
                iteration: event.iteration.unwrap_or(0),
            },
            minicode_goal::GoalEventKind::TaskComplete => TurnEvent::GoalTaskComplete {
                task_id: event.task_id.unwrap_or_default(),
                success: true,
                summary: event.message,
            },
            minicode_goal::GoalEventKind::TaskFailed => TurnEvent::GoalTaskComplete {
                task_id: event.task_id.unwrap_or_default(),
                success: false,
                summary: event.message,
            },
            minicode_goal::GoalEventKind::CheckpointSaved => TurnEvent::GoalCheckpoint {
                iteration: event.iteration.unwrap_or(0),
            },
            minicode_goal::GoalEventKind::Stalled => TurnEvent::GoalStallDetected {
                reason: event.message,
            },
            minicode_goal::GoalEventKind::BudgetWarning => TurnEvent::GoalBudgetWarning(event.message),
            minicode_goal::GoalEventKind::Completed => TurnEvent::GoalComplete {
                summary: event.message,
            },
            minicode_goal::GoalEventKind::Failed => TurnEvent::GoalFailed {
                reason: event.message,
            },
            minicode_goal::GoalEventKind::Cancelled => TurnEvent::GoalCancelled,
            minicode_goal::GoalEventKind::BudgetExceeded => TurnEvent::GoalBudgetExceeded {
                reason: event.message,
            },
            _ => TurnEvent::GoalProgress(event.message),
        };
        let _ = event_tx.send(turn_event);
    });

    runner.start()?;

    let (todos, criteria) = create_rule_based_plan(&objective);
    let total_tasks = todos.len();
    let plan_text = todos.iter()
        .map(|t| format!("  ⬜ {}: {}", t.id, t.title))
        .collect::<Vec<_>>()
        .join("\n");

    runner.set_initial_plan(todos, criteria);

    let _ = tx.send(TurnEvent::GoalStart {
        goal_id: runner.goal_id().to_string(),
        objective: objective.clone(),
    });
    let _ = tx.send(TurnEvent::GoalPlanReady {
        total: total_tasks,
        plan_text,
    });

    state.goal_runner = Some(runner);
    state.goal_mode_active = true;
    state.goal_interrupted = false;
    state.is_busy = true;

    state.goal_runner.as_ref().map(inject_goal_system_goal_state);

    let think_prompt = if let Some(runner) = &mut state.goal_runner {
        if let Some(task) = runner.begin_iteration().cloned() {
            let prompt = runner.get_think_prompt(&task);
            state.goal_current_task_id = Some(task.id.clone());
            let _ = tx.send(TurnEvent::GoalTaskStart {
                task_id: task.id.clone(),
                title: task.title.clone(),
                iteration: runner.state().current_iteration,
            });
            prompt
        } else {
            "Goal started. Please proceed.".to_string()
        }
    } else {
        "Goal started.".to_string()
    };

    append_runtime_message(ChatMessage::User {
        content: format!("[Goal任务指令]\n{}", think_prompt),
    });

    let goal_id_str = state.goal_runner.as_ref().map(|r| r.goal_id().to_string()).unwrap_or_default();
    state.status = Some("Goal模式: 执行中...".to_string());
    render_screen(terminal, state)?;

    drive_goal_turn_loop(terminal, state, tx, rx, goal_id_str).await?;

    Ok(())
}

async fn drive_goal_turn_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &mut ScreenState,
    tx: mpsc::UnboundedSender<TurnEvent>,
    mut rx: mpsc::UnboundedReceiver<TurnEvent>,
    _goal_id: String,
) -> Result<()> {
    let permission_manager = get_permission_manager();

    loop {
        if state.goal_interrupted {
            if let Some(mut runner) = state.goal_runner.take() {
                runner.pause();
            }
            state.goal_mode_active = false;
            state.is_busy = false;
            state.goal_interrupted = false;
            append_runtime_message(ChatMessage::runtime_display(
                "goal:paused",
                "⏸️ Goal已暂停，可稍后继续。".to_string(),
            ));
            state.goal_runner = None;
            break;
        }

        let messages = runtime_messages();
        state.context_tokens_estimate = estimate_context_tokens(&messages);
        permission_manager.begin_turn();
        state.status = Some("Goal: Thinking...".to_string());
        state.stream_text.clear();
        state.stream_frozen = false;

        let (stream_tx, mut stream_rx) = mpsc::unbounded_channel::<(String, bool)>();
        let forward_tx = tx.clone();
        tokio::spawn(async move {
            while let Some((delta, is_final)) = stream_rx.recv().await {
                let _ = forward_tx.send(TurnEvent::StreamDelta(delta, is_final));
            }
        });

        permission_manager.set_prompt_handler(build_prompt_handler(tx.clone())).await;
        let model = get_model_adapter();

        let turn_tx = tx.clone();
        let task = tokio::spawn(async move {
            let mut callbacks = ChannelCallbacks { tx: turn_tx.clone() };
            run_agent_turn_streaming(model.as_ref(), None, Some(&mut callbacks), Some(stream_tx)).await;
            let _ = turn_tx.send(TurnEvent::Done);
        });

        let mut turn_done = false;
        while !turn_done {
            while let Ok(event) = rx.try_recv() {
                let is_done = matches!(event, TurnEvent::Done);
                let is_cancel = matches!(event, TurnEvent::GoalCancelled)
                    || matches!(event, TurnEvent::GoalComplete { .. })
                    || matches!(event, TurnEvent::GoalFailed { .. })
                    || matches!(event, TurnEvent::GoalBudgetExceeded { .. })
                    || matches!(event, TurnEvent::GoalPaused(_));
                apply_turn_event(state, event);
                if is_done || is_cancel {
                    turn_done = true;
                    break;
                }
                render_screen(terminal, state)?;
            }
            render_screen(terminal, state)?;

            if !turn_done && event::poll(Duration::from_millis(UI_POLL_MS))? {
                let input_event = event::read()?;
                match busy_input::handle_busy_event(state, input_event) {
                    busy_input::BusyEventAction::None => {}
                    busy_input::BusyEventAction::Submit(raw) => {
                        queue_busy_submission(state, raw).await;
                    }
                    busy_input::BusyEventAction::Interrupt => {
                        task.abort();
                        append_runtime_message(ChatMessage::runtime_display(
                            "goal:info",
                            "已收到中断信号，Goal将在当前工具完成后暂停。按Ctrl+C再次强制退出。".to_string(),
                        ));
                        state.goal_interrupted = true;
                        turn_done = true;
                    }
                }
                render_screen(terminal, state)?;
            }
        }

        if state.goal_interrupted {
            if let Some(mut runner) = state.goal_runner.take() {
                runner.pause();
                let _ = tx.send(TurnEvent::GoalPaused("用户中断".to_string()));
            }
            state.goal_mode_active = false;
            state.is_busy = false;
            state.goal_interrupted = false;
            break;
        }

        let (found_marker, success, summary) = parse_goal_result_from_last_message();

        let should_stop = if let Some(runner) = &mut state.goal_runner {
            let current_task_id = state.goal_current_task_id.clone().unwrap_or_else(|| "unknown".to_string());

            let tokens_est = state.context_tokens_estimate;
            runner.record_task_result(&current_task_id, success, &summary, tokens_est / 20);

            if let Some(warn) = runner.budget.check_warning() {
                let _ = tx.send(TurnEvent::GoalBudgetWarning(format!("{:?}", warn)));
            }

            let mut stop = false;
            if runner.is_finished() {
                let _ = tx.send(TurnEvent::GoalComplete {
                    summary: runner.format_progress(),
                });
                stop = true;
            } else if !found_marker && success {
                runner.state.add_reflection("Agent did not mark task as done explicitly; assuming progress made but continuing.".to_string());
            }

            if !stop && runner.all_todos_done() {
                runner.complete_goal("所有任务已完成");
                let _ = tx.send(TurnEvent::GoalComplete {
                    summary: runner.format_progress(),
                });
                stop = true;
            }
            if !stop {
                if let Some(exceeded) = runner.budget.check_exceeded() {
                    runner.state.status = minicode_goal::GoalStatus::BudgetExceeded;
                    let _ = runner.persist();
                    let _ = tx.send(TurnEvent::GoalBudgetExceeded {
                        reason: format!("{}", exceeded),
                    });
                    stop = true;
                }
            }
            stop
        } else {
            true
        };

        if should_stop {
            state.goal_mode_active = false;
            state.is_busy = false;
            state.goal_runner = None;
            state.goal_current_task_id = None;
            state.status = None;
            state.context_tokens_estimate = estimate_context_tokens(&runtime_messages());
            render_screen(terminal, state)?;
            break;
        }

        if let Some(runner) = &state.goal_runner {
            inject_goal_system_goal_state(runner);
        }

        if let Some(runner) = &mut state.goal_runner {
            if let Some(next_task) = runner.begin_iteration().cloned() {
                let prompt = runner.get_think_prompt(&next_task);
                state.goal_current_task_id = Some(next_task.id.clone());
                let _ = tx.send(TurnEvent::GoalTaskStart {
                    task_id: next_task.id.clone(),
                    title: next_task.title.clone(),
                    iteration: runner.state().current_iteration,
                });
                append_runtime_message(ChatMessage::User {
                    content: format!("[Goal续轮]\n{}", prompt),
                });
            } else {
                runner.complete_goal("所有可执行任务已处理");
                let _ = tx.send(TurnEvent::GoalComplete {
                    summary: runner.format_progress(),
                });
                continue;
            }
        }

        state.status = Some("Goal: Thinking...".to_string());
        state.stream_text.clear();
        state.stream_frozen = false;
        render_screen(terminal, state)?;
    }

    state.is_busy = false;
    state.status = None;
    state.context_tokens_estimate = estimate_context_tokens(&runtime_messages());
    state.transcript_scroll_offset = 0;
    render_screen(terminal, state)?;
    Ok(())
}

/// 处理用户提交：本地命令、快捷工具或模型回合。
pub(crate) async fn handle_submit(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &mut ScreenState,
    raw_input: String,
) -> Result<bool> {
    let permission_manager = get_permission_manager();
    let input = raw_input.trim().to_string();
    if input.is_empty() {
        return Ok(false);
    }
    if input == "/exit" {
        return Ok(true);
    }

    // 团队模式：/team:continue 续轮
    if input == "/team:continue" {
        append_runtime_message(ChatMessage::runtime_display(
            "command",
            "> /team:continue".to_string(),
        ));
        append_runtime_message(ChatMessage::runtime_display(
            "team:info",
            "续轮功能将在后续版本支持。当前 /team 执行完毕即结束一轮。".to_string(),
        ));
        return Ok(false);
    }

    // 团队模式：/team <task> 启动团队任务
    if input.starts_with("/team ") {
        let task = input.trim_start_matches("/team ").trim().to_string();
        if task.is_empty() {
            append_runtime_message(ChatMessage::runtime_display(
                "command",
                "> /team".to_string(),
            ));
            append_runtime_message(ChatMessage::runtime_display(
                "command:error",
                "用法: /team <任务描述>。例如: /team 重构 user 模块并添加测试".to_string(),
            ));
            return Ok(false);
        }
        return handle_team_command(terminal, state, task).await;
    }

    // Goal 模式：/goal <objective> 启动长时自主执行
    if input.starts_with("/goal") {
        return handle_goal_command(terminal, state, &input).await;
    }

    // /compact 需要异步执行，避免阻塞 UI
    if input == "/compact" || input.starts_with("/compact ") {
        return handle_compact_command(terminal, state, &input).await;
    }

    if input.starts_with('/') {
        handle_command_submission(state, &input).await;
        return Ok(false);
    }

    if let Some(shortcut) = parse_local_tool_shortcut(&input) {
        append_runtime_message(ChatMessage::runtime_display(
            "command",
            format!("> {input}"),
        ));
        state.is_busy = true;
        state.status = Some(format!("Running {}...", shortcut.tool_name));
        let (tx, mut rx) = mpsc::unbounded_channel::<TurnEvent>();
        permission_manager
            .set_prompt_handler(build_prompt_handler(tx.clone()))
            .await;
        let payload = shortcut.input;
        let tool_name_owned = shortcut.tool_name.to_string();

        let task = tokio::spawn(async move {
            let _ = tx.send(TurnEvent::ToolStart {
                tool_name: tool_name_owned.clone(),
                input: payload.clone(),
            });
            let result = get_tool_registry().execute(&tool_name_owned, payload).await;
            let _ = tx.send(TurnEvent::ToolDone(result));
        });

        let mut tool_done = false;
        while state.is_busy {
            let mut updated = false;
            while let Ok(event) = rx.try_recv() {
                if matches!(event, TurnEvent::ToolDone(_)) {
                    tool_done = true;
                }
                let _ = apply_turn_event(state, event);
                updated = true;
                if tool_done {
                    flush_queued_busy_inputs(state);
                    state.is_busy = false;
                }
            }
            if updated {
                render_screen(terminal, state)?;
            }
            if event::poll(Duration::from_millis(UI_POLL_MS))? {
                let input_event = event::read()?;
                match handle_busy_event(state, input_event) {
                    BusyEventAction::None => {}
                    BusyEventAction::Submit(raw) => queue_busy_submission(state, raw).await,
                    BusyEventAction::Interrupt => {
                        task.abort();
                        append_runtime_message(ChatMessage::runtime_display(
                            "command:error",
                            "已中断当前轮次。",
                        ));
                        state.transcript_scroll_offset = 0;
                        state.is_busy = false;
                    }
                }
                render_screen(terminal, state)?;
            }
        }
        flush_queued_busy_inputs(state);
        return Ok(false);
    }

    let _ = add_history_entry(&input);
    state.history = load_history_entries();
    state.history_index = state.history.len();
    state.history_draft.clear();

    append_runtime_message(ChatMessage::User {
        content: input.clone(),
    });
    let messages = runtime_messages();
    state.context_tokens_estimate = estimate_context_tokens(&messages);

    permission_manager.begin_turn();
    state.status = Some("Thinking...".to_string());
    state.is_busy = true;
    state.stream_text.clear();
    state.stream_frozen = false;

    let (tx, mut rx) = mpsc::unbounded_channel::<TurnEvent>();
    permission_manager
        .set_prompt_handler(build_prompt_handler(tx.clone()))
        .await;
    let model = get_model_adapter();

    let (stream_tx, mut stream_rx) = mpsc::unbounded_channel::<(String, bool)>();
    let forward_tx = tx.clone();
    tokio::spawn(async move {
        while let Some((delta, is_final)) = stream_rx.recv().await {
            let _ = forward_tx.send(TurnEvent::StreamDelta(delta, is_final));
        }
    });
    let mut task = tokio::spawn(async move {
        let mut callbacks = ChannelCallbacks { tx: tx.clone() };
        run_agent_turn_streaming(model.as_ref(), None, Some(&mut callbacks), Some(stream_tx)).await;
        let _ = tx.send(TurnEvent::Done);
    });

    // 循环处理：如果回合结束后有排队输入，自动发起新回合
    loop {
        let mut turn_done = false;
        while !turn_done {
            let mut updated = false;
            while let Ok(event) = rx.try_recv() {
                if matches!(event, TurnEvent::ToolResult { .. }) {
                    flush_queued_busy_inputs(state);
                }
                if apply_turn_event(state, event) {
                    turn_done = true;
                    break;
                }
                updated = true;
            }

            if updated {
                render_screen(terminal, state)?;
            }

            if !turn_done && event::poll(Duration::from_millis(UI_POLL_MS))? {
                let input_event = event::read()?;
                match handle_busy_event(state, input_event) {
                    BusyEventAction::None => {}
                    BusyEventAction::Submit(raw) => queue_busy_submission(state, raw).await,
                    BusyEventAction::Interrupt => {
                        task.abort();
                        append_runtime_message(ChatMessage::runtime_display(
                            "command:error",
                            "已中断当前轮次。",
                        ));
                        state.transcript_scroll_offset = 0;
                        turn_done = true;
                    }
                }
                render_screen(terminal, state)?;
            }
        }
        flush_queued_busy_inputs(state);

        // 回合结束后，若没有排队的新输入则退出循环
        if state.queued_busy_inputs.is_empty() {
            break;
        }
        // 有排队输入，自动发起新回合
        let (new_tx, new_rx) = mpsc::unbounded_channel::<TurnEvent>();
        let (new_stream_tx, new_stream_rx) = mpsc::unbounded_channel::<(String, bool)>();
        let new_forward_tx = new_tx.clone();
        tokio::spawn(async move {
            let mut new_stream_rx = new_stream_rx;
            while let Some((delta, is_final)) = new_stream_rx.recv().await {
                let _ = new_forward_tx.send(TurnEvent::StreamDelta(delta, is_final));
            }
        });
        permission_manager
            .set_prompt_handler(build_prompt_handler(new_tx.clone()))
            .await;
        let new_model = get_model_adapter();
        let new_task = tokio::spawn(async move {
            let mut callbacks = ChannelCallbacks { tx: new_tx.clone() };
            run_agent_turn_streaming(
                new_model.as_ref(),
                None,
                Some(&mut callbacks),
                Some(new_stream_tx),
            )
            .await;
            let _ = new_tx.send(TurnEvent::Done);
        });
        task = new_task;
        rx = new_rx;
        state.status = Some("Thinking...".to_string());
        state.stream_text.clear();
        state.stream_frozen = false;
    }

    let done = runtime_messages();
    state.context_tokens_estimate = estimate_context_tokens(&done);
    permission_manager.end_turn();
    state.is_busy = false;
    state.status = None;
    state.active_tool = None;
    state.pending_approval = None;
    Ok(false)
}
