# Astral-Code 项目总控记录

最后更新：2026-06-11 13:42 CST

这份文档是 Astral-Code 长线改造的中文 handoff。它的用途不是对外宣传，而是让后续任何一次
compact、睡醒恢复、subagent 接手或人工复盘时，都能迅速知道：我们到底要做什么、哪些边界不能碰、
哪些已经完成、现在停在哪里、下一刀应该切哪里。

## 一句话目标

把 `astral-code` 做成一个新的 provider-neutral coding agent harness：继承 Codex 优秀的工程骨架，
重做模型协议层和模型可见工具 flavor，让国产模型、Anthropic Messages 模型、OpenAI-compatible
模型都能用接近 Claude Code 的工具轨迹顺手工作。

这不是 Codex 兼容升级，也不是 OpenAI Responses 协议外面套一层代理。

## 项目身份

- GitHub 仓库：`oines/astral-code`
- CLI 命令：`astral`
- 用户可见项目名：`astral-code`
- 配置与状态目录：`~/.astral-code`
- 主要环境变量：
  - `ASTRAL_HOME`
  - `ASTRAL_API_KEY`
  - `ASTRAL_BASE_URL`
  - `ASTRAL_EXEC_SERVER_URL`

所有新增用户可见面都应该使用 Astral 命名。内部 crate 名、部分模块名和旧测试名可以暂时保持
`codex-*`，后续单独做机械重命名，避免现在把架构重构和命名重构搅在一起。

## 不做的事

- 不读取 `~/.codex`。
- 不迁移旧 Codex session、auth、config、plugin、skill 数据。
- 不 fallback 到 `CODEX_HOME`、`CODEX_API_KEY`、`CODEX_*` 旧命名空间。
- 不把 OpenAI Responses 当成内部核心真相。
- 不靠外部 100 行 Go/Python 反代劫持协议作为主路线。
- 不为了 Claude Code 风格而破坏 Codex 的 sandbox、PTY、exec-server、approval 或 C/S 架构。
- 不把真实 API key、DeepSeek key 或任何 provider secret 写进仓库、fixture 或文档。

## 需要继承的 Codex 骨架

这些是 Codex 做得非常好的“抗压肉体”，原则上直接继承，除非有明确 bug：

- app-server / app-server-protocol
- exec-server
- UnifiedExecProcessManager
- Environment / ExecBackend 抽象
- PTY 输出缓冲、stdin、terminate、后台任务、长任务持续观察
- Seatbelt / Bwrap / Windows sandbox
- approval engine 与 denied-action retry
- Plan Mode
- Goal Mode
- local compact 与 history reconstruction
- MCP runtime 和 MCP resources
- skills/plugins runtime
- multi-agent v2 runtime
- daemon / C/S 生命周期

尤其要注意：sandbox 和 approval 是核心边界。Claude-ish tool schema 只能改变模型看到的工具形状，
不能绕过原有执行、安全和权限链路。

## 总体架构路线

Astral 的内部方向是“深度重构”，不是“尾端 adapter 改名”。

推荐分层：

1. Provider-neutral Agent IR
   - 表达 messages、tool_use、tool_result、stream delta、usage、stop reason、错误。
   - OpenAI Responses、Anthropic Messages、OpenAI-compatible chat completions 都只映射到这个 IR。

2. Provider adapters
   - Anthropic Messages adapter 成为一等路径。
   - OpenAI-compatible `/v1/chat/completions` adapter 成为一等路径。
   - OpenAI Responses 降级为 legacy/optional adapter。

3. Astral-native tool flavor
   - 模型侧暴露 Claude Code-like 工具名和 schema。
   - Rust 侧实现 Astral 自己的参数类型、handler 和 result formatter。
   - 运行时侧复用 Codex primitives，不绕过 ToolOrchestrator、approval、sandbox、Environment。

4. Host modes
   - Plan Mode 尊重 Codex 的宿主实现，不硬塞 Claude `EnterPlanMode/ExitPlanMode`。
   - Goal Mode 保留 Codex 实现。
   - Compact 暂时保留 Codex local compact。Claude Code 和 Codex 的大方向都接近，当前没有证据需要重写。
   - OpenAI remote compact 应删除或禁用。

## Claude-ish 工具目标

目标不是 100% 复刻 Claude Code 全部官方工具，而是复刻对编程 agent 最关键、最容易命中 SFT 轨迹的
工具形状。

已决定做或已做的核心工具：

- `Bash`
- `Monitor`
- `TaskStop`
- `Read`
- `Write`
- `Edit`
- `Glob`
- `Grep`
- `TodoWrite`
- `Agent`
- `SendMessage`
- `AskUserQuestion`
- `RequestPermissions`
- `ToolSearch`
- `Skill`
- `ListMcpResourcesTool`
- `ReadMcpResourceTool`

暂缓或不做的工具：

- `LSP`：Codex 没有稳定 LSP runtime，暂缓。
- `Cron`：Codex 没有对应 session cron runtime，暂缓。
- `Worktree`：需要清晰隔离 git/worktree 生命周期，暂缓。
- `Team`：Claude experimental agent teams，不是 v1 核心。
- Claude Task v2：先用 Codex Goal/Multi-agent/TaskStop 能力，不追 v2 task 系统。
- `NotebookEdit`：非主线编程 agent 能力。
- `PowerShell`：未来 Windows 可考虑，现在不抢。
- `Workflow`、`RemoteTrigger`、`ScheduleWakeup`、`PushNotification`：暂缓。
- `WebSearch` / `WebFetch`：只有 provider-neutral 搜索实现稳定后再暴露，不能保留 OpenAI-only gating。

## 工具运行时映射

当前设计原则：工具 schema 是新的，handler 是 Astral-native 的，但执行仍走 Codex runtime。

- `Bash`
  - 模型侧：Claude-ish `Bash`。
  - 运行时：映射到 Codex shell execution / UnifiedExec。
  - 保留 timeout、background、stdout/stderr streaming、stdin、terminate、approval 和 sandbox。

- `Monitor`
  - 模型侧：用于持续盯后台命令输出、poll 日志、处理长命令进度。
  - 运行时：映射到 Codex `write_stdin` / unified exec session 观察能力。
  - 这是用户明确喜欢 Codex 的关键体验之一：ffmpeg 等长任务要持续汇报，不要像 Claude Code 那样沉默卡住。

- `TaskStop`
  - 运行时：可以停止 shell task，也可以 interrupt multi-agent task。

- `Read` / `Write` / `Edit` / `Glob` / `Grep`
  - 运行时：复用 Codex filesystem、patch/search/sandbox context。
  - 支持 `environment_id`，允许目标环境路由。

- `TodoWrite`
  - 运行时：映射到 Codex `update_plan`。
  - 注意：Plan Mode 和 TodoWrite 不是一回事。Plan Mode 是长计划、用户批准后执行；TodoWrite 是执行过程中的 checklist / progress state。

- `Agent`
  - 运行时：映射 Codex multi-agent spawn。

- `SendMessage`
  - 运行时：映射 multi-agent v2 messaging/mailbox。

- `AskUserQuestion`
  - 运行时：映射 Codex 用户输入/澄清问题通道。

- `RequestPermissions`
  - 模型侧：让模型知道请求被拦，需要提权。
  - 运行时：仍由 Codex approval/sandbox 决定是否允许。

- `ToolSearch` / `Skill`
  - 运行时：复用现有 skills/plugins/dynamic tools 机制。

- MCP resource tools
  - 保留 MCP，不动核心 runtime。

## 关键源码入口

工具 flavor 相关：

- `codex-rs/tools/src/astral_flavor.rs`
- `codex-rs/core/src/tools/spec_plan.rs`
- `codex-rs/core/src/tools/handlers/astral_bash.rs`
- `codex-rs/core/src/tools/handlers/astral_monitor.rs`
- `codex-rs/core/src/tools/handlers/astral_file_tools.rs`
- `codex-rs/core/src/tools/handlers/astral_todo_write.rs`
- `codex-rs/core/src/tools/handlers/astral_agent.rs`
- `codex-rs/core/src/tools/handlers/astral_send_message.rs`
- `codex-rs/core/src/tools/handlers/astral_task_stop.rs`
- `codex-rs/core/src/tools/handlers/astral_request_permissions.rs`

Auth / OpenAI 控制面相关：

- `codex-rs/login/src/lib.rs`
- `codex-rs/login/src/auth.rs`
- `codex-rs/login/src/login_with_api_key.rs`
- `codex-rs/login/src/logout.rs`
- `codex-rs/cli/src/login.rs`
- `codex-rs/app-server/src/request_processors/account_processor.rs`
- `codex-rs/app-server-protocol/src/protocol/v2/account.rs`
- `codex-rs/app-server/README.md`

Provider / model routing 相关：

- `codex-rs/model-provider-info`
- `codex-rs/model-provider`
- `codex-rs/models-manager`
- `codex-rs/core/src/client.rs`
- `codex-rs/core/src/model_family.rs`
- `codex-rs/core/src/config*`

Remote / cloud control-plane 风险区：

- `codex-rs/backend-client`
- `codex-rs/cloud-config`
- `codex-rs/cloud-tasks`
- `codex-rs/core-plugins/src/remote.rs`
- `codex-rs/core-plugins/src/manager.rs`
- `codex-rs/memories/write`

这些风险区默认不要信任。凡是会静默访问 `chatgpt.com/backend-api`、OpenAI hosted backend、ChatGPT
account 或 OpenAI-only plugin 分发的代码，都需要删除、禁用或隔离到显式非默认 feature。

## 已完成的重要提交

- 本轮已完成：停止 app-server account login/logout 触发 ChatGPT remote plugin 刷新
  - `account/login/start` 协议层已经是 API-key-only。
  - `AccountRequestProcessor` 不再持有 `ThreadManager` 或 `ConfigManager`。
  - 登录成功后不再调用 remote installed plugins cache refresh。
  - logout 后不再调用 remote installed plugins cache refresh。
  - 这一步让 Astral 的 account 登录/登出保持 provider-neutral/local auth 行为，不暗中进入
    ChatGPT hosted plugin control-plane。

- `98b8cc01a4 Remove Astral ChatGPT token refresh`
  - 删除 ChatGPT `/oauth/token` refresh flow。
  - 删除 `CODEX_REFRESH_TOKEN_URL_OVERRIDE` / `REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR`。
  - `AuthManager::auth()` 不再主动刷新 ChatGPT token。
  - `AuthManager::refresh_token()` 仅保留 external bearer provider refresh。
  - `UnauthorizedRecovery` 不再包含 `RefreshToken` step。

- `6292fe36df Document Astral handoff state`
  - 补充中文进度文档。

- `a473cb07a4 Remove Astral OAuth login server`
  - 删除旧 OAuth callback server、PKCE、callback HTML assets、login server e2e 测试。

- `8fd0e99080 Make Astral logout local-only`
  - logout 不再调用 OpenAI OAuth revoke，只清理本地 auth。

- `615bf458f7 Reject legacy auth in Astral doctor`
  - `doctor` 不再把 ChatGPT/PAT/Agent Identity 当成可用 Astral 登录态。

- `a22be11e61 Translate Astral progress document`
  - 把项目进度文档切成中文。

- `d5f51c86b9 Document Astral fork progress`
  - 创建初版项目状态文档。

- `7099420969 Remove ChatGPT login entrypoints from CLI`
  - 删除 CLI ChatGPT login 参数：`--with-access-token`、`--device-auth`、
    `--experimental_issuer`、`--experimental_client-id`。
  - 保留 `astral login --with-api-key`、`astral login status`、`astral logout`。

- `7f1e959c0f Remove OpenAI backend routing from providers`
  - 删除 OpenAI/ChatGPT 默认 base URL 路由。
  - 删除 legacy Responses provider 的 OpenAI org/project env headers。
  - 禁用 legacy Responses provider 的 Astral-managed auth 和 websocket 特权。
  - 默认 provider capability 改为 provider-neutral。

- `55c446e0c9 Switch Windows installer to Astral`
  - Windows installer 用户可见 env、path、package name 切换到 Astral。

- `5d511f6440 Let Astral file tools target environments`
  - `Read/Write/Edit/Glob/Grep` 支持 `environment_id`。

- `28382b9678 Switch package entrypoint and installer to Astral`
  - package entrypoint 和 Unix installer 切到 `astral`、`ASTRAL_*`、`~/.astral-code`。

- `f8bd6937b1 Guard Astral tool names in provider-neutral plans`
  - provider-neutral tool planning 里保留 Astral tool name。

- `bf5b06b874 Add Anthropic prompt cache markers`
  - 增加 Anthropic prompt-cache 支持脚手架。

- `c82843b174 Prune noisy directories in Astral file search`
  - 为 Astral file search 剪掉 noisy directories。

- `e53f7253e6 Disable TUI feedback upload flow`
  - 禁用 TUI feedback upload。

- `42e5f7f83e Remove OpenAI rate-limit model nudge`
- `0fd4f2e703 Remove add-credits nudge app-server API`
- `5a9ddcf8d4 Remove add-credits nudge from TUI`
- `b756262d8e Remove add-credits nudge backend client`
  - 删除 OpenAI add-credits / rate-limit upsell 路径。

## 最近完成的 account slice

本轮完成的代码 slice：

> 让 app-server account login/logout 不再触发 ChatGPT remote installed plugins cache refresh。

已编辑文件：

- `codex-rs/app-server/src/request_processors/account_processor.rs`
- `codex-rs/app-server/src/message_processor.rs`

改动内容：

- `AccountRequestProcessor` 不再持有 `ThreadManager`。
- `AccountRequestProcessor` 不再持有 `ConfigManager`。
- 删除 `maybe_refresh_remote_installed_plugins_cache_for_current_config(...)`。
- 删除 `spawn_effective_plugins_changed_task(...)`。
- `send_login_success_notifications(...)` 不再在登录成功后刷新 remote installed plugins cache。
- `logout_common(...)` 不再在 logout 后刷新 remote installed plugins cache。
- `MessageProcessor` 构造 `AccountRequestProcessor` 时不再传入 `thread_manager` 和 `config_manager`。

为什么要做：

- `account/login` 协议层已经是 API-key-only。
- 但 login/logout 后触发的 remote installed plugin refresh 会进入 `core-plugins` remote 路径。
- 该路径最终依赖 ChatGPT auth / hosted plugin control-plane，不适合作为 Astral 的默认 account 行为。
- Astral 的 account 登录应该是 provider-neutral/local auth 行为，不应该暗中触发 OpenAI/ChatGPT 控制面。

注意：不要用宽泛 `just test -p codex-app-server auth` 作为这个 slice 的主要验证，因为它会匹配到
plugin `needsAuth` 测试，那些测试还保留旧 ChatGPT app auth 语义，可能产生与本 slice 无关的失败。

## 已运行验证记录

近期通过的 focused checks：

- `just fmt`
- `just test -p codex-tools astral_flavor`
- `just test -p codex-core astral_file_tools`
- `just test -p codex-model-provider-info`
- `just test -p codex-model-provider configured_provider_uses_default_capabilities`
- `just test -p codex-model-provider configured_provider_models_manager_uses_provider_bearer_token`
- `just test -p codex-models-manager refresh_available_models_fetches_with_provider_auth`
- `just test -p codex-cli login`
- `just test -p codex-cli doctor`
- `just test -p codex-login logout`
- `just test -p codex-login unauthorized_recovery`
- `just test -p codex-login auth_env_telemetry`
- `just test -p codex-app-server suite::auth::get_auth_status_with_api_key`
- `just test -p codex-app-server suite::v2::account::login_account_api_key_succeeds_and_notifies`
- `just test -p codex-app-server suite::v2::account::logout_account_removes_auth_and_notifies`
- 旧 ChatGPT refresh 符号窄范围搜索：`codex-rs/login` 与 app-server auth 测试无命中。
- `git diff --check`

已观察到但暂不处理的问题：

- `just test -p codex-model-provider` 全量仍有 Bedrock catalog 失败，因为 bundled `models.json`
  缺少 `gpt-5.5`。这和当前 Astral provider-neutral cleanup 无关。
- `just test -p codex-app-server auth` 会额外匹配 plugin install/read 的 `needsAuth` 测试，这些测试仍按
  旧 ChatGPT app auth 语义期待 `chatgpt.com/apps/...` 认证项。处理 plugin remote/control-plane 时再改。

## 剩余高优先级工作

1. 审计并清理剩余 OpenAI/ChatGPT auth/config 面
   - `chatgpt_base_url`
   - app-server account docs/tests 中残留的 ChatGPT auth 语义
   - `AuthMode::ChatGPT` / PAT / Agent Identity 是否需要彻底删除、隔离或标记 legacy unsupported

2. 审计 remote/cloud control-plane
   - `backend-client`
   - `cloud-config`
   - `cloud-tasks`
   - `core-plugins/src/remote*`
   - `memories/write`
   - 目标：默认路径不能静默访问 `chatgpt.com/backend-api`。

3. 推进 provider-neutral protocol
   - Anthropic Messages stream/tool_use/tool_result。
   - OpenAI-compatible chat-completions stream/tool_calls。
   - usage、stop reason、error recovery 映射。
   - Responses legacy adapter 去中心化。

4. 硬化 Claude-ish tool result
   - 必要时对照 `/Users/oines/project/claude-code` 源码。
   - 必要时真实跑 Claude Code 抓 fixture。
   - 优先校准 `Bash`、`Monitor`、`Read`、`Edit`、`TodoWrite`、`Agent`、
     `RequestPermissions` 的 schema/result shape。

5. 验证 terminal agentic 体验
   - 后台长命令持续 monitor。
   - y/n prompt 可写 stdin。
   - ffmpeg 等长任务有进度输出。
   - `TaskStop` 可终止。
   - 保持 Codex 比 Claude Code 更丝滑的 terminal 体验。

6. 清理 CI 和 GitHub Actions 噪音
   - 用户曾看到大量 GitHub Actions fail 邮件和 action 消耗。
   - 已经讨论过需要暂时关掉或减少 workflows。
   - 后续如继续推公开仓库，要避免每个 WIP commit 都触发全量 CI。

## 测试策略

用户明确要求：不要把主要时间浪费在测试上，优先推进项目完成；必要时快速过，最后统一测统一修。

因此当前策略是：

- 每次 Rust 编辑后仍跑 `just fmt`。
- 每个 slice 跑最小 focused tests。
- 避免频繁跑 workspace full suite。
- 只有改 shared core/protocol 且风险较高时，再考虑更大范围测试。
- full suite、CI matrix、跨平台修复放到阶段性收敛后处理。
- Rust 命令变慢时耐心等待，不 kill 进程。

## Compact 后恢复指令

如果上下文被压缩，下一个 agent 应该这样恢复：

1. 进入仓库：
   - `cd /Users/oines/project/astral-code`

2. 查看本文件：
   - `sed -n '1,260p' ASTRAL_CODE_PROGRESS.md`

3. 查看当前工作树：
   - `git status --short`
   - `git diff --stat`
   - `git diff -- codex-rs/app-server/src/request_processors/account_processor.rs codex-rs/app-server/src/message_processor.rs`

4. 选择下一块时优先看：
   - `chatgpt_base_url`
   - app-server account docs/tests 旧语义
   - `core-plugins` remote control-plane

5. 不要把 goal 标记为 complete，除非 provider-neutral protocol、Claude-ish tools、OpenAI 控制面清理、
   app-server/account/remote plugin 风险都已经达到可交付状态。

## 安全与风格约束

- 不要编辑 `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` 或 `CODEX_SANDBOX_ENV_VAR` 相关代码。
- 不要削弱 sandbox、approval、exec-server 或 PTY 行为。
- 不要为了让旧测试通过而重新启用 OpenAI/ChatGPT login。
- 不要把旧 Codex 数据兼容路径加回来。
- 不要新增只用一次的小 helper。
- Rust trait 新增时要写清楚职责，不要用 `#[async_trait]` 作为捷径。
- 修改 `ConfigToml` 或嵌套 config type 时要跑 `just write-config-schema`。
- 修改 app-server v2 API 时要遵守 camelCase、TS export、schema fixture 和 README 更新规则。

## 当前目标状态

Goal 仍然 active：

> 完成 astral-code 深度 fork：以全新公开项目形态继承 Codex 边界能力，重构 provider-neutral 协议层与
> Claude-ish 工具 flavor，并移除 OpenAI 专有控制面。

目前还没有达到 complete 条件。当前最重要的是继续清理 OpenAI 专有控制面，同时保护 Codex 的执行骨架。
