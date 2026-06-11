# Astral-Code 项目状态

最后更新：2026-06-11

这份文档是 Astral-Code 长线改造的“航标”。每次暂停、重大提交后，或者担心上下文被 compact 丢失前，都应该更新它。

## 核心目标

把 `astral-code` 做成一个深度 fork，而不是 Codex 的兼容模式或换皮模式。

产品形态：

- 公开仓库：`oines/astral-code`
- CLI 命令：`astral`
- 用户可见项目名：`astral-code`
- 状态与配置命名空间：`ASTRAL_HOME`、`~/.astral-code`、`ASTRAL_API_KEY`、
  `ASTRAL_BASE_URL`、`ASTRAL_EXEC_SERVER_URL`

架构意图：

- 继承 Codex 的优秀运行时骨架：C/S 架构、daemon/app-server、exec-server、PTY 缓冲、
  UnifiedExec、sandbox、approval、environment、MCP、skills/plugins、Plan Mode、Goal Mode、
  local compact 和 subagent runtime。
- 把 OpenAI/Responses-first 的模型协议假设替换成 provider-neutral 的模型管线。
- 给模型暴露 Claude Code-like 的核心工具，让国产模型或 OpenAI-compatible coding model
  看到更熟悉的 agentic SFT 轨迹。
- 删除或隔离 OpenAI 专有控制面：ChatGPT 登录、OpenAI hosted auth/routing、充值/限流提示、
  feedback upload、remote compact、OpenAI-only web/search gating、ChatGPT backend 默认路由。

## 不可动摇的决策

- 不兼容旧 Codex 用户数据。不要读取、迁移或 fallback 到 `~/.codex`、`CODEX_HOME`、旧
  session、旧 auth、旧 plugin 或旧 config。
- 新增用户可见面只使用 Astral 命名。内部 crate 名可以暂时继续是 `codex-*`，后续可以做
  机械重命名。
- 不削弱 sandbox 行为。Seatbelt/Bwrap/Windows sandbox、approval、denied-action retry 都是
  必须继承的运行时边界。
- 做 tool flavor 时不要替换 app-server、exec-server、UnifiedExec、PTY 或
  Environment/ExecBackend。
- 保留 MCP 和本地 skills/plugins。OpenAI/ChatGPT 远程 plugin 分发要单独审计，不能默认信任。
- 保留 Codex local compact，除非有明确证据证明它破坏模型原生 tool streaming。OpenAI remote
  compact 不应该作为依赖保留。
- 不要把真实 provider secret 写进仓库。用户曾提供过 DeepSeek 模型名用于未来手工测试，但
  API key 不能写入文档、fixture 或 commit。

## 当前实现路线

Astral 不应该是一个薄反代，也不应该靠末端 hook 偷换协议。

首选架构：

1. 内部使用 provider-neutral Agent IR，承载 messages、tool use、tool result、stream delta、
   usage、stop reason 和错误。
2. 实现 Anthropic Messages adapter 和 OpenAI-compatible `/v1/chat/completions` adapter。
3. Responses 只作为过渡期 legacy/optional provider adapter，不再作为核心真相。
4. 工具层使用 Astral-native schema 和 handler：模型侧 Claude-ish，运行时侧复用 Codex 的
   primitives。

工具层必须在 planning boundary 就原生化：模型可见的名字和 schema 应该在 core tool plan
里被选择，而不是只在最终 HTTP 边缘改名。

## 必须继承的运行时边界

除非有强理由，不要动这些 Codex 系统：

- `app-server` 和 `app-server-protocol`
- `exec-server`
- `Environment` / `ExecBackend`
- `UnifiedExecProcessManager`
- PTY、stdin、terminate、output streaming 行为
- sandbox 和 approval engine
- permission request 生命周期
- Plan Mode 和 Goal Mode 的宿主行为
- local compact 和 history reconstruction
- MCP runtime 和 MCP resources
- skills/plugins runtime
- multi-agent v2 runtime

## Claude-ish Tool Flavor 状态

当前 `codex-rs/tools/src/astral_flavor.rs` 里已有的模型可见核心工具：

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

当前已经落地的运行时映射：

- `Bash` 通过 `AstralBashHandler` 映射到 Codex `exec_command` / `shell_command`。
- `Monitor` 通过 `AstralMonitorHandler` 映射到 `write_stdin`。
- `TaskStop` 可以终止 UnifiedExec shell task，也可以 interrupt multi-agent task。
- `Read/Write/Edit/Glob/Grep` 已有 Astral-native file handler，使用 Codex filesystem 和
  sandbox context，并支持 `environment_id`。
- `TodoWrite` 映射到 Codex `update_plan`。
- `Agent` 映射到 `spawn_agent`。
- `SendMessage` 映射到 multi-agent v2 `send_message`。
- `RequestPermissions` 映射到现有 approval/permission channel。
- MCP resource tools、`ToolSearch`、`Skill` 复用已有 extension/runtime 机制。

关键实现文件：

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

按决策暂缓的工具：

- `LSP`
- `Cron`
- `Worktree`
- `Team`
- Claude Task v2
- `NotebookEdit`
- `PowerShell`
- `Workflow`
- `RemoteTrigger`
- `ScheduleWakeup`
- `PushNotification`
- provider-neutral `WebSearch` / `WebFetch`

这些工具以后只有在 Codex 已经有可靠 runtime primitive，或者功能价值明确时，才应该原生实现。

## 已完成进度

近期关键进展：

- 本轮已完成：清理 `astral doctor` 的旧 ChatGPT/token-backed auth 诊断
  - `doctor` 不再把旧 ChatGPT/PAT/Agent Identity 凭据当成可细诊断或可用的 Astral 登录态。
  - 旧 token-backed auth 统一显示为 unsupported legacy credentials。
  - 不再输出 `stored ChatGPT tokens`、`ChatGPT auth is missing ...` 或
    `ChatGPT login plus API key` 这类用户可见诊断。
  - API key 本地凭据和 provider-specific env var 诊断保持不变。

- `d5f51c86b9 Document Astral fork progress`
  - 添加项目状态文档，用于长时间开发和上下文 compact 后接力。

- `7099420969 Remove ChatGPT login entrypoints from CLI`
  - 删除 CLI 里的隐藏参数：`--with-access-token`、`--device-auth`、`--experimental_issuer`、
    `--experimental_client-id`。
  - 删除 CLI crate 里已经禁用的 OAuth/access-token login stub 和 export。
  - 保留 `astral login --with-api-key`、`astral login status` 和 `astral logout`。
  - 验证被删除的 ChatGPT login 参数现在会在 CLI parsing 层失败，不会进入隐藏的 disabled flow。

- `7f1e959c0f Remove OpenAI backend routing from providers`
  - 删除 provider conversion 里的 OpenAI/ChatGPT 默认 base URL 路由。
  - 删除 legacy Responses provider 的 OpenAI org/project env headers。
  - 禁用 legacy Responses provider 的 Astral-managed auth 和 websocket 特权。
  - 把默认 provider capabilities 改为 provider-neutral：默认不暴露 hosted image generation 或
    web search。
  - 修复 provider-local bearer token 的 model refresh，使 `/models` 不依赖 ChatGPT backend
    auth。

- `55c446e0c9 Switch Windows installer to Astral`
  - Windows installer 用户可见 env、path、package name 切换到 Astral。

- `5d511f6440 Let Astral file tools target environments`
  - 给 `Read/Write/Edit/Glob/Grep` 增加 `environment_id` 支持。
  - 文件工具通过 Codex environment resolution 和 sandbox context 路由。

- `28382b9678 Switch package entrypoint and installer to Astral`
  - package entrypoint 和 Unix installer 切到 `astral`、`ASTRAL_*`、`~/.astral-code`。

- `f8bd6937b1 Guard Astral tool names in provider-neutral plans`
  - 确保 provider-neutral tool planning 里保留 Astral tool name。

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

## 最新暂停点

最新完成的代码 slice 是：

- 本轮已完成：清理 `astral doctor` 的旧 ChatGPT/token-backed auth 诊断

意图：

- 让 `doctor` 遵守 Astral 的新项目边界：只支持 API key / provider env auth。
- 旧 Codex/ChatGPT token-backed auth 只作为 unsupported legacy residue 报告。
- 不再提示用户修复 ChatGPT token 或 refresh metadata。

该 slice 已运行验证：

- `just fmt`
- `just test -p codex-cli doctor`
  - 结果：84 个测试通过，197 个测试跳过。

## 已运行验证

近期 focused checks：

- `just fmt`
- `just test -p codex-tools astral_flavor`
- `just test -p codex-core astral_file_tools`
- `just test -p codex-model-provider-info`
- `just test -p codex-model-provider configured_provider_uses_default_capabilities`
- `just test -p codex-model-provider configured_provider_models_manager_uses_provider_bearer_token`
- `just test -p codex-models-manager refresh_available_models_fetches_with_provider_auth`
- `just test -p codex-cli login`
- `just test -p codex-cli doctor`
- `git diff --check`

已观察到的无关或既有问题：

- `just test -p codex-model-provider` 全量仍有 Bedrock catalog 失败，原因是 bundled
  `models.json` 缺少 `gpt-5.5`。除非正在处理 Bedrock catalog，否则把它视为与 provider-neutral
  cleanup 无关。

## 剩余高优先级工作

1. 审计并移除剩余 OpenAI/ChatGPT auth/config 面：
   - core config 里的 `chatgpt_base_url`
   - app-server 的 `account/login/start` 行为
   - `codex-rs/login/src/server.rs` OAuth callback server
   - 只服务 ChatGPT OAuth 的 revoke/token 路径

2. 审计 cloud/remote control-plane crates：
   - `codex-rs/backend-client`
   - `codex-rs/cloud-config`
   - `codex-rs/cloud-tasks`
   - `codex-rs/core-plugins/src/remote*`
   - `codex-rs/memories/write`

   需要决定这些模块是删除、compile-disable，还是隔离到显式非默认 feature 后面。不能让它们默认或静默访问
   `chatgpt.com/backend-api`。

3. 继续 provider-neutral protocol 工作：
   - 让 Anthropic Messages 的 stream/tool_use/tool_result 路径成为 first-class。
   - 让 OpenAI-compatible chat-completions 的 stream/tool_calls 路径成为 first-class。
   - Responses 继续降级为 legacy，而不是核心真相。
   - 稳定 usage 和 stop-reason 映射。

4. 硬化 tool result 形状：
   - 必要时对照 Claude Code fixture 检查 Astral `tool_result` payload。
   - 尤其验证 `Bash`、`Monitor`、`TaskStop`、`Read`、`Edit`、`TodoWrite`、`Agent` 和
     permission-denied/retry flow。
   - 不要为了模仿名字而改坏 Codex runtime 行为。

5. 验证长任务 terminal 体验：
   - `Bash(run_in_background=true)` 返回可 monitor 的 id。
   - `Monitor` 可以 poll output，也可以给 y/n prompt 写 stdin。
   - `TaskStop` 可以终止 shell task。
   - 保持现有 PTY progress streaming 体验。

6. 继续保留 local compact，除非后续证据推翻：
   - 不要为了形式相似重写 compact。
   - 如果发现 OpenAI remote compact 依赖，要删除或禁用。
   - 保持 tool streaming/history shape。

7. 清理用户可见命名残留：
   - 技术上准确时，文档、注释、测试可以继续说 OpenAI-compatible protocol。
   - 用户可见产品字符串应该使用 Astral/Astral-Code。
   - 内部 crate 名可以留到后续机械阶段再处理。

## 当前测试策略

当前开发策略是优先推进：

- Rust 编辑后运行 `just fmt`。
- 对变更 crate 或变更行为运行 focused tests。
- 不要每个 slice 都消耗时间和磁盘跑大范围 workspace tests。
- full-suite testing 和大范围 CI 修复留到后续稳定阶段。
- 如果 focused test 触发重编译，等它自然结束，不要 kill Rust 进程。

## 重要安全注意事项

- 不要编辑 `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` 或 `CODEX_SANDBOX_ENV_VAR` 相关代码。
- 不要削弱 sandbox、approval 或 exec-server 行为。
- 不要把 proxy-only hack 作为主架构。
- 不要把真实 API key 写进文件。
- 不要为了让旧测试通过而重新启用 OpenAI/ChatGPT login。
- 在 provider-neutral protocols、Claude-ish tools、OpenAI control-plane removal 都被当前仓库状态证明完成前，不要把 goal 标记为 complete。
