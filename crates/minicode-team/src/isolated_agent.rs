use std::collections::HashSet;
use minicode_tool::{ToolResult, get_tool_registry};
use minicode_types::{AgentStep, ChatMessage, ToolCall, get_model_adapter};
use serde_json::Value;
use crate::types::{CommandRecord, SubAgentConfig, SubTaskResult};

#[allow(dead_code)]
struct IsolatedContext {
    messages: Vec<ChatMessage>,
    system_prompt: String,
    allowed_tools: HashSet<String>,
    working_dir: Option<std::path::PathBuf>,
    max_steps: usize,
    modified_files: HashSet<std::path::PathBuf>,
    commands_executed: Vec<CommandRecord>,
    issues: Vec<String>,
}

impl IsolatedContext {
    fn new(config: &SubAgentConfig) -> Self {
        let system_prompt = if config.system_prompt.is_empty() {
            "You are a helpful coding assistant.".to_string()
        } else {
            config.system_prompt.clone()
        };
        Self {
            messages: vec![ChatMessage::System {
                content: system_prompt.clone(),
            }],
            system_prompt,
            allowed_tools: config.tools.iter().cloned().collect(),
            working_dir: config.working_dir.clone(),
            max_steps: config.max_steps,
            modified_files: HashSet::new(),
            commands_executed: Vec::new(),
            issues: Vec::new(),
        }
    }

    fn messages_for_model(&self) -> Vec<ChatMessage> {
        self.messages
            .iter()
            .filter(|m| m.should_include_in_context())
            .cloned()
            .collect()
    }

    fn add_message(&mut self, msg: ChatMessage) {
        self.messages.push(msg);
    }

    fn track_file_modification(&mut self, tool_name: &str, input: &Value) {
        let path_key = match tool_name {
            "edit_file" | "modify_file" | "write_file" | "patch_file" => Some("path"),
            _ => None,
        };
        if let Some(key) = path_key {
            if let Some(path) = input.get(key).and_then(|v| v.as_str()) {
                let p = std::path::PathBuf::from(path);
                if p.exists() || tool_name == "write_file" {
                    self.modified_files.insert(p);
                }
            }
        }
    }

    fn track_command(&mut self, input: &Value, result: &ToolResult) {
        if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
            self.commands_executed.push(CommandRecord {
                command: cmd.to_string(),
                success: result.ok,
            });
        }
    }

    async fn execute_tool(&mut self, tool_name: &str, input: Value) -> ToolResult {
        if !self.allowed_tools.is_empty() && !self.allowed_tools.contains(tool_name) {
            return ToolResult::err(format!(
                "Tool '{}' is not allowed for this sub-agent. Allowed tools: {}",
                tool_name,
                self.allowed_tools.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
        }

        if tool_name == "ask_user" {
            return ToolResult::err(
                "Sub-agents cannot ask_user. Complete the task autonomously or report issues in your final response."
                    .to_string(),
            );
        }

        if tool_name == "load_skill" {
            return ToolResult::err(
                "Sub-agents cannot load_skill. Use built-in tools only.".to_string(),
            );
        }

        self.track_file_modification(tool_name, &input);

        let registry = get_tool_registry();
        let result = registry.execute(tool_name, input.clone()).await;

        if tool_name == "run_command" {
            self.track_command(&input, &result);
        }

        result
    }

    fn get_final_message(&self) -> Option<String> {
        self.messages
            .iter()
            .rev()
            .find_map(|m| match m {
                ChatMessage::Assistant { content } => Some(content.clone()),
                _ => None,
            })
    }
}

pub async fn run_isolated_subagent(
    config: &SubAgentConfig,
    task_description: &str,
) -> SubTaskResult {
    let model = get_model_adapter();
    let mut ctx = IsolatedContext::new(config);

    let user_prompt = format!(
        "{}\n\n---\n任务名称：{}\n任务描述：{}\n\n请立即开始执行任务。完成后给出清晰的总结。",
        config.system_prompt, config.name, task_description
    );
    ctx.add_message(ChatMessage::User {
        content: user_prompt,
    });

    let mut empty_retry = 0usize;
    let mut tool_error_count = 0usize;

    for step in 0..ctx.max_steps {
        let messages = ctx.messages_for_model();

        let next = match model.next(&messages).await {
            Ok(step) => step,
            Err(err) => {
                ctx.issues.push(format!("Model request failed: {}", err));
                return build_result(&ctx, &config.name, false);
            }
        };

        match next {
            AgentStep::Assistant { content, .. } => {
                let is_empty = content.trim().is_empty();

                if is_empty {
                    if empty_retry < 2 {
                        empty_retry += 1;
                        let retry_prompt = "上一条回复为空。请继续执行任务，给出工具调用或最终总结。";
                        ctx.add_message(ChatMessage::Minicode {
                            content: retry_prompt.to_string(),
                        });
                        continue;
                    }
                    ctx.add_message(ChatMessage::Assistant {
                        content: "（子Agent达到空响应限制，停止执行）".to_string(),
                    });
                    break;
                }

                ctx.add_message(ChatMessage::Assistant {
                    content: content.clone(),
                });
                break;
            }
            AgentStep::ToolCalls {
                calls,
                content,
                ..
            } => {
                if let Some(c) = content {
                    if !c.trim().is_empty() {
                        ctx.add_message(ChatMessage::AssistantProgress {
                            content: c.clone(),
                        });
                    }
                }

                if calls.is_empty() {
                    continue;
                }

                if step % 10 == 9 && ctx.messages.len() > 30 {
                    let truncated = truncate_context(&ctx.messages);
                    ctx.messages = truncated;
                }

                for call in calls {
                    let ToolCall {
                        id,
                        tool_name,
                        input,
                    } = call;

                    let result = ctx.execute_tool(&tool_name, input.clone()).await;

                    if !result.ok {
                        tool_error_count += 1;
                    }

                    ctx.add_message(ChatMessage::AssistantToolCall {
                        tool_use_id: id.clone(),
                        tool_name: tool_name.clone(),
                        input: input.clone(),
                    });
                    ctx.add_message(ChatMessage::ToolResult {
                        tool_use_id: id,
                        tool_name,
                        content: result.output.clone(),
                        is_error: !result.ok,
                    });

                    if result.await_user {
                        ctx.issues.push(
                            "Sub-agent requested user input; cannot proceed autonomously."
                                .to_string(),
                        );
                        return build_result(&ctx, &config.name, false);
                    }
                }
            }
        }
    }

    let success = ctx.issues.is_empty() && tool_error_count == 0;
    build_result(&ctx, &config.name, success)
}

fn truncate_context(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut kept = Vec::new();
    let mut skip_old_tool_results = true;

    kept.push(messages.first().unwrap().clone());

    for msg in messages.iter().skip(1) {
        match msg {
            ChatMessage::ToolResult { .. } if skip_old_tool_results => {
                continue;
            }
            ChatMessage::User { .. } | ChatMessage::Assistant { .. } => {
                skip_old_tool_results = false;
                kept.push(msg.clone());
            }
            _ => {
                kept.push(msg.clone());
            }
        }
    }

    if kept.len() == messages.len() {
        return messages.to_vec();
    }
    kept.insert(
        1,
        ChatMessage::ContextSummary {
            content: "[早期工具结果已被截断以节省上下文窗口]".to_string(),
        },
    );
    kept
}

fn build_result(ctx: &IsolatedContext, name: &str, success: bool) -> SubTaskResult {
    let summary = ctx
        .get_final_message()
        .unwrap_or_else(|| "（无最终总结）".to_string());

    SubTaskResult {
        task_id: String::new(),
        name: name.to_string(),
        success,
        modified_files: ctx.modified_files.iter().cloned().collect(),
        commands_executed: ctx.commands_executed.clone(),
        issues: ctx.issues.clone(),
        summary,
        final_message: ctx.get_final_message(),
    }
}
