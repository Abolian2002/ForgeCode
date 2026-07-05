# ForgeCode

<p align="center">
  <strong>一个类 Claude Code 的本地终端 Coding Agent</strong>
</p>

<p align="center">
  基于 Rust 实现，支持代码仓库理解、文件操作、命令执行、权限审批、上下文压缩、多 Agent 团队协作与长时目标执行。
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Rust-2024-CE412B?style=flat-square&logo=rust" alt="Rust 2024" />
  <img src="https://img.shields.io/badge/Cargo-Workspace-6E4C1E?style=flat-square&logo=rust" alt="Cargo Workspace" />
  <img src="https://img.shields.io/badge/TUI-Ratatui-1D7A5E?style=flat-square" alt="Ratatui TUI" />
  <img src="https://img.shields.io/badge/License-MIT-yellow?style=flat-square" alt="MIT License" />
</p>

## ✨ 项目简介

ForgeCode 是一个面向本地开发工作流的终端 Coding Agent，参考 Claude Code 的交互范式，但在此基础上扩展了多 Agent 团队模式和长时自主目标模式。它不是普通聊天机器人，而是能在真实代码仓库中搜索文件、理解上下文、修改代码、运行命令、根据反馈继续修正的工程型 Agent。

核心执行方式围绕 `model → tool → model` 闭环展开：模型理解任务并决定下一步工具调用，工具返回真实环境反馈后，模型继续推理、修改和验证，直到完成任务或需要用户介入。

## 🎯 核心特性

### 类 Claude Code 的终端 Agent 闭环
- **仓库级代码理解**：支持文件搜索、内容读取、上下文整理和跨文件分析
- **真实环境操作**：可执行文件修改、命令运行、测试验证等本地开发动作
- **多轮反馈修复**：根据命令输出、错误日志和工具结果继续调整方案

### `/team` 多 Agent 团队模式
- **主 Agent 编排**：主 Agent 负责分析任务、拆解阶段、调度子 Agent、汇总结果
- **任务驱动子 Agent**：每个子 Agent 面向一个具体子任务动态执行，而不是固定死角色
- **上下文隔离**：子 Agent 独立处理局部任务，降低主上下文噪声
- **适用场景**：复杂重构、实现 + 测试 + 审查、多模块协作任务

### `/goal` 长时自主执行模式
- **目标驱动**：用户给出高层目标后，Agent 自动拆解 todo 并持续推进
- **持续循环**：围绕 Think → Act → Observe → Reflect 的过程自主执行多轮任务
- **进度持久化**：保存 goal 状态、任务进度和检查点，便于恢复和追踪
- **风险控制**：通过时间、轮次、连续失败、停滞检测等机制避免失控执行

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
ForgeCode/
├── apps/minicode/              # 主程序入口
├── crates/
│   ├── minicode-agent-core/    # Agent 循环核心
│   ├── minicode-team/          # /team 多 Agent 团队模式
│   ├── minicode-goal/          # /goal 长时目标执行模式
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
git clone https://github.com/Abolian2002/ForgeCode.git
cd ForgeCode
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
| `/team <task>` | 启动多 Agent 团队协作模式，适合复杂任务拆分、并行处理与结果汇总 |
| `/goal <objective>` | 启动长时自主目标模式，适合多轮执行、验证和持续推进的大目标 |
| `/goal --status` | 查看当前 goal 进度 |
| `/goal --stop` | 停止当前 goal |

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
- [多 Agent 团队模式设计](docs/多Agent团队模式设计.md) — `/team` 模式设计说明
- [team 流程与面试话术](docs/team流程与面试话术.md) — `/team` 流程说明与面试表达
- [goal 长时自主执行模式设计](docs/goal长时自主执行模式设计.md) — `/goal` 模式设计说明
- [goal 流程与面试话术](docs/goal流程与面试话术.md) — `/goal` 流程说明与面试表达
- [SWE-bench 评测流程](docs/SWE-bench评测mini-code流程.md) — 使用 SWE-bench 评测 Agent 的流程说明
- [Claude Code 设计模式](docs/多%20agent架构.md) — 架构思想参考

## 🤝 贡献

欢迎提交 Issue 和 Pull Request！

## 📄 许可证

[MIT License](LICENSE)

## 🙏 致谢

本项目受 [Claude Code](https://www.anthropic.com/claude-code) 设计思想启发，参考了多个开源项目的实现方式，并在此基础上探索多 Agent 协作与长时自主执行能力。
