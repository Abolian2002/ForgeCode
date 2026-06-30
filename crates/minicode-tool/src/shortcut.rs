use anyhow::Result;

pub struct ToolCommand {
    pub prefix: &'static str,
    pub usage: &'static str,
    pub description: &'static str,
    pub handler: fn(&str) -> Result<LocalToolShortcut>,
}

pub const TOOL_COMMANDS: &[ToolCommand] = &[
    ToolCommand {
        prefix: "/cmd",
        usage: "/cmd [cwd::]<command> [args...]",
        description: "直接执行允许列表内命令。",
        handler: |input| {
            if let Some((cwd, cmd)) = input.split_once("::") {
                if cmd.trim().is_empty() {
                    return Err(anyhow::anyhow!("Command is required."));
                }
                return Ok(LocalToolShortcut {
                    tool_name: "run_command",
                    input: serde_json::json!({ "cwd": cwd.trim(), "command": cmd.trim() }),
                });
            }

            if input.trim().is_empty() {
                return Err(anyhow::anyhow!("Command is required."));
            }
            Ok(LocalToolShortcut {
                tool_name: "run_command",
                input: serde_json::json!({ "command": input.trim() }),
            })
        },
    },
    ToolCommand {
        prefix: "/ls",
        usage: "/ls [path]",
        description: "列出目录文件。",
        handler: |input| {
            let path = if input.trim().is_empty() {
                "."
            } else {
                input.trim()
            };
            Ok(LocalToolShortcut {
                tool_name: "list_files",
                input: serde_json::json!({ "path": path }),
            })
        },
    },
    ToolCommand {
        prefix: "/grep",
        usage: "/grep <pattern>::[path]",
        description: "在文件中搜索文本。",
        handler: |input| {
            let parts = input.split("::").collect::<Vec<_>>();
            if parts.len() == 2 {
                if parts[0].trim().is_empty() {
                    return Err(anyhow::anyhow!("Pattern is required."));
                }
                return Ok(LocalToolShortcut {
                    tool_name: "grep_files",
                    input: serde_json::json!({ "pattern": parts[0].trim(), "path": parts[1].trim() }),
                });
            }
            if input.trim().is_empty() {
                return Err(anyhow::anyhow!("Pattern is required."));
            }
            Ok(LocalToolShortcut {
                tool_name: "grep_files",
                input: serde_json::json!({ "pattern": input.trim() }),
            })
        },
    },
    ToolCommand {
        prefix: "/read",
        usage: "/read <path>",
        description: "直接读取文件内容。",
        handler: |input| {
            if input.trim().is_empty() {
                return Err(anyhow::anyhow!("Path is required."));
            }
            Ok(LocalToolShortcut {
                tool_name: "read_file",
                input: serde_json::json!({ "path": input.trim() }),
            })
        },
    },
    ToolCommand {
        prefix: "/write",
        usage: "/write <path>::<content>",
        description: "直接写入文件。",
        handler: |input| {
            let parts = input.splitn(2, "::").collect::<Vec<_>>();
            if parts.len() == 2 {
                return Ok(LocalToolShortcut {
                    tool_name: "write_file",
                    input: serde_json::json!({ "path": parts[0].trim(), "content": parts[1] }),
                });
            }
            Err(anyhow::anyhow!(
                "Invalid format. Usage: /write <path>::<content>"
            ))
        },
    },
    ToolCommand {
        prefix: "/modify",
        usage: "/modify <path>::<content>",
        description: "替换文件内容（可审阅 diff）。",
        handler: |input| {
            let parts = input.splitn(2, "::").collect::<Vec<_>>();
            if parts.len() == 2 {
                return Ok(LocalToolShortcut {
                    tool_name: "modify_file",
                    input: serde_json::json!({ "path": parts[0].trim(), "content": parts[1] }),
                });
            }
            Err(anyhow::anyhow!(
                "Invalid format. Usage: /modify <path>::<content>"
            ))
        },
    },
    ToolCommand {
        prefix: "/patch",
        usage: "/patch <path>::<search1>::<replace1>::...",
        description: "对单文件执行多组替换。",
        handler: |input| {
            let parts = input.split("::").collect::<Vec<_>>();
            if parts.len() >= 3 && parts.len() % 2 == 1 {
                let path = parts[0].trim();
                if path.is_empty() {
                    return Err(anyhow::anyhow!("Path is required."));
                }
                let mut replacements = Vec::new();
                let mut i = 1;
                while i + 1 < parts.len() {
                    replacements.push(serde_json::json!({
                        "search": parts[i],
                        "replace": parts[i + 1],
                    }));
                    i += 2;
                }
                return Ok(LocalToolShortcut {
                    tool_name: "patch_file",
                    input: serde_json::json!({ "path": path, "replacements": replacements }),
                });
            }
            Err(anyhow::anyhow!(
                "Invalid format. Usage: /patch <path>::<search1>::<replace1>::..."
            ))
        },
    },
    ToolCommand {
        prefix: "/edit",
        usage: "/edit <path>::<search>::<replace>",
        description: "按精确文本替换编辑文件。",
        handler: |input| {
            let parts = input.splitn(3, "::").collect::<Vec<_>>();
            if parts.len() == 3 {
                return Ok(LocalToolShortcut {
                    tool_name: "edit_file",
                    input: serde_json::json!({ "path": parts[0].trim(), "search": parts[1], "replace": parts[2] }),
                });
            }
            Err(anyhow::anyhow!(
                "Invalid format. Usage: /edit <path>::<search>::<replace>"
            ))
        },
    },
];

#[derive(Debug, Clone)]
pub struct LocalToolShortcut {
    pub tool_name: &'static str,
    pub input: serde_json::Value,
}

/// 解析本地斜杠命令并映射为内置工具调用。
pub fn parse_local_tool_shortcut(input: &str) -> Option<LocalToolShortcut> {
    for cmd in TOOL_COMMANDS {
        if input.starts_with(cmd.prefix)
            && let Ok(shortcut) = (cmd.handler)(input[cmd.prefix.len()..].trim())
        {
            return Some(shortcut);
        }
    }
    None
}
