use minicode_history::append_runtime_message;
use minicode_types::ChatMessage;

use crate::state::{PendingApproval, PendingAskUser, ScreenState, TurnEvent};

/// 为工具输入生成便于展示的简短摘要。
fn summarize_tool_input(tool_name: &str, input: &serde_json::Value) -> String {
    if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
        return format!("{} path={}", tool_name, path);
    }
    if let Some(command) = input.get("command").and_then(|v| v.as_str()) {
        return format!("{} {}", tool_name, command);
    }
    serde_json::to_string(input).unwrap_or_else(|_| "(invalid input)".to_string())
}

/// 应用单个回合事件到 UI 状态，必要时返回新消息列表。
pub(crate) fn apply_turn_event(state: &mut ScreenState, event: TurnEvent) -> bool {
    match event {
        TurnEvent::ToolStart { tool_name, input } => {
            state.stream_text.clear();
            state.stream_frozen = true;
            state.active_tool = Some(tool_name.clone());
            state.status = Some(format!("Running {tool_name}..."));
            let _ = summarize_tool_input(&tool_name, &input);
            false
        }
        TurnEvent::ToolResult {
            tool_name,
            output,
            is_error,
        } => {
            state.recent_tools.push((tool_name, !is_error));
            let _ = output;
            false
        }
        TurnEvent::Assistant(content) => {
            state.stream_text.clear(); // 完整消息已入库后再到这里，清除流式残影
            state.stream_frozen = true;
            let _ = content;
            false
        }
        TurnEvent::Progress(content) => {
            let _ = content;
            false
        }
        TurnEvent::Approval { request, responder } => {
            state.pending_approval = Some(PendingApproval {
                request,
                responder: Some(responder),
                selected_index: 0,
                awaiting_feedback: false,
                feedback: String::new(),
            });
            state.status = Some("Approval required...".to_string());
            false
        }
        TurnEvent::Status(text) => {
            state.status = Some(text);
            false
        }
        TurnEvent::AskUserPrompt { question, options } => {
            state.pending_ask_user = Some(PendingAskUser {
                question,
                options,
                selected_index: 0,
            });
            state.status = Some("Ask user...".to_string());
            false
        }
        TurnEvent::StreamDelta(delta, is_final) => {
            if state.stream_frozen {
                return false;
            }
            if is_final {
                // final 只表示流结束，不在这里清空，避免“先回退再出现最终文本”
            } else {
                // 兼容两类 provider：
                // 1) delta 增量；2) cumulative 全量片段。
                if state.stream_text.is_empty() {
                    state.stream_text.push_str(&delta);
                } else if delta.starts_with(&state.stream_text) {
                    state.stream_text = delta;
                } else {
                    state.stream_text.push_str(&delta);
                }
            }
            false
        }
        TurnEvent::ToolDone(result) => {
            state.recent_tools.push((
                state
                    .active_tool
                    .clone()
                    .unwrap_or_else(|| "tool".to_string()),
                result.ok,
            ));
            let kind = if result.ok {
                "command:result"
            } else {
                "command:error"
            };
            append_runtime_message(ChatMessage::runtime_display(kind, result.output));
            state.active_tool = None;
            state.status = None;
            false
        }
        TurnEvent::Done => {
            state.stream_text.clear();
            state.stream_frozen = true;
            true
        }
        TurnEvent::TeamPhaseStart { phase, total } => {
            state.team_status = Some(format!("团队模式：阶段 {}/{}", phase, total));
            state.status = Some(format!("团队执行中 - 阶段 {}/{}", phase, total));
            append_runtime_message(ChatMessage::runtime_display(
                "team:phase",
                format!("─── 阶段 {} / {} ───", phase, total),
            ));
            false
        }
        TurnEvent::TeamSubTaskStart {
            task_id,
            name,
            description,
        } => {
            state.status = Some(format!("[{}] {} 执行中...", task_id, name));
            append_runtime_message(ChatMessage::runtime_display(
                "team:subtask:start",
                format!("▶ [{}] {}: {}", task_id, name, description),
            ));
            false
        }
        TurnEvent::TeamSubTaskComplete {
            task_id,
            name,
            success,
            summary,
        } => {
            let icon = if success { "✅" } else { "❌" };
            let detail: String = summary.chars().take(120).collect();
            append_runtime_message(ChatMessage::runtime_display(
                "team:subtask:done",
                format!("{} [{}] {} - {}", icon, task_id, name, detail),
            ));
            false
        }
        TurnEvent::TeamProgress(msg) => {
            append_runtime_message(ChatMessage::runtime_display("team:progress", msg));
            false
        }
        TurnEvent::TeamComplete(summary) => {
            append_runtime_message(ChatMessage::runtime_display("team:complete", summary));
            state.team_mode_active = false;
            state.team_status = None;
            state.status = None;
            state.is_busy = false;
            true
        }
        TurnEvent::GoalStart { goal_id, objective } => {
            append_runtime_message(ChatMessage::runtime_display(
                "goal:start",
                format!("> /goal {}\n\n🎯 Goal 启动，ID: {}\n正在分析目标并制定初始计划...", objective, goal_id),
            ));
            state.status = Some("Goal模式: 规划中...".to_string());
            false
        }
        TurnEvent::GoalPlanReady { total, plan_text } => {
            append_runtime_message(ChatMessage::runtime_display(
                "goal:plan",
                format!("📋 初始计划已生成，共 {} 个子任务\n\n{}", total, plan_text),
            ));
            state.status = Some("Goal模式: 执行中...".to_string());
            false
        }
        TurnEvent::GoalTaskStart { task_id, title, iteration } => {
            state.goal_current_task_id = Some(task_id.clone());
            state.status = Some(format!("Goal [{}] {} (iter {})", task_id, title, iteration));
            append_runtime_message(ChatMessage::runtime_display(
                "goal:task:start",
                format!("▶ [{}] {}", task_id, title),
            ));
            false
        }
        TurnEvent::GoalTaskComplete { task_id, success, summary } => {
            let icon = if success { "✅" } else { "❌" };
            let detail: String = summary.chars().take(200).collect();
            append_runtime_message(ChatMessage::runtime_display(
                if success { "goal:task:done" } else { "goal:task:fail" },
                format!("{} [{}] {}", icon, task_id, detail),
            ));
            state.goal_current_task_id = None;
            false
        }
        TurnEvent::GoalProgress(msg) => {
            append_runtime_message(ChatMessage::runtime_display("goal:progress", msg));
            false
        }
        TurnEvent::GoalCheckpoint { iteration } => {
            append_runtime_message(ChatMessage::runtime_display(
                "goal:checkpoint",
                format!("💾 检查点已保存（迭代 {}）", iteration),
            ));
            false
        }
        TurnEvent::GoalStallDetected { reason } => {
            append_runtime_message(ChatMessage::runtime_display(
                "goal:stall",
                format!("🐛 检测到停滞: {}，正在调整策略...", reason),
            ));
            false
        }
        TurnEvent::GoalBudgetWarning(warn) => {
            append_runtime_message(ChatMessage::runtime_display("goal:warning", format!("⚠️ {}", warn)));
            false
        }
        TurnEvent::GoalComplete { summary } => {
            append_runtime_message(ChatMessage::runtime_display("goal:complete", format!("🎉 Goal 达成！\n\n{}", summary)));
            state.goal_mode_active = false;
            state.goal_runner = None;
            state.goal_current_task_id = None;
            state.goal_interrupted = false;
            state.status = None;
            state.is_busy = false;
            true
        }
        TurnEvent::GoalFailed { reason } => {
            append_runtime_message(ChatMessage::runtime_display("goal:failed", format!("💥 Goal 失败: {}", reason)));
            state.goal_mode_active = false;
            state.goal_runner = None;
            state.goal_current_task_id = None;
            state.goal_interrupted = false;
            state.status = None;
            state.is_busy = false;
            true
        }
        TurnEvent::GoalPaused(msg) => {
            append_runtime_message(ChatMessage::runtime_display("goal:paused", format!("⏸️ {}", msg)));
            state.goal_mode_active = false;
            state.status = None;
            state.is_busy = false;
            true
        }
        TurnEvent::GoalCancelled => {
            append_runtime_message(ChatMessage::runtime_display("goal:cancelled", "🚫 Goal 已取消".to_string()));
            state.goal_mode_active = false;
            state.goal_runner = None;
            state.goal_current_task_id = None;
            state.goal_interrupted = false;
            state.status = None;
            state.is_busy = false;
            true
        }
        TurnEvent::GoalBudgetExceeded { reason } => {
            append_runtime_message(ChatMessage::runtime_display("goal:budget", format!("⏹️ 预算耗尽: {}\n已保存检查点，可稍后 /goal --resume 恢复", reason)));
            state.goal_mode_active = false;
            state.status = None;
            state.is_busy = false;
            true
        }
    }
}
