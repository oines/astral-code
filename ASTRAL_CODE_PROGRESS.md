# Astral-Code 项目总控记录

最后更新：2026-06-11 18:45 CST

这份文档是 Astral-Code 长线改造的中文 handoff。它的用途不是对外宣传，而是让后续任何一次
compact、睡醒恢复、subagent 接手或人工复盘时，都能迅速知道：我们到底要做什么、为什么这么做、
哪些边界不能碰、哪些已经完成、现在停在哪里、下一刀应该切哪里。

文档必须保持中文。可以保留源码符号、路径、commit hash、API 名和工具名的英文原文，但解释、进度、
取舍和恢复指令都应使用中文。

## 一句话目标

把 `astral-code` 做成一个新的 provider-neutral coding agent harness：继承 Codex 优秀的工程骨架，
重做模型协议层和模型可见工具 flavor，让国产模型、Anthropic Messages 模型、OpenAI-compatible
模型都能用接近 Claude Code 的工具轨迹顺手工作。

这不是 Codex 兼容升级，也不是 OpenAI Responses 协议外面套一层代理。

## 当前整体进度快照

当前状态：项目已经从“方案评估”进入“深度 fork 实施中期”。命名空间、CLI、若干核心工具 flavor、
provider 去 OpenAI 默认路由、登录态清理、cloud-config/cloud-tasks 控制面拆除等关键 slice 已落地。

粗略完成度判断：

- 项目身份与命名：基本完成。
- `~/.astral-code` / `ASTRAL_*` 命名空间：主路径已完成，仍需继续扫尾旧测试和边缘文案。
- Claude-ish tool flavor：核心文件和 handler 已落地一批，仍需继续校准 schema/result shape。
- terminal 执行体验：继承 Codex 的 PTY / UnifiedExec / exec-server，不应重写。
- Plan Mode / Goal Mode / local compact：决定保留 Codex 方案，当前不作为重构主战场。
- OpenAI 登录态和 ChatGPT OAuth：主路径已删除或禁用。
- OpenAI/ChatGPT hosted control-plane：已经拆掉多处默认外联，但仍是最高优先级扫尾区域。
- Provider-neutral Agent IR / Anthropic Messages / OpenAI-compatible chat completions：主干已存在，仍需继续补齐
  国产模型兼容细节、fixture 和端到端测试。
- 全量 CI：当前不追求全绿，用户明确要求先推进实现，最后集中测试集中修。

当前最新 slice：参考本机 `/Users/oines/project/cc-switch` 的 provider/proxy 兼容逻辑后，收敛了
Astral 原生 Chat Completions adapter 的请求形状：`developer` 消息不再发给上游，而是与其他
`system` 消息合并成首条 `system`；当最终请求没有非空 `tools` 时，会移除 `tool_choice` 和
`parallel_tool_calls`。这样更贴近国内 OpenAI-compatible 网关的保守兼容面，同时保持这是 Astral
原生 adapter 逻辑，不是外挂反代。

下一步优先继续处理底层 `app-server-transport` remote-control 旧模块、`chatgpt_base_url` 配置字段和其他
ChatGPT hosted 残留；provider adapter 方向则继续补 Anthropic/chat-completions fixture 和国内模型兼容选项。
当前明确口径：标准 OpenAI API 指 `/v1/chat/completions`，不是旧 `/v1/completions`；Anthropic 路线是
Messages API，`base_url` 可以承载 `/v1` 或 `/anthropic/v1` 这类网关前缀。

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

## 用户真实诉求

用户不是想做一个“小修小补的 Codex 皮肤”，而是要一个新的公开项目 `astral-code`：

- 继承 Codex 的强工程骨架：daemon、app-server、exec-server、PTY、sandbox、approval、MCP、
  skills/plugins、multi-agent、Goal Mode、Plan Mode 和 local compact。
- 删除 OpenAI/ChatGPT 专有控制面：登录、遥测、feedback、remote compact、workspace settings、
  connector directory、cloud-config policy fetch、OpenAI hosted backend 默认路由等。
- 重做模型协议：把 OpenAI Responses 从核心真相降级，改成 provider-neutral 内部 IR，再支持
  Anthropic Messages 和 OpenAI-compatible chat completions。
- 重做模型可见工具：不是把旧 Codex 工具包一层改名 adapter，而是实现 Astral-native 的
  Claude-ish schema、参数类型、handler 和 tool_result。
- 重点服务国产模型：例如 DeepSeek v4 pro / flash 一类 OpenAI-compatible 或 Anthropic-ish 模型，
  让它们看到更接近 Claude Code SFT 轨迹的工具形状，从而更顺手地做 agentic coding。
- 不要求 100% 复刻 Claude Code 官方全部工具。原则是：对模型编程能力最关键、Codex runtime 已经有良好
  承载能力的工具优先做；没有 runtime 的工具不要硬造假。

特别重要：用户很看重 Codex 的 terminal agentic 体验。长命令、ffmpeg、卡住的 shell、需要 stdin 的
交互命令，都要保留 Codex 这种持续观察、可写入、可终止的丝滑感。不要为了 Claude Code 外形牺牲这个优势。

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
   - Anthropic Messages adapter 成为一等路径，目标兼容 `/anthropic` / `/v1/messages` 这类国内网关常见
     Anthropic 入口。
   - OpenAI-compatible `/v1/chat/completions` adapter 成为一等路径，这是 DeepSeek 等国产模型最常用的
     OpenAI API 兼容面。
   - 老式 `/v1/completions` 文本补全不是主路径；如果未来确实需要，只能作为极薄降级 adapter，不能反过来
     约束 agent/tool 协议设计。
   - OpenAI Responses 降级为 legacy/optional adapter。

### `cc-switch` 参考结论

已阅读本机 `/Users/oines/project/cc-switch` 的关键代码，尤其是：

- `src-tauri/src/services/model_fetch.rs`
- `src-tauri/src/proxy/providers/transform.rs`
- `src-tauri/src/proxy/providers/transform_codex_chat.rs`
- `src-tauri/src/proxy/providers/streaming_codex_chat.rs`
- `src-tauri/src/proxy/providers/claude.rs`
- `src/config/codexProviderPresets.ts`

可借鉴但不照搬的点：

- `cc-switch` 的路线是“客户端仍说 Claude/Codex 原协议，本地代理再转上游”。Astral 不走这个形态，
  但可以吸收它沉淀出的国内 provider 兼容规则，放进原生 provider adapter。
- OpenAI-compatible Chat Completions 对 strict 网关应尽量朴素：多个 `system` 合并到首条，避免中间
  `system`/`developer`；没有工具时不要发送 `tool_choice` 或 `parallel_tool_calls`；stream 时要显式
  `stream_options.include_usage = true` 才能拿到 usage 尾包。
- 国内 reasoning/thinking 差异需要后续单独建 provider capability：DeepSeek/Kimi/Moonshot/智谱/百炼等
  可能需要不同的 `thinking`、`enable_thinking`、`reasoning_effort`、`reasoning_content` 处理。
- 模型列表发现不能只做 `{base}/models`：`cc-switch` 会根据 `/v1`、`/v4`、`/api/anthropic`、
  `/claudecode`、`/coding` 等兼容路径构造候选 URL。Astral 后续如果做 provider model discovery，可参考
  这个候选生成逻辑。

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

当前实现状态：

- 已有 `codex-rs/tools/src/astral_flavor.rs`，用于集中定义 Astral 工具 flavor。
- 已有 `codex-rs/core/src/tools/astral_tool_bridge.rs`，用于把 Astral 工具名/参数桥接到 Codex runtime。
- 已有 `codex-rs/core/src/tools/handlers/astral_bash.rs`。
- 已有 `codex-rs/core/src/tools/handlers/astral_monitor.rs`。
- 已有 `codex-rs/core/src/tools/handlers/astral_file_tools.rs`。
- 已有 `codex-rs/core/src/tools/handlers/astral_todo_write.rs`。
- 已有 `codex-rs/core/src/tools/handlers/astral_agent.rs`。
- 已有 `codex-rs/core/src/tools/handlers/astral_send_message.rs`。
- 已有 `codex-rs/core/src/tools/handlers/astral_task_stop.rs`。
- 已有 `codex-rs/core/src/tools/handlers/astral_request_permissions.rs`。
- 已有 `codex-rs/core/src/tools/handlers/astral_ask_user_question.rs`。
- 已有 `codex-rs/core/src/tools/handlers/astral_tool_search.rs`。
- 已有 `codex-rs/core/src/tools/handlers/astral_skill.rs`。
- 已有 `codex-rs/core/src/tools/handlers/astral_mcp_resource.rs`。

后续不要误判为“工具还没开始”。真正剩余的是：继续对齐 Claude Code-like schema/result，补 integration
测试，确认所有 handler 都走 Codex 原生 runtime、安全边界和事件链路。

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

- 本轮已完成：backend-client 不再隐式改写 ChatGPT host
  - `Client::new(...)` 只裁剪尾部 `/`，不再把 `https://chatgpt.com` 或
    `https://chat.openai.com` 自动补成 `/backend-api`。
  - `PathStyle::ChatGptApi` 暂时保留，只有调用方显式传入带 `/backend-api` 的 URL 时才使用。
  - 目的：避免 Astral 默认路径因为一个 ChatGPT host 字符串而静默进入 OpenAI hosted WHAM backend。

- 本轮已完成：ChatGPT Cloudflare 全局 cookie store 禁用
  - `codex-rs/codex-client/src/chatgpt_cloudflare_cookies.rs` 保留
    `with_chatgpt_cloudflare_cookie_store(...)` 兼容函数，但函数直接返回原 builder。
  - 旧 process-global `Jar`、Cloudflare cookie allowlist、ChatGPT host cookie 过滤和对应测试已移除。
  - `codex-rs/login/src/auth/default_client.rs` fallback 日志不再提 ChatGPT Cloudflare cookie store。
  - 目的：去掉一个 ChatGPT hosted control-plane 遗留状态源，同时不搅乱现有 reqwest/custom-CA 构造链路。

- 本轮已完成：ChatGPT workspace settings / connector directory 不再联网
  - `codex-rs/chatgpt/src/workspace_settings.rs` 不再请求
    `/accounts/{account_id}/settings`。
  - `codex_plugins_enabled_for_workspace(...)` 现在本地返回 `true`，插件是否启用只由 Astral 本地
    feature/config 决定。
  - `codex-rs/chatgpt/src/connectors.rs` 不再请求 ChatGPT connector directory，也不再要求
    ChatGPT/Codex backend auth。
  - connector list 仍保留 plugin apps 和 MCP accessible connectors；这是去 hosted control-plane，不是砍
    apps/plugins/MCP 骨架。

- 本轮已完成：cloud-config 导出入口不再启动 ChatGPT hosted policy fetch
  - `codex-cloud-config` 的公开 `cloud_config_bundle_loader(...)` 改为返回 no-op
    `CloudConfigBundleLoader::default()`。
  - `cloud_config_bundle_loader_for_storage(...)` 不再创建 shared `AuthManager`，也不再构造远程
    backend client。
  - 删除旧 `BackendBundleClient` 和后台 cache refresh loop。
  - 旧 cloud-config service/backend/cache 只在测试构建中保留，用于现有 bundle 解析、验证和缓存测试。

- 本轮已完成：cloud-tasks 不再默认访问 ChatGPT hosted backend
  - 删除 `codex-rs/cloud-tasks` 中对 `CODEX_CLOUD_TASKS_BASE_URL` 的读取。
  - 新增 `ASTRAL_CLOUD_TASKS_BASE_URL` 作为 cloud tasks 的显式后端配置入口。
  - `init_backend(...)`、TUI environment list、environment autodetect 都改为走统一 helper。
  - 缺少 `ASTRAL_CLOUD_TASKS_BASE_URL` 时返回本地错误，不再 fallback 到
    `https://chatgpt.com/backend-api`。
  - debug mock 模式仍可无后端运行，但只使用 `http://localhost/backend-api` 作为本地占位 URL。

- 本轮已完成：拆除 AuthManager 对 `chatgpt_base_url` 的依赖
  - `CodexAuth::from_auth_storage(...)` 不再接收 ChatGPT base URL。
  - `AuthManager::new(...)` / `AuthManager::shared(...)` 不再保存或转发 ChatGPT base URL。
  - `load_auth(...)` / `from_auth_dot_json(...)` 不再携带无效 ChatGPT URL 参数。
  - cloud-config 仍然把 `chatgpt_base_url` 用于 legacy backend bundle client，但不再传给
    AuthManager。
  - 这一步不等于全仓删除 `chatgpt_base_url`，只是把 auth 层从 ChatGPT hosted URL 概念里解耦出来。

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

## 最近完成的 chatgpt workspace/connectors slice

本轮完成的代码 slice：

> 让 app-server 相关的 workspace settings 和 connector directory 路径不再访问 ChatGPT hosted backend。

已编辑文件：

- `codex-rs/chatgpt/src/workspace_settings.rs`
- `codex-rs/chatgpt/src/connectors.rs`
- `codex-rs/chatgpt/src/workspace_settings_tests.rs`

改动内容：

- 删除 workspace settings cache、settings response 解析和 path encoding 逻辑。
- `codex_plugins_enabled_for_workspace(...)` 改成本地 no-op allow。
- 删除 connector directory cache context、connector auth 和 ChatGPT directory fetch closure。
- `list_all_connectors_with_options(...)` 只从 plugin apps 构造 connector 列表。
- `list_connectors(...)` 继续合并 MCP accessible connectors，并把 `all_connectors_loaded` 设为
  `false`，避免没有 hosted directory 时过滤掉 MCP 可访问项。
- 删除旧 workspace settings URL encoding 测试。

为什么要做：

- Astral 不需要 ChatGPT workspace beta setting 作为插件开关来源。
- Astral 不应该依赖 ChatGPT hosted connector directory 才能列出 app/plugin/MCP 能力。
- 这一步保留本地 plugin app、MCP app 和 feature flag 行为，只移除 OpenAI hosted 查询。

仍需后续处理：

- `codex-rs/chatgpt/src/chatgpt_client.rs` 仍存在，当前主要还被 `get_task.rs` 使用。
- app-server plugin remote share/install/read 路径仍保留大量 remote plugin 类型和 ChatGPT 语义，但多数入口已有
  `remote_plugin_control_plane_enabled() == false` gate。
- core config 里的 `chatgpt_base_url` 字段仍未移除。

## 最近完成的 cloud-config slice

本轮完成的代码 slice：

> 让 `codex-cloud-config` 的导出 loader 不再启动 legacy ChatGPT hosted workspace policy 拉取。

已编辑文件：

- `codex-rs/cloud-config/src/lib.rs`
- `codex-rs/cloud-config/src/bundle_loader.rs`
- `codex-rs/cloud-config/src/backend.rs`
- `codex-rs/cloud-config/src/service.rs`

改动内容：

- 生产导出的 `cloud_config_bundle_loader(...)` 现在直接返回空 loader。
- 生产导出的 `cloud_config_bundle_loader_for_storage(...)` 现在直接返回空 loader。
- 删除 `BackendBundleClient`，避免 cloud-config crate 自己构造 hosted backend client。
- 删除后台 refresh loop，避免长期后台轮询 remote policy bundle。
- 将旧 backend/cache/metrics/service/validation 模块限制为测试构建。
- 旧 ChatGPT hosted fetch 行为测试已标记 ignored，避免测试把旧控制面重新拉回默认路径。

为什么要做：

- TUI 主入口已经默认使用空 `CloudConfigBundleLoader`，但 crate 导出函数仍能被误用并启动
  ChatGPT hosted cloud-config 控制面。
- Astral 不需要 OpenAI/ChatGPT workspace-managed policy 拉取。
- provider-neutral policy/config control plane 未来应该重新设计，不应复用旧 ChatGPT hosted bundle。

仍需后续处理：

- `codex-rs/core/src/config` 里 `chatgpt_base_url` 字段和 config schema 仍存在。
- app-server plugins/catalog/apps 里仍有 workspace settings / remote plugin 旧语义，需要继续去 OpenAI 化。
- cloud-config 的旧测试 fixture 仍保留 `codex` 命名和 ChatGPT auth 概念，后续做机械/语义清理。
- `just test -p codex-cloud-config` 当前结果是 13 passed / 14 skipped；skipped 部分都是旧
  ChatGPT hosted fetch 行为。

## 最近完成的 cloud-tasks slice

本轮完成的代码 slice：

> 让 `codex-rs/cloud-tasks` 不再在缺省配置下静默访问 ChatGPT hosted backend。

已编辑文件：

- `codex-rs/cloud-tasks/src/lib.rs`
- `codex-rs/cloud-tasks/src/util.rs`

改动内容：

- 新增 `util::ASTRAL_CLOUD_TASKS_BASE_URL_ENV_VAR`。
- 新增 `util::cloud_tasks_base_url_from_env()`，统一读取并 normalize
  `ASTRAL_CLOUD_TASKS_BASE_URL`。
- `init_backend(...)` 不再读取 `CODEX_CLOUD_TASKS_BASE_URL`。
- cloud task TUI 的 environment list/autodetect 不再复制 getenv + ChatGPT 默认值逻辑。
- 新增 `list_environments_from_configured_backend()` 和
  `autodetect_environment_from_configured_backend(...)`，收拢环境加载路径。
- debug mock 模式保留可运行性，但默认只使用本地占位 URL，不触碰 OpenAI/ChatGPT。

为什么要做：

- cloud-tasks 是 remote/cloud control-plane 风险区。
- 旧实现缺少 env 时会直接 fallback 到 `https://chatgpt.com/backend-api`。
- Astral 不应该把 OpenAI hosted backend 当成默认控制面。
- 这一步先拔掉默认外联风险，不重写 cloud task backend 协议。

仍需后续处理：

- `build_chatgpt_headers(...)`、`AuthMode::ChatGPT`、`ChatGPT-Account-Id` 仍是旧 cloud task
  auth 语义，需要在 remote/cloud control-plane 总清理时一起处理。
- URL parser 和 task URL formatter 测试仍保留 `chatgpt.com` fixture，用于覆盖旧 URL 解析行为；
  是否删除这些 fixture 等 cloud tasks 去 legacy 化时再决定。

## 最近完成的 memories guard slice

本轮完成的代码 slice：

> 让 memories startup 不再访问 ChatGPT/Codex hosted backend 查询 rate limit。

已编辑文件：

- `codex-rs/memories/write/src/guard.rs`
- `codex-rs/memories/write/src/lib.rs`
- `codex-rs/memories/write/src/guard_tests.rs`
- `codex-rs/memories/write/Cargo.toml`
- `codex-rs/Cargo.lock`

改动内容：

- `guard::rate_limits_ok(...)` 改为本地 allow，并记录 debug 日志。
- 删除旧 `rate_limits_check(...)`，不再调用 `AuthManager::auth()`、`uses_codex_backend()`、
  `BackendClient::from_auth(...)` 或 `get_rate_limits_many()`。
- 删除旧 rate-limit snapshot helper 和测试。
- 删除 `guard_limits::CODEX_LIMIT_ID`。
- 从 `codex-memories-write` 直接依赖中移除 `codex-backend-client`。

为什么要做：

- Astral 不应该为了本地 memories startup 去访问 ChatGPT hosted backend。
- memories 是本地/agent 体验能力，不应被 OpenAI rate-limit guard 绑定。
- 删除 direct dependency 后，`codex-memories-write` 不再把 hosted backend client 作为自身依赖带入。

验证：

- `just fmt`
- `cargo check --tests -p codex-memories-write`
- `just test -p codex-memories-write`：27 passed / 0 skipped
- `just bazel-lock-update`
- `bazel mod deps --lockfile_mode=error`

注意：

- `just bazel-lock-check` 当前在本机失败，原因是 Unix 包装脚本
  `.github/scripts/run_bazel_with_buildbuddy.py` 使用了 Python 3.10+ 的 `type | None` 语法，但系统
  `python3` 是 3.9.6。直接运行核心 Bazel lock 校验命令已通过，`MODULE.bazel.lock` 没有 diff。

## 最近完成的 ChatGPT task fetch slice

本轮完成的代码 slice：

> 删除 `codex-chatgpt` 里 legacy ChatGPT task fetch 的 authenticated HTTP client。

已编辑文件：

- `codex-rs/chatgpt/src/chatgpt_client.rs`
- `codex-rs/chatgpt/src/lib.rs`
- `codex-rs/chatgpt/src/get_task.rs`
- `codex-rs/chatgpt/src/apply_command.rs`
- `codex-rs/chatgpt/Cargo.toml`
- `codex-rs/Cargo.lock`

改动内容：

- 删除 `chatgpt_client` 模块。
- 删除 `chatgpt_get_request(...)` / `chatgpt_get_request_with_timeout(...)`。
- `get_task.rs` 只保留 task response/diff 解析类型，不再构造 `/wham/tasks/{task_id}` 请求。
- `run_apply_command(...)` 对 legacy ChatGPT task apply 明确返回 unsupported。
- `apply_diff_from_task(...)` 保留，用于本地 fixture 和后续可能的 provider-neutral task/diff 数据输入。
- 从 `codex-chatgpt` 直接依赖中移除 `codex-model-provider`，因为它只服务于旧 ChatGPT auth header。

为什么要做：

- Astral 不再支持通过 ChatGPT hosted `/wham/tasks/...` 拉取 Codex task。
- 旧路径依赖 ChatGPT backend auth、`OAI-Product-Sku` 和 OpenAI provider auth header，不适合
  provider-neutral 项目。
- 保留本地 diff apply helper 可以避免把有用的 patch 应用逻辑和旧网络控制面绑死。

验证：

- `just fmt`
- `cargo check --tests -p codex-chatgpt`
- `just test -p codex-chatgpt`：6 passed / 0 skipped
- `just bazel-lock-update`
- `bazel mod deps --lockfile_mode=error`

## 最近完成的 ChatGPT base URL default slice

本轮完成的代码 slice：

> Astral effective config 不再默认携带 ChatGPT hosted backend URL。

已编辑文件：

- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/config/config_tests.rs`
- `codex-rs/app-server/src/lib.rs`

改动内容：

- `Config::load_from_base_config_with_overrides(...)` 中的 `chatgpt_base_url` 默认值从
  `https://chatgpt.com/backend-api/` 改为空字符串。
- `Config` 字段注释改为 deprecated legacy ChatGPT-hosted control-plane URL。
- 新增测试 `load_config_does_not_default_to_chatgpt_backend`，防止默认 ChatGPT URL 回流。
- app-server 启用 remote control 但没有显式 backend URL 时，直接返回清晰错误。

为什么要做：

- 旧默认值会让任何仍持有 `config.chatgpt_base_url` 的旧控制面对象默认指向 OpenAI/ChatGPT。
- Astral 默认路径不能暗中访问或携带 `chatgpt.com/backend-api`。
- 这一步还没有删除字段本身；字段仍作为 legacy explicit override 存在，方便后续拆 remote control、
  remote plugin 和旧测试时分阶段收敛。

验证：

- `just fmt`
- `cargo check --tests -p codex-core -p codex-app-server`
- `just test -p codex-core load_config_does_not_default_to_chatgpt_backend`：1 passed / 2639 skipped

已暴露的后续风险：

- 已由下一节 remote control slice 处理：app-server 不再允许启动或通过 RPC 启用 legacy hosted
  remote control。底层 `app-server-transport` 里的旧 remote-control 模块仍存在，但默认和 app-server
  暴露路径已经切断。

## 最近完成的 agent identity guard slice

本轮完成的代码 slice：

> 禁用 `codex-agent-identity` 中会访问 ChatGPT hosted agent identity 服务的网络入口。

已编辑文件：

- `codex-rs/agent-identity/src/lib.rs`
- `ASTRAL_CODE_PROGRESS.md`

改动内容：

- 新增 hosted agent identity control-plane disabled guard。
- `fetch_agent_identity_jwks(...)` 在构造/请求 ChatGPT JWKS URL 前直接返回 Astral disabled。
- `register_agent_task(...)` 在 task registration 签名和 HTTP request 前直接返回 Astral disabled。
- 保留本地能力：JWT payload/verified decode、key generation、AgentAssertion signing、task id decrypt
  等纯本地工具函数不动。

为什么要做：

- Agent Identity 是 OpenAI/ChatGPT hosted 控制面，依赖 `chatgpt_base_url`、JWKS、agent runtime id 和
  task registration。
- Astral 当前只支持 provider-neutral/API-key 模式，不应该允许隐藏路径去访问 ChatGPT agent identity 服务。
- 这一步继续收窄 OpenAI 专有 auth/control-plane，同时不破坏本地加密/签名工具和现有类型边界。

后续风险：

- `login` storage 里仍保留 AgentIdentity auth record 的解析/roundtrip 类型，但 `CodexAuth` 主路径已拒绝
  AgentIdentity。
- 如果后续完全删除 AgentIdentity auth 类型，需要联动 app-server account/auth status、storage fixture 和
  config/schema 测试，适合作为单独机械清理。

## 最近完成的 core-skills remote guard slice

本轮完成的代码 slice：

> `core-skills/src/remote.rs` hosted remote skill API 在库层直接禁用，避免未来入口误接上
> ChatGPT `/hazelnuts` skill service。

已编辑文件：

- `codex-rs/core-skills/src/remote.rs`
- `codex-rs/core-skills/src/remote_tests.rs`
- `ASTRAL_CODE_PROGRESS.md`

改动内容：

- 新增 remote skill control-plane disabled guard。
- `list_remote_skills(...)` 在处理 `chatgpt_base_url` 和 ChatGPT auth 前直接返回 Astral disabled。
- `export_remote_skill(...)` 在 ChatGPT auth、download request、zip payload 校验和 extract 前直接返回
  Astral disabled。
- 新增两条测试，确认 list/export 都在缺少 auth 时返回 disabled，而不是进入旧 ChatGPT auth 错误或网络路径。

为什么要做：

- remote skill client 当前注释为“future wiring”，但代码已经是完整 ChatGPT hosted `/hazelnuts` API 客户端。
- Astral 的 skill runtime 要保留，但 hosted skill marketplace/export 控制面不能默认保留旧 ChatGPT 后端语义。
- 这一步和 remote plugin guard 保持同一原则：本地 skills/plugins/MCP 不动，OpenAI hosted 分发面默认切断。

后续风险：

- `RemoteSkillScope` / `RemoteSkillProductSurface` 等类型仍存在，用于当前编译边界。
- 如果后续要做 provider-neutral skill registry，应新建 Astral 自己的服务协议，不复用 `chatgpt_base_url` 和
  `/hazelnuts` 语义。

## 最近完成的 chat-completions usage stream slice

本轮完成的代码 slice：

> 补强 OpenAI-compatible `/v1/chat/completions` 流式 adapter，支持国内模型常见的空 `choices` usage
> chunk，并保留一次性 `Completed` 事件语义。

已编辑文件：

- `codex-rs/codex-api/src/agent_adapters/chat_completions.rs`
- `codex-rs/codex-api/src/agent_adapters/chat_completions_tests.rs`
- `codex-rs/codex-api/src/sse/agent.rs`
- `codex-rs/codex-api/src/sse/agent_tests.rs`
- `ASTRAL_CODE_PROGRESS.md`

改动内容：

- `parse_stream_chunk(...)` 现在识别 `choices: []` 且带 `usage` 的标准 OpenAI-compatible final usage
  chunk。
- chat-completions SSE 处理器会暂存 `finish_reason`，等后续 usage chunk 到达时合并成一次
  `ResponseEvent::Completed`。
- 如果 provider 只发送 finish_reason 后直接 `[DONE]`，SSE 处理器仍会用暂存 stop reason 完成这一轮。
- 新增测试覆盖 empty choices usage chunk，以及 finish_reason chunk + usage chunk 合并后只产生一次
  `Completed`，并保留 input/output/cached token usage。

为什么要做：

- 很多国内 OpenAI-compatible 网关在 `stream_options.include_usage = true` 时会用
  `choices: []` 的最终 chunk 返回 usage。
- 旧实现会在 finish_reason chunk 先完成 stream，然后直接丢掉后续 usage chunk，导致 token 统计和缓存命中
  统计不准。
- 用户明确关心国产模型成本和缓存命中率，这类 usage 映射属于 provider adapter 的基础可靠性。

后续风险：

- 还需要继续抓/整理真实 DeepSeek、Anthropic-compatible、OpenAI-compatible fixture，校准 tool call
  delta、reasoning/thinking、provider-specific request body 字段和错误恢复。
- 需要评估是否增加显式 provider config，用于 omit 某些国内网关不兼容的 chat-completions 字段，例如
  `parallel_tool_calls` 或 `stream_options`。

## 最近完成的 core-plugins remote guard slice

本轮完成的代码 slice：

> `core-plugins/src/remote/*` hosted remote plugin 控制面在库层直接禁用，防止 app-server 以外的调用路径
> 绕过上层 gate 后触碰旧 ChatGPT auth/network/archive 逻辑。

已编辑文件：

- `codex-rs/core-plugins/src/remote.rs`
- `codex-rs/core-plugins/src/remote/share.rs`
- `codex-rs/core-plugins/src/remote/remote_installed_plugin_sync.rs`
- `codex-rs/core-plugins/src/remote_tests.rs`
- `codex-rs/app-server/src/request_processors/plugins.rs`
- `ASTRAL_CODE_PROGRESS.md`

改动内容：

- 新增 `RemotePluginCatalogError::ControlPlaneDisabled`，统一表达 Astral 禁用 legacy hosted remote
  plugin control-plane。
- `fetch_remote_marketplaces(...)`、global remote catalog fetch/cache helpers、OpenAI curated remote
  marketplace fetch、remote installed plugins fetch、remote share context fetch、remote skill/detail fetch、
  remote install/uninstall 等入口在函数最前面返回 `ControlPlaneDisabled`。
- `has_cached_global_remote_plugin_catalog(...)` 在 Astral disabled 模式下固定返回 `false`。
- `cached_global_remote_discoverable_plugins(...)` 在 Astral disabled 模式下固定返回空列表。
- `save/list/delete/update remote plugin share` 在 archive/config/auth 前直接返回 disabled。
- `sync_remote_installed_plugin_bundles_once(...)` 在 auth/config/network 前直接返回 disabled。
- app-server 将 `ControlPlaneDisabled` 映射成 `invalid_request`，保持 RPC 层禁用语义清晰。

为什么要做：

- 之前 app-server 层已经禁用了 remote plugin/share/read/install/uninstall 多数入口，但底层
  `core-plugins` remote 函数仍然保留完整 hosted control-plane 实现。
- Astral 是新项目，不兼容旧 Codex remote plugin/share 状态，也不应该保留任何默认可触发的
  ChatGPT plugin service 外联口。
- 这一步不是删除本地 plugin/skill/MCP runtime，而是把 OpenAI hosted plugin 分发与分享控制面从默认
  库能力里切掉。

后续风险：

- remote plugin 类型、测试 fixture 和部分 manager 调度结构仍存在，用于当前编译边界。
- 更洁癖的下一步可以把 legacy remote plugin 实现拆成非默认 feature 或删除对应类型面，但要避免一次性
  引爆 plugin manager、app-server protocol 和测试 fixture 的大范围 diff。

## 最近完成的 app-server remote-control URL cleanup slice

本轮完成的代码 slice：

> app-server disabled remote-control runtime 不再携带 `config.chatgpt_base_url`。

已编辑文件：

- `codex-rs/app-server/src/lib.rs`
- `ASTRAL_CODE_PROGRESS.md`

改动内容：

- app-server 启动时仍会创建 disabled remote-control handle，以保持 status/disable/read 生命周期稳定。
- 但传给 `RemoteControlStartConfig.remote_control_url` 的值从 `config.chatgpt_base_url.clone()` 改为空字符串。
- 由于 `remote_control_enabled = false`，底层不会 normalize 或连接这个 URL；这一步避免 disabled runtime
  的 handle/log 上继续携带 ChatGPT hosted backend URL。

为什么要做：

- 上一批改动已经禁用了 remote control 启用入口，但 disabled runtime 仍在构造时持有旧
  ChatGPT base URL。
- Astral 默认路径不应该携带 OpenAI/ChatGPT hosted 控制面的 URL，即使它当前不会被连接。
- 这一步不破坏 app-server C/S 架构，也不改变 remote-control status disabled 语义。

后续风险：

- `app-server-transport/src/transport/remote_control/*` 仍包含完整 legacy WHAM/WebSocket 实现和测试。
  下一步可以继续把底层 transport 降级成 Astral disabled stub，或拆成非默认 legacy feature。

## 最近完成的 app-server remote installed plugin slice

本轮完成的代码 slice：

> `plugin/installed` 在 remote plugin control-plane 禁用时，不再触发 remote installed catalog fetch 或
> bundle sync。

已编辑文件：

- `codex-rs/app-server/src/request_processors/plugins.rs`
- `codex-rs/app-server/tests/suite/v2/plugin_list.rs`
- `ASTRAL_CODE_PROGRESS.md`

改动内容：

- `plugin_installed_response(...)` 只有在 `remote_installed_plugin_visible_scopes` 非空时，才启动
  `maybe_start_remote_installed_plugin_bundle_sync(...)`。
- `load_remote_installed_plugins(...)` 收到空 `visible_scopes` 时直接返回空列表，不再先调用
  `build_and_cache_remote_installed_plugin_marketplaces(...)`。
- 更新测试，断言 remote plugin control-plane disabled 时 `plugin/installed` 不会请求
  `/ps/plugins/installed`，也不会下载或写入 remote plugin bundle cache。

为什么要做：

- 原代码虽然把可见 remote scope 置空，但仍可能先 fetch remote installed plugin catalog，再把结果按空
  scope 丢掉。
- 这属于隐蔽 hosted 外联口：用户只是读取 installed plugin 列表，Astral 不应暗中访问 ChatGPT plugin
  service。
- 这一步仍保留本地 installed plugin、suggested plugin、本地 marketplace 行为。

后续风险：

- `PluginsManager` 和 `core-plugins/src/remote/remote_installed_plugin_sync.rs` 仍包含远程同步实现；app-server
  默认路径已切断，后续可以继续将底层实现降级为 legacy feature 或 Astral disabled stub。

## 最近完成的 app-server remote plugin disabled short-circuit slice

本轮完成的代码 slice：

> remote plugin/share/read/install/uninstall 的 disabled 判断前移，避免返回禁用前触碰旧 config/auth。

已编辑文件：

- `codex-rs/app-server/src/request_processors/plugins.rs`
- `ASTRAL_CODE_PROGRESS.md`

改动内容：

- `plugin/read` 的 remote marketplace 分支在 `remote_plugin_control_plane_enabled() == false` 时，加载
  config 前直接返回 `remote plugin read is not enabled ...`。
- `plugin/skill/read` 在 control-plane 禁用时，加载 config 前直接返回 disabled。
- `plugin/share/save`、`plugin/share/updateTargets`、`plugin/share/checkout` 在 control-plane 禁用时，调用
  `load_plugin_share_config_and_auth()` 前直接返回 `plugin sharing is disabled`。
- remote `plugin/install`、remote `plugin/uninstall` 在 control-plane 禁用时，加载 config 前直接返回 disabled。
- 保留原有错误文案，避免这一步扩大 app-server API 行为差异。

为什么要做：

- 这些入口已经被 false 闸门挡住，但原先部分路径仍会先读取 config/auth，再判断 disabled。
- Astral 不兼容旧 Codex 用户数据，也不应该在 provider-neutral 模式里无意义触碰 ChatGPT/OpenAI auth 面。
- 这一步不影响本地 plugin install/read/uninstall，也不影响 MCP/skills。

后续风险：

- 源码内的 remote plugin service client、bundle download/install、remote installed sync 仍存在；这一步只是把
  app-server disabled 快速路径进一步收紧。

## 最近完成的 app-server remote plugin mapping slice

本轮完成的代码 slice：

> remote plugin control-plane 禁用时，app-server 不再读取或展示本地旧 share 映射。

已编辑文件：

- `codex-rs/app-server/src/request_processors/plugins.rs`
- `codex-rs/app-server/tests/suite/v2/plugin_list.rs`
- `codex-rs/app-server/tests/suite/v2/plugin_read.rs`
- `ASTRAL_CODE_PROGRESS.md`

改动内容：

- `load_shared_plugin_ids_by_local_path(...)` 在 `remote_plugin_control_plane_enabled() == false` 时直接返回空映射。
- `plugin/list` 读取本地 marketplace 时，即使 Astral home 里残留
  `localPluginPathsByRemotePluginId`，也不会在 `PluginSummary.share_context` 暴露旧 hosted share 状态。
- `plugin/read` 读取本地插件详情时，同样忽略旧 share mapping，不再表现为“曾经分享到 ChatGPT
  plugin service 的本地插件”。
- 更新两条窄测试，明确 Astral disabled control-plane 下旧 mapping 应被忽略。

为什么要做：

- Astral 是新项目，不兼容旧 Codex 用户数据；不能因为磁盘上存在旧 mapping 文件，就让 API 暗示仍有
  ChatGPT hosted plugin sharing。
- 上游 remote plugin/share 代码仍在编译边界内，但 control-plane 已禁用。读取旧 mapping 是一个显示层回流口，
  会让用户以为 share/install/read 还有 hosted 后端语义。
- 这一步不破坏本地 plugin/skill/MCP 能力，只隐藏旧 remote share metadata。

后续风险：

- `core-plugins/src/remote/share.rs`、`remote_installed_plugin_sync.rs` 和 app-server remote
  `plugin/share/*`、remote install/uninstall/read 的旧实现仍在源码中。当前 app-server 闸门默认挡住调用，
  后续要么降级成 Astral disabled stub，要么拆成非默认 legacy feature。

## 最近完成的 app-server remote control slice

本轮完成的代码 slice：

> 禁用 app-server 的 legacy hosted remote control 入口，避免 Astral 继续暴露 ChatGPT WHAM 控制面。

已编辑文件：

- `codex-rs/app-server/src/lib.rs`
- `codex-rs/app-server/src/request_processors/remote_control_processor.rs`
- `codex-rs/app-server/src/request_processors/remote_control_processor/remote_control_processor_tests.rs`
- `codex-rs/app-server/tests/suite/v2/remote_control.rs`

改动内容：

- `AppServerRuntimeOptions.remote_control_enabled` 被请求时，启动直接返回错误：
  `legacy hosted remote control is disabled in Astral until a provider-neutral control plane exists`。
- app-server RPC 层的 `remoteControl/enable`、pairing start/status、client list/revoke 全部返回同一
  Astral disabled 错误。
- `remoteControl/status/read` 和 `remoteControl/disable` 保留 disabled 状态行为，避免破坏普通
  app-server 生命周期和 UI 状态读取。
- 删除 app-server remote-control 测试中 mock ChatGPT backend、ChatGPT auth fixture 和
  `/backend-api/wham/remote/control/...` 成功路径断言，改为验证 Astral 禁用语义。

为什么要做：

- Codex 原 remote control 是 OpenAI/ChatGPT hosted 控制面：需要 ChatGPT authentication、account id、
  enrollment、server token、pairing 和 client management。
- Astral 还没有 provider-neutral remote control 服务端协议，不能把旧 WHAM 控制面通过换 base URL 的方式保留。
- 这一步尊重 Codex 的 C/S 骨架：app-server、transport 类型、status disabled 状态仍保留；只是切断会访问
  ChatGPT hosted control-plane 的启用入口。

后续风险：

- `codex-rs/app-server-transport/src/transport/remote_control/*` 里仍保留旧实现和测试，用于当前编译边界。
  后续如果要更洁癖，可以把它降级为 stub 或拆成非默认 legacy feature。

## 最近完成的 CLI / daemon remote control slice

本轮完成的代码 slice：

> 禁用 CLI 和 app-server daemon 层的 legacy hosted remote control 启用入口。

已编辑文件：

- `codex-rs/cli/src/remote_control_cmd.rs`
- `codex-rs/cli/src/main.rs`
- `codex-rs/app-server-daemon/src/lib.rs`
- `codex-rs/app-server-daemon/src/settings.rs`
- `codex-rs/app-server-daemon/src/backend/pid.rs`
- `codex-rs/app-server-daemon/src/backend/pid_tests.rs`
- `codex-rs/app-server-daemon/src/remote_control_client.rs`
- `ASTRAL_CODE_PROGRESS.md`

改动内容：

- 顶层 `astral remote-control` / `astral remote-control start` / `astral remote-control stop`
  不再启动 foreground app-server 或 managed daemon，直接返回 Astral disabled 错误。
- `app-server --remote-control`、`app-server daemon bootstrap --remote-control`、
  `app-server daemon enable-remote-control` 在 CLI 层直接拒绝。
- `codex_app_server_daemon::bootstrap(...)` 在 `remote_control_enabled = true` 时拒绝。
- `ensure_remote_control_started(...)`、`ensure_remote_control_ready(...)`、
  `enable_remote_control_on_socket(...)` 直接拒绝。
- `set_remote_control(RemoteControlMode::Enabled)` 直接拒绝；`Disabled` 保留，作为无害清理动作。
- 删除 daemon 内部旧 `remote_control_client`，不再通过 socket 发送 `remoteControl/enable`。
- daemon 读取到旧 `remoteControlEnabled: true` 设置时归一成 `false`。
- pid backend 即使被传入 `remote_control_enabled = true`，也只启动普通
  `app-server --listen unix://`，不会拼 `--remote-control`。
- CLI help 文案从“启用 remote control”改为 legacy hosted remote control 在 Astral 中禁用。

为什么要做：

- 上一个 slice 已经让 app-server 暴露层拒绝 legacy hosted remote control；如果 CLI/daemon 仍尝试启动，
  用户会先看到旧启动流程和旧 daemon 语义，再在更深处失败。
- Astral 没有 provider-neutral remote-control 服务端协议前，不应该保留任何显式启用 hosted
  remote-control 的入口。
- 保留 app-server daemon 的普通 start/stop/version，不影响本地 C/S 架构。

后续风险：

- `app-server-daemon` 的输出类型和 settings 类型仍保留 `remote_control_enabled` 字段用于当前 API 编译边界；
  但读取和启动效果已经固定为 disabled。后续更洁癖时可以把字段从协议/输出形状里一起删掉。

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
- `just test -p codex-login auth`
- `cargo check --tests -p codex-cli -p codex-cloud-config -p codex-cloud-tasks -p codex-app-server-transport -p codex-core`
- `cargo check --tests -p codex-cloud-tasks`
- `cargo check --tests -p codex-cloud-config`
- `just test -p codex-cloud-config`
- `cargo check --tests -p codex-chatgpt`
- `just test -p codex-chatgpt`
- `cargo check --tests -p codex-memories-write`
- `just test -p codex-memories-write`
- `just bazel-lock-update`
- `bazel mod deps --lockfile_mode=error`
- `cargo check --tests -p codex-core -p codex-app-server`
- `just test -p codex-core load_config_does_not_default_to_chatgpt_backend`
- `cargo check --tests -p codex-app-server`
- `just test -p codex-app-server remote_control`
- `CARGO_INCREMENTAL=0 cargo check --tests -p codex-cli -p codex-app-server-daemon`
- `CARGO_INCREMENTAL=0 just test -p codex-cli remote_control_subcommand_names_match_cli_shape`
- `CARGO_INCREMENTAL=0 cargo check --tests -p codex-app-server-daemon`
- `CARGO_INCREMENTAL=0 just test -p codex-app-server-daemon remote_control`
- `CARGO_INCREMENTAL=0 cargo check --tests -p codex-app-server`
- `CARGO_INCREMENTAL=0 just test -p codex-app-server plugin_list_ignores_local_share_mapping_when_remote_control_plane_disabled`
- `CARGO_INCREMENTAL=0 just test -p codex-app-server plugin_read_ignores_local_share_mapping_when_remote_control_plane_disabled`
- `CARGO_INCREMENTAL=0 just test -p codex-app-server plugin_install_rejects_remote_marketplace_when_plugins_are_disabled`
- `CARGO_INCREMENTAL=0 just test -p codex-app-server plugin_uninstall_rejects_remote_plugin_when_plugins_are_disabled`
- `CARGO_INCREMENTAL=0 just test -p codex-app-server plugin_share_save_rejects_when_plugin_sharing_disabled`
- `CARGO_INCREMENTAL=0 just test -p codex-app-server plugin_read_rejects_remote_marketplace_when_plugins_are_disabled`
- `CARGO_INCREMENTAL=0 just test -p codex-app-server plugin_installed_skips_remote_fetch_when_control_plane_disabled`
- `CARGO_INCREMENTAL=0 just test -p codex-app-server remote_control`
- `CARGO_INCREMENTAL=0 cargo check --tests -p codex-core-plugins -p codex-app-server`
- `CARGO_INCREMENTAL=0 just test -p codex-core-plugins control_plane_disabled`
- `CARGO_INCREMENTAL=0 just test -p codex-app-server plugin_read_rejects_remote_marketplace_when_plugins_are_disabled`
- `CARGO_INCREMENTAL=0 cargo check --tests -p codex-api`
- `CARGO_INCREMENTAL=0 just test -p codex-api chat_completions`
- `CARGO_INCREMENTAL=0 just test -p codex-api chat_stream_merges_finish_reason_with_empty_choices_usage_chunk`
- `CARGO_INCREMENTAL=0 cargo check --tests -p codex-core-skills`
- `CARGO_INCREMENTAL=0 just test -p codex-core-skills remote_skill`
- `CARGO_INCREMENTAL=0 cargo check --tests -p codex-agent-identity`
- `CARGO_INCREMENTAL=0 just test -p codex-agent-identity hosted_agent_identity_control_plane_is_disabled`
- `just fmt`
- `CARGO_INCREMENTAL=0 cargo check --tests -p codex-client -p codex-login -p codex-backend-client`
- `just fmt`
- `CARGO_INCREMENTAL=0 cargo check --tests -p codex-backend-client`
- 旧 ChatGPT refresh 符号窄范围搜索：`codex-rs/login` 与 app-server auth 测试无命中。
- `git diff --check`

已观察到但暂不处理的问题：

- `just test -p codex-model-provider` 全量仍有 Bedrock catalog 失败，因为 bundled `models.json`
  缺少 `gpt-5.5`。这和当前 Astral provider-neutral cleanup 无关。
- `just test -p codex-app-server auth` 会额外匹配 plugin install/read 的 `needsAuth` 测试，这些测试仍按
  旧 ChatGPT app auth 语义期待 `chatgpt.com/apps/...` 认证项。处理 plugin remote/control-plane 时再改。
- 本机 `just bazel-lock-check` 的 Unix 包装脚本会调用
  `.github/scripts/run_bazel_with_buildbuddy.py`，该脚本使用 Python 3.10+ 的 `type | None` 注解语法；
  当前 `/usr/bin/python3` 是 3.9.6，会在真正执行 Bazel 前 TypeError。直接执行
  `bazel mod deps --lockfile_mode=error` 可以完成 lockfile 校验。
- `just test -p codex-app-server remote_control` 的旧失败来源已在 app-server 暴露层处理：测试改为验证
  Astral disabled 语义。底层 `app-server-transport` remote-control 旧模块仍待后续降级或删除。

## 剩余高优先级工作

1. 审计并清理剩余 OpenAI/ChatGPT auth/config 面
   - core config 和 remote/cloud 模块中剩余的 `chatgpt_base_url`
   - app-server account docs/tests 中残留的 ChatGPT auth 语义
   - `AuthMode::ChatGPT` / PAT / Agent Identity 是否需要彻底删除、隔离或标记 legacy unsupported

2. 审计 remote/cloud control-plane
   - `backend-client`
   - `cloud-config` 旧测试/类型命名
   - `cloud-tasks` 旧 auth/header 语义
   - `core-plugins/src/remote*`
   - `memories/write`
   - 目标：默认路径不能静默访问 `chatgpt.com/backend-api`。
  - app-server remote control 暴露入口已禁用；下一刀建议优先处理 app-server / core-plugins remote
     plugin share/install/read 的旧语义，或把 app-server-transport remote-control 旧模块降级为 stub。

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
   - 如果有未提交改动，先读 diff，确认是否来自上一轮正在做的 slice，不要误删。

4. 继续开发时，优先从以下三条线中选最小 coherent slice：
   - `codex-rs/chatgpt/src/chatgpt_client.rs`、`get_task.rs`、`apply_command.rs`：禁用 legacy
     ChatGPT task fetch/apply 默认路径。
   - `codex-rs/memories/write/src/guard.rs`：检查是否还会通过 `BackendClient::from_auth(...)`
     访问 hosted backend。
   - `codex-rs/core/src/config` / `codex-rs/config`：继续收敛 `chatgpt_base_url` 和旧 ChatGPT auth/config
     字段，注意不要破坏新的 `ASTRAL_BASE_URL` / provider-neutral 配置。

5. 搜索优先关键词：
   - `chatgpt_base_url`
   - `chatgpt_get_request`
   - `backend-api`
   - `AuthMode::ChatGPT`
   - `uses_codex_backend`
   - `remote_plugin_control_plane_enabled`

6. 不要把 goal 标记为 complete，除非 provider-neutral protocol、Claude-ish tools、OpenAI 控制面清理、
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
