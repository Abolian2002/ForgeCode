# MiniCode Agent

<p align="center">
  <strong>一个轻量级的终端 AI 编码助手</strong>
</p>

<p align="center">
  基于 Rust 实现的 Claude Code 风格 agent，在终端中提供智能代码编写、文件操作与工作流自动化能力。
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Rust-2024-CE412B?style=flat-square&logo=rust" alt="Rust 2024" />
  <img src="https://img.shields.io/badge/Cargo-Workspace-6E4C1E?style=flat-square&logo=rust" alt="Cargo Workspace" />
  <img src="https://img.shields.io/badge/TUI-Ratatui-1D7A5E?style=flat-square" alt="Ratatui TUI" />
  <img src="https://img.shields.io/badge/License-MIT-yellow?style=flat-square" alt="MIT License" />
</p>

## ✨ 项目简介

MiniCode Agent 是一个面向本地开发工作流的轻量级终端编码助手，灵感来自 Claude Code。它围绕一个简洁的 `model → tool → model` 循环构建：接收用户请求、检查工作区、按需调用工具、在执行危险操作前请求审批，最终在同一个终端会话里返回结果。

整个项目有意保持紧凑，让主控制流、工具模型和 TUI 行为都更容易理解和扩展。

## 🎯 核心特性

### 智能上下文管理
- **三级渐进式压缩**：Microcompact（清理旧工具结果）→ SnipCompact（安全区间剪枝）→ AutoCompact（模型生成摘要），自动管理长会话的 Token 成本
- **跨会话断点恢复**：对话历史通过 TOML 持久化，下次启动可继续上次工作

### 可扩展的 Skill 能力体系
- **二阶段加载机制**：启动时只注入 Skill 概览，模型按需通过 `load_skill` 加载完整工作流
- **多作用域发现**：支持全局级、项目级、Claude 兼容级三层 Skill 目录
- **模块化扩展**：通过 `add` / `remove` 命令即可增减能力，无需修改核心代码

### 多协议 MCP 工具生态
- **三种传输协议自动协商**：Content-Length → Newline-JSON → Streamable HTTP，按能力从高到低回退
- **并行连接启动**：通过 `FuturesUnordered` 同时连接多个 MCP Server
- **热插拔扩展**：MCP 工具自动注册为 `mcp__server__tool` 格式，无需重启即可接入搜索、文件系统等外部能力

### 安全与权限审查
- **多级权限模型**：默认拒绝危险操作，支持"仅本次/本回合/永久"三种授权粒度
- **审批决策持久化**：用户偏好自动学习，减少重复审批
- **AI 风险检测**：内置 prompt 注入防御机制

### 流式用户体验
- **打字机式响应**：基于 SSE 流式事件，实时显示模型思考与回复
- **Token 估算**：底部状态栏实时显示上下文使用率
- **审批弹窗**：危险操作前弹出确认界面，支持 y/n/a/d 等快捷键

## 📦 项目结构

```
MiniCode-rs/
├── apps/minicode/              # 主程序入口
├── crates/
│   ├── minicode-agent-core/    # Agent 循环核心
│   ├── minicode-mcp/           # MCP 协议实现
│   ├── minicode-skills/        # Skill 发现与加载
│   ├── minicode-prompt/        # 系统提示词构建
│   ├── minicode-permissions/   # 权限系统
│   ├── minicode-history/       # 会话持久化
│   ├── minicode-tool/          # 工具注册表
│   ├── minicode-tools-runtime/ # 内置工具实现
│   ├── minicode-tui/           # TUI 渲染
│   └── ...
└── docs/                       # 架构文档
```

## 🚀 安装

### 前置要求
- Rust 2024 edition（建议通过 [rustup](https://rustup.rs) 安装）
- 一个 Anthropic API 密钥（或其他兼容的 LLM 端点）

### 从源码编译

```bash
git clone https://github.com/Abolian2002/minicode-agent.git
cd minicode-agent
cargo build --release
```

编译产物位于 `target/release/minicode`。

### 设置 API 密钥

```bash
export ANTHROPIC_API_KEY="your-api-key"
```

## 🛠️ 快速开始

启动一个新的对话会话：

```bash
./target/release/minicode
```

恢复之前的会话：

```bash
./target/release/minicode --resume
# 或指定会话ID
./target/release/minicode --resume <session_id>
```

## ⌨️ 命令

| 命令 | 说明 |
|------|------|
| `/help` | 显示帮助信息 |
| `/compact` | 手动触发上下文压缩 |
| `/clear` | 清空当前对话 |
| `/history` | 查看历史会话 |
| `/resume <id>` | 切换到指定会话 |
| `/skills` | 查看已加载的 Skill |
| `/mcp` | 查看 MCP Server 状态 |
| `/permissions` | 查看权限配置 |

## 🔧 配置

配置文件位置：
- 用户级：`~/.minicode/settings.toml`
- 项目级：`<project>/.minicode/settings.toml`

示例配置：

```toml
[model]
provider = "anthropic"
name = "claude-3-5-sonnet"
max_tokens = 8192

[permissions]
default_policy = "ask"  # ask | allow | deny
auto_allow_dir = ["src/", "tests/"]

[[mcp_servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
```

## 📚 文档

- [架构说明](docs/ARCHITECTURE.md) — 完整的系统架构与数据流
- [会话存储](docs/jianli.md) — 项目设计说明
- [Claude Code 设计模式](docs/多%20agent架构.md) — 架构思想参考

## 🤝 贡献

欢迎提交 Issue 和 Pull Request！

## 📄 许可证

[MIT License](LICENSE)

## 🙏 致谢

本项目受 [Claude Code](https://www.anthropic.com/claude-code) 设计思想启发，参考了多个开源项目的实现方式。
