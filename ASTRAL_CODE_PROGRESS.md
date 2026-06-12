# Astral-Code 项目总控记录

最后更新：2026-06-12 本轮

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

最近完成的主要 slice：禁用了 Codex/OpenAI 内置 Statsig 遥测默认外联。Astral 的 OTEL 默认
metrics exporter 现在是 `none`，旧配置里显式写 `statsig` 也会在 `codex-otel` 层解析为 `None`，
源码中不再携带 `https://ab.chatgpt.com/otlp/v1/metrics` 和内置 Statsig key。用户仍可显式配置
自己的 `otlp-http` / `otlp-grpc` endpoint，这是 provider-neutral 自管遥测，不属于 OpenAI 专有控制面。

当前补充 slice：`feedback/upload` 真实上传链路此前已经硬禁用；本轮继续清理残留的 `/feedback`
用户引导文案，避免 Astral 在错误消息、slash 描述或启动 tips 里暗示还能把日志上传给维护者。

已完成的配置命名 slice：`chatgpt_base_url` 已从 Astral 用户可见配置、profile、schema、MCP config、
plugin manager input、app-server 测试 helper 和 core 测试 fixture 中移除，统一收敛为 `hosted_base_url`。
这是新项目语义，不提供旧 Codex 配置字段兼容。

最新补充：Agent Identity 的 hosted 控制面此前已经硬禁用；本轮只把公开函数参数从
`chatgpt_base_url` 改成 `hosted_base_url`，不改 JWT claim 里的 `chatgpt_user_id` /
`chatgpt_account_is_fedramp`，因为那是历史 token payload 形状。

最新补充 2：`astral exec-server --remote` 不再暴露旧的 `--use-agent-identity-auth` flag。远程
exec-server 注册继续只允许 API-key auth，Agent Identity / ChatGPT / PAT 路径保持禁用。

最新补充 3：connectors/apps 的内部 cache key 已从 `chatgpt_base_url` 收敛为 `hosted_base_url`；
同时删除了缺省 `https://chatgpt.com/apps/...` 安装链接 fallback。Astral 现在只保留 connector 自带的显式
`install_url`，没有 provider-neutral app directory 时不会自动生成 ChatGPT 安装链接，也不会触发 ChatGPT
auth elicitation 安装引导。

最新补充 4：apps/connectors 总开关已从 ChatGPT auth gating 中解耦。`Feature::Apps` 现在直接决定
provider-neutral apps 是否开启，不再要求 `CodexAuth::uses_codex_backend()` 或 `is_chatgpt_auth()`。这会让本地
plugin apps、MCP apps 和 app-server `apps/list` 在 Astral API-key/provider-neutral 模式下按 feature flag 工作，
而不是因为没有 ChatGPT 登录态被静默置空。

最新补充 5：tool suggest 的 connector 候选不再读取 legacy ChatGPT connector directory cache。Astral 现在只从
本地 plugin app connector ids 和显式配置的 connector discoverables 构造候选，再和 MCP/accessibility 状态合并；
不会因为 tool suggest 去读取旧 hosted directory cache、`chatgpt_base_url` 或 ChatGPT account/user id。

最新补充 6：cloud-tasks 的 auth/header 主路径已从 ChatGPT-only 改为 Astral API-key/provider-neutral。
`astral cloud` 缺 auth 时提示 `astral login --with-api-key`；已登录时直接用 Astral 支持的本地 auth 生成请求
headers，不再要求 `uses_codex_backend()`，也不再打印 `ChatGPT-Account-Id` 相关日志。`normalize_base_url`
也不再把 `https://chatgpt.com` 自动补成 `/backend-api`；只有用户显式配置带 `/backend-api` 的 backend 时才走
旧 path style。

最新补充 7：core-plugins curated startup sync 删除了 ChatGPT backend export archive fallback。git sync 失败后仍可
尝试 GitHub API zipball fallback，但如果 GitHub HTTP 也失败，将直接失败并保留已有本地 curated snapshot，不再访问
`https://chatgpt.com/backend-api/plugins/export/curated`，也不再维护 backup archive zip / git ref 解析代码。

最新补充 8：core-skills remote skill control-plane 从“禁用 guard 包着旧实现”升级为“只保留 unsupported stub”。
`list_remote_skills(...)` / `export_remote_skill(...)` 不再包含 ChatGPT `/hazelnuts` HTTP client、Codex backend
auth 检查、download zip 校验或解压逻辑；本地 skills runtime 不受影响。

最新补充 9：core-plugins legacy remote featured/mutation client 也已收敛成 disabled stub。旧
`/plugins/featured`、`/plugins/{id}/enable`、`/plugins/{id}/uninstall` HTTP 实现和 ChatGPT auth 检查被删除；
即使 manager featured ids 入口未来被误触发，也只会返回 Astral disabled，不会访问 hosted plugin service。

最新补充 10：app-server API 文档的 auth/account 段已同步到 Astral 行为。README 不再宣称支持 ChatGPT
browser/device-code login、ChatGPT plan/rate-limit 示例或 OpenAI quota 窗口；`account/login/start` 文档现在只
描述 `apiKey`，rate limits / usage 明确为当前 provider-neutral 模式不可用。

最新补充 11：provider request body override 支持 JSON null 删除默认字段。Anthropic Messages 和
OpenAI-compatible chat-completions adapter 现在都会把 `metadata.provider["field"] = null` 解释为从请求体移除
`field`，用于适配不接受 `stream_options`、`stream` 等默认字段的 strict 国内网关。

最新补充 12：model provider 配置新增 `request_body_remove`。TOML 里可以写
`request_body_remove = ["stream_options", "parallel_tool_calls"]`，core 构建 `AgentRequest` 时会把这些字段映射成
provider metadata 里的 JSON null，从而复用 adapter 的删除语义。这样 strict 国内 OpenAI-compatible 网关不需要
vendor-specific 分支，也不需要用户在不可表达 null 的 TOML 里写绕路配置。

最新补充 13：`codex-mcp` 的 hosted apps MCP URL 构造不再内置 ChatGPT 域名特例。Astral 不会再把
`https://chatgpt.com` 或 `https://chat.openai.com` 自动补成 `/backend-api`；只有用户显式配置包含
`/backend-api` 的 hosted backend 时才走旧 `wham/apps` path style。普通 hosted URL 默认走
`/api/codex/apps`。

最新补充 14：app-server 已从 `codex-chatgpt` crate 脱钩。connector helper 上移到
`codex_core::connectors`，app-server 自己持有 provider-neutral 的 workspace settings stub；随后删除了未再被引用的
`codex-rs/chatgpt` legacy crate、它的 BUILD/Cargo/test fixture，以及 root workspace dependency。对应的 connector
行为测试已迁移到 core。

最新补充 15：`AuthDotJson` 内部 API key 字段已从 OpenAI 命名的 `openai_api_key` 收敛为
provider-neutral 的 `api_key`；磁盘 JSON 字段仍是 Astral 自己的 `ASTRAL_API_KEY`，不引入旧 Codex 兼容路径。
本轮同时更新了 login、doctor、app-server fixture 和 remote-control 测试调用点。

最新补充 16：app-server remote-control 的 URL 规范化不再内置 ChatGPT hosted 域名白名单。
`chatgpt.com` / `chatgpt-staging.com` 现在和其他外部域名一样被拒绝；当前只保留 localhost
remote-control 测试/自测路径。remote-control 请求头也从 `chatgpt-account-id` /
`x-codex-installation-id` 收敛为 `x-astral-account-id` / `x-astral-installation-id`，
错误文案改为中性的 hosted account authentication。

最新补充 17：OpenAI Guardian / auto_review 审批控制面已从主审批路由中短路。即使配置
`approvals_reviewer = "auto_review"` 或 MCP/app override 请求 AutoReview，Astral 也不会启动 Guardian
review subagent，权限请求回到普通用户审批事件与现有 sandbox/approval 链路。`strict_auto_review`
响应也会被归一化为 `false`，不再强制后续工具调用进入 Guardian。

最新补充 18：multi-agent / subagent 不再继续 Claude-ish 改名。此前实现过 `Agent` / `SendMessage` /
`TaskStop` 三个薄包装，但复盘后确认 subagent 不是 v1 主路径，薄改名收益低、容易留下半新半旧的模型可见面。
本轮已删除这些 Astral subagent handler 和 schema，multi-agent v2 模型可见工具恢复 Codex 原版
`spawn_agent`、`send_message`、`followup_task`、`wait_agent`、`interrupt_agent`、`list_agents`。
底层 `AgentControl`、thread tree、mailbox、fork/resume、approval/sandbox 继续完整继承 Codex，不做重写。

最新补充 19：subagent 回退切片已做 scoped 编译验证：
`CARGO_INCREMENTAL=0 cargo check --tests -p codex-core -p codex-tools` 通过。随后继续推进 provider
adapter 兼容性：Anthropic Messages adapter 现在只有在请求实际包含 tools 时才默认发送 `tool_choice`；
纯文本/无工具请求不再携带 `tool_choice: { "type": "auto" }`，避免严格 `/anthropic` 兼容网关因为“无 tools
却有 tool_choice”拒绝请求。对应窄测试 `just test -p codex-api anthropic` 通过 6 个测试。

最新补充 20：OpenAI-compatible chat-completions adapter 增加 DeepSeek cache usage 归一化。DeepSeek 官方
usage 字段包含 `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`；Astral 现在会把
`prompt_cache_hit_tokens` 映射到内部 `cache_read_input_tokens`，并在缺少 `prompt_tokens` 时可用 hit+miss
回推输入 token 数。这让后续 cache 命中率、成本估算和 status/usage 展示不会丢失 DeepSeek 官方缓存信息。
对应窄测试 `just test -p codex-api chat_completions` 通过 8 个测试。

最新补充 21：OpenAI-compatible chat-completions stream parser 增加 DeepSeek 风格
`delta.reasoning_content` 支持。该字段现在会映射到内部 `ContentDelta::Reasoning`，并使用独立 block index，
避免 reasoning、正文和 tool call 在 Response event mapper 里互相覆盖。对应窄测试
`just test -p codex-api chat_completions` 通过 9 个测试。

最新补充 22：OpenAI-compatible chat-completions request serializer 对纯文本 user message 改为更保守的
`"content": "..."` 字符串形状，贴近 DeepSeek 和多数国内 OpenAI-compatible 网关的 text-only 入参习惯。
包含图片的 user message 仍保留 OpenAI content parts 数组，确保原版 Codex 可读图/多模态能力不被削弱。
对应窄测试 `just test -p codex-api chat_completions` 通过 10 个测试。

最新补充 23：remote plugin 的用户可见 marketplace 展示名从 `OpenAI Curated Remote` 收敛为
`Astral Curated Remote`，TUI 本地 curated tab 文案也从 `OpenAI Curated` 改为 `Astral Curated`。
manager 的 remote discoverable cache 入口不再用 `uses_codex_backend()` / ChatGPT account id 做 gating；
因为 Astral 当前 remote plugin control-plane 已整体 disabled，默认路径会直接返回空，不再保留“必须是
ChatGPT 后端 auth 才能继续”的语义。对应窄测试：
`just test -p codex-core-plugins build_remote_installed_plugin_marketplaces_from_cache_uses_remote_metadata`
通过 1 个测试；
`just test -p codex-tui plugins_popup_openai_curated_tab_omits_marketplace_in_rows`
通过 1 个测试。后者编译较重，后续非必要不要频繁跑 TUI 单测。

磁盘维护记录：TUI 窄测编译后 `codex-rs/target` 涨到约 109G、磁盘剩余约 44Gi。按用户要求只清理
Astral-Code 项目内较低风险构建缓存，删除 `codex-rs/target/debug/incremental`，释放约 6Gi；
清理后 `codex-rs/target` 约 103G、磁盘剩余约 50Gi。未删除项目外任何文件。

最新补充 24：Guardian / auto_review 旁路审计完成。MCP elicitation、delegated subagent MCP
compatibility path、普通 shell/apply_patch approval 都会先检查
`routes_approval_to_guardian_with_reviewer(...)` 或 `routes_approval_to_guardian(...)`；该函数在 Astral
当前实现中无条件返回 `false`，并已有测试覆盖 `AutoReview` 配置和 app/MCP override 都不能重新启用 Guardian。
`strict_auto_review` 也会在 request-permissions 响应归一化时清掉，不能授予 session 级自动审查能力。
因此当前没有发现 MCP elicitation 绕过普通用户审批而触发 Guardian review subagent 的运行时路径。
剩余 Guardian 类型、analytics、UI notification 和 tests 属于 legacy/dead surface，后续可继续隔离或删除，
但它们不是当前默认外联风险。

最新补充 25：OpenAI hosted-only extensions 默认暴露路径完成审计。`web-search` extension 当前没有安装进
app-server extension registry；`image-generation` extension 虽然仍在 registry 里，但工具暴露被双重 gating：
只有 `config.model_provider.is_openai()` 且当前 auth 使用 Codex backend 时才会注入。因此 Astral API-key、
Anthropic Messages、国内 OpenAI-compatible provider 默认都不会看到这些 hosted-only tool。结论是 v1 暂不需要
额外删除该入口；后续若做完整 hosted-control-plane 瘦身，可把 image-generation 也改成显式 provider/plugin
能力，而不是默认内置能力。

最新补充 26：`codex-memories-write` 后台 memories 任务移除 OpenAI 模型名硬编码。Stage 1 原先默认
`gpt-5.4-mini`，Stage 2 原先默认 `gpt-5.4`；现在两阶段都走“显式 `memories.extract_model` /
`memories.consolidation_model` 优先，否则继承当前 `config.model`，仍无模型则跳过后台模型调用”的路径。
这样保留 Codex 的本地 memories/local compact 工程设计，但不会在 Astral provider-neutral 模式下偷偷 fallback 到
OpenAI 模型名。对应轻量验证已跑：
`just fmt` 通过；
`just test -p codex-memories-write memories_startup_phase1_uses_live_thread_service_tier_and_detached_metadata`
通过 1 个测试。该测试首次编译较重，结束后磁盘剩余约 46Gi，暂未清缓存；后续如继续接近告警线，只清理
`/Users/oines/project/astral-code` 内的低风险构建缓存。

最新补充 27：TUI app connector handoff 文案从 ChatGPT 绑定改为 provider-neutral。`app_link_view` 不再要求
`codex_apps` auth URL 必须属于 `chatgpt.com` / `chatgpt-staging.com`，而是和普通外部 action 一样只接受
HTTPS、禁止 username/password；按钮从 `Install/Manage on ChatGPT` 改成 `Install app` / `Manage app`，
浏览器确认文案也改为通用 app/browser 表述。`chatwidget/plugins.rs` 里的 plugin 安装后 app setup
弹窗同步改成“在浏览器里安装/管理 app”，并移除默认 `help.openai.com/.../apps-in-chatgpt` hyperlink。
相关 app_link snapshot 已手动更新。验证：
`just fmt` 通过；
`cargo check -p codex-tui` 通过。第一次 check 较重，触到 TUI 共享依赖；第二次增量复查约 9 秒通过。
随后按用户要求只清理 Astral-Code 项目内的 `codex-rs/target/debug/incremental`，磁盘剩余从约 45Gi 回到约 49Gi；
未删除项目外任何文件。

最新补充 28：TUI apps/connectors 前端可见性也已从 ChatGPT account gating 中解耦。
`chatwidget/connectors.rs` 的 `connectors_enabled()` 现在只看 `Feature::Apps`，因此 `/apps`、`$app`
mention 候选、session 配置后的 prefetch 和 bottom pane 开关不会因为没有 ChatGPT 登录态而被静默关闭。
同时把一个 apps popup 测试改成“无 ChatGPT account 也能保持 loading 并接收最终 snapshot”的防回归用例。
`model_popups.rs` 中“OpenAI base URL is overridden”的用户可见警告也改成中性的 configured base URL 文案。
验证：
`just fmt` 通过；
`CARGO_INCREMENTAL=0 cargo check -p codex-tui` 通过。磁盘当前剩余约 48Gi；
`codex-rs/target` 约 104G，其中 `debug/deps` 约 100G、`debug/incremental` 为 0B。因为剩余空间仍健康，
暂不删除 `debug/deps`，避免显著拖慢后续开发；后续若再次逼近告警线，只清理 Astral-Code 项目内构建缓存。

最新补充 29：模型 reroute warning 从 OpenAI/ChatGPT cyber 文案改为 provider-neutral。
此前 `server_model != requested_model` 会被无条件解释成“high-risk cyber activity”，并向用户展示
`https://chatgpt.com/cyber` 和 OpenAI cyber-safety 文档链接；这在国产 OpenAI-compatible 网关返回别名模型、
fallback 模型或路由模型时会严重误导。现在 core 新增 `ModelRerouteReason::ProviderModelReroute`，app-server
v2 schema 同步暴露 `providerModelReroute`，session warning 改成“provider returned model X while Y was
requested”。旧 `HighRiskCyberActivity` 变体保留，用于 legacy/error 兼容；compact 的 warning filter 同时识别
旧 OpenAI warning 和新 Astral provider reroute warning，避免它们作为用户消息进入模型上下文。
验证：
`just fmt` 通过；
`just write-app-server-schema` 通过；
`just test -p codex-app-server-protocol` 通过 222 个测试；
`just test -p codex-core safety_check_downgrade` 通过 7 个测试；
`just test -p codex-core collect_user_messages_filters_legacy_warnings` 通过；
`just test -p codex-core process_compacted_history_drops_legacy_warnings` 通过。
本次 core 窄测首次重编较重，用时约 8 分半，后续非必要不要频繁跑 core 测试。测试后磁盘剩余约 41Gi；
已按用户要求仅删除 Astral-Code 项目内 `codex-rs/target/debug/incremental`，清理后剩余约 45Gi，
未删除项目外文件，也未删除 `debug/deps`。

最新补充 30：Claude-ish 核心工具 schema 对照继续推进。对照 `/Users/oines/project/claude-code/tools`
里的 Bash、Read、Write、Edit、Glob、Grep、TodoWrite、AskUserQuestion、ToolSearch 源码后，确认当前
Astral 的 `ToolSearch.max_results`、`Bash.command/timeout/description/run_in_background`、文件工具
`file_path/old_string/new_string/replace_all`、`TodoWrite.todos[].content/status/activeForm` 这些主轨迹字段
基本贴近 Claude Code。发现并修正一个实际错配：Astral `Read` runtime 当前只稳定支持文本文件和本地图片，
PDF/pages 会明确拒绝；因此本轮从模型可见 `Read` schema 中撤掉 `pages` 字段，避免模型按 Claude Code 的
PDF page extraction 习惯调用尚未实现的能力。handler 里仍保留对外部硬塞 `pages` 的显式拒绝分支，保证边界清晰。
验证：
`just fmt` 通过；
`just test -p codex-tools astral` 通过 7 个 Astral schema 相关测试。
本次窄测触发部分共享依赖重编，磁盘剩余约 44Gi；暂不删除 `debug/deps`，只在继续下降时清理
Astral-Code 项目内低风险构建缓存。

最新补充 31：协议层 usage/quota 错误文案去 ChatGPT 化。`codex-protocol` 里 `UsageNotIncluded` 不再提示
“用 ChatGPT plan 升级 Plus”，而是提示当前账号/API key 没有 provider 访问权限；`UsageLimitReachedError`
不再根据 ChatGPT plan 类型输出 Plus/Pro 升级、`chatgpt.com/codex/settings/usage` 或 hosted promo 文案。
现在通用限额只展示 provider-neutral 消息：“检查 provider account、billing 或 model quota”，并继续保留
`resets_at` 的本地化重试时间。workspace credits / spend cap 这类有用结构化错误仍保留原行为。
同时把相关协议测试中的 ChatGPT hosted URL fixture 换成中性的 `provider.example` URL，避免测试样例继续暗示
Astral 默认使用 ChatGPT backend。验证：
`just fmt` 通过；
`just test -p codex-protocol error` 通过 33 个相关测试。
本轮测试后只清理了 Astral-Code 项目内 `codex-rs/target/debug/incremental`（约 435M），磁盘剩余约 44Gi；
未删除项目外文件，也未删除 `debug/deps`。

最新补充 32：TUI 启动阶段的 OpenAI 模型 upsell 提示默认隔离。原 Codex 的 model migration prompt /
model availability NUX 会根据 model catalog 推 `gpt-*` 新模型或迁移目标；这在 Astral 默认 `astral`
provider（DeepSeek/OpenAI-compatible chat-completions）下会给用户错误信号。现在
`prepare_startup_tooltip_override(...)` 和 `handle_model_migration_prompt_if_needed(...)` 都会先检查
`config.model_provider.is_openai()`，非 OpenAI provider 直接不展示这些 OpenAI 模型推广/迁移提示。旧 helper
和显式 OpenAI provider 路径保留，避免大面积删除 TUI 迁移组件；Astral 默认路径不会触发。
验证：
`just fmt` 通过；
`just test -p codex-tui prepare_startup_tooltip_override` 通过 2 个过滤测试。
注意：该 TUI 过滤测试编译成本很高，触发了约 5 分半依赖重编，后续非必要不要再跑 TUI nextest；
测试后只清理了 Astral-Code 项目内 `codex-rs/target/debug/incremental`（约 4.7G），磁盘剩余约 44Gi。

最新补充 33：继续清理用户可见 OpenAI/ChatGPT 出口，但不动 sandbox/exec 行为。完成项：
standalone update 命令和 release notes snapshot 已指向 `oines/astral-code` 与 `ASTRAL_NON_INTERACTIVE`；
npm registry 测试 fixture 从 `@openai/codex` 改为 `astral-code`；TUI Cyber / Trusted Access 提示改成
provider-neutral 的“provider requested additional safety review”，不再展示 `chatgpt.com/cyber`；memories、
MCP 空状态、Windows sandbox 帮助链接改到 Astral 仓库文档；feature flag、profile 冲突、metrics、bwrap
warning 等 CLI/config/sandbox 文案也改为 Astral 文档或 Astral 项目名。`/status` 相关快照同步更新为
`Astral-Code`，移除 ChatGPT usage 链接，并展示 `Model provider: Astral`。
验证：
`just fmt` 通过；
`just test -p codex-tui cyber_policy_error_event` 通过 2 个测试；
`just test -p codex-tui standalone` 通过 4 个测试；
`just test -p codex-tui memories_enable_prompt memories_settings_popup windows_sandbox_required` 通过 6 个测试；
`just test -p codex-tui status_snapshot` 通过 24 个测试；
`just test -p codex-features deprecation` 通过 1 个测试；
`just test -p codex-config profile_v2` 通过 3 个测试。
`just test -p codex-sandboxing bwrap` 因过滤器没有匹配到测试返回 “0 tests”，但 `codex-sandboxing` crate 已编译通过。
本轮误把三个 Rust 测试并行启动，Cargo 锁导致后两个排队；后续 Rust 测试应顺序跑。磁盘当前约 40Gi 可用，
`codex-rs/target/debug/incremental` 为 0B，`debug/deps` 约 108G；按用户要求暂不删除 `debug/deps`，只继续监控。

最新补充 34：core-plugins curated startup sync 已从 OpenAI 默认外联改成 Astral 本地 snapshot-only stub。
原实现会默认 `git ls-remote` / `git fetch` `https://github.com/openai/plugins.git`，失败后还会走 GitHub API
repository/zipball fallback；这条默认外联链路已经删除。现在 `sync_curated_plugins_repo(...)` 只做三件事：
拿 `.tmp/plugins.sync.lock`，如果本地 `.tmp/plugins/.agents/plugins/marketplace.json` 和 `.tmp/plugins.sha`
同时存在则返回本地 sha，否则返回明确错误
`Astral curated plugin startup sync is disabled and no local curated plugins snapshot is available`。
本地 curated marketplace 读取、plugin install 时对本地 sha 的依赖、MCP/plugins runtime 均保留；只是不会默认拉
OpenAI plugins 仓库。内部函数名也从 `sync_openai_plugins_repo` 改为 `sync_curated_plugins_repo`，避免后续误读。
验证：
`just fmt` 通过；
`just test -p codex-core-plugins startup_sync` 通过 5 个测试。
本轮后 `startup_sync.rs` / `startup_sync_tests.rs` 已搜不到 `openai/plugins`、`api.github.com`、`zipball`、
`GitHub HTTP`、`sync_openai_plugins_repo` 等旧入口。磁盘仍约 40Gi 可用，`debug/incremental` 为 0B，
`debug/deps` 约 108G，暂不清理高价值编译缓存。

最新补充 35（2026-06-12 03:14 CST）：继续收敛用户可见文档、模型 catalog 和 fallback prompt。
完成项：
`codex-rs/README.md`、`docs/*`、`codex-rs/app-server-daemon/README.md`、`codex-rs/default.nix` 已改为
Astral 命名、Astral 安装路径、`ASTRAL_HOME`、`astral` CLI 和 provider-neutral auth/config 文案；这些文档中
已不再含 `developers.openai.com`、`github.com/openai`、`chatgpt.com`、`CODEX_HOME` 等旧入口。
`codex-rs/models-manager/models.json` 现在所有 bundled model 都复用第一条 DeepSeek/Astral base instructions 和
model_messages；`upgrade` / `availability_nux` 全部清空；`available_in_plans` 清空；`supports_search_tool` 关闭；
`apply_patch_tool_type` 设为 `null`，避免默认把旧 `apply_patch` 暴露给模型侧工具面。`codex-auto-review` 的展示名改为
`Astral Auto Review`，但 slug 暂保留以降低测试/兼容面冲击；`gpt-5.3-codex` slug 也暂保留，展示名改成 legacy。
`codex-rs/models-manager/prompt.md` 的 fallback 提示词改成 Astral 工具口径：强调 `Bash`、`Read`、`Write`、
`Edit`、`Glob`、`Grep`、`TodoWrite` 等，不再指导模型使用 `update_plan` 或 `apply_patch` 作为模型侧工具。
`codex-rs/models-manager/src/model_info.rs` 的未知模型 personality fallback header 已从 “You are Codex...” 改为
“You are Astral...”。`codex-rs/tui/src/lib.rs` 的新 TUI 日志文件名改成 `astral-tui.log`，同时保留对旧
`codex-tui.log` 的启动清理。
登录/配置运行时文案里 “Stored OpenAI/ChatGPT credentials...” 改为 “Stored upstream hosted credentials...”；
MCP OAuth 注释里的 OpenAI GitHub 链接移除，相关注释改为 Astral。
验证：
`just fmt` 通过；
`just test -p codex-models-manager bundled_models_json_roundtrips` 通过 1 个测试；
`just test -p codex-tui startup_removes_legacy_tui_log_file` 通过 1 个测试。
磁盘从约 40Gi 可用降到约 38Gi 可用，主要来自 Rust 编译缓存增长；仍未清理 `debug/deps`，因为这会显著拖慢后续开发。

最新补充 36（2026-06-12 03:35 CST）：backend-client / cloud-tasks hosted backend 命名继续去 OpenAI 化。
`codex-rs/backend-client/src/client.rs` 不再使用 `ChatGptApi` path style，改为 `HostedApi`；默认 HTTP user-agent
从 `codex-cli` 改为 `astral`。client 也不再生成 `ChatGPT-Account-Id` 或 `X-OpenAI-Fedramp` 这类 OpenAI
专用 header，账号透传改为 Astral 自有的 `Astral-Account-Id`。Cloud tasks 的 debug env 和 CLI 文案此前已经改成
`ASTRAL_CLOUD_TASKS_*` / `astral cloud`，本轮又把 `codex-rs/cloud-tasks-client/src/http.rs` 的 wrapper 方法收敛为
`with_hosted_account_id(...)`。`codex-rs/cloud-tasks/src` / `tests`、`codex-rs/backend-client/src`、
`codex-rs/cloud-tasks-client/src` 范围内已搜不到 `ChatGptApi`、`ChatGPT-Account-Id`、`X-OpenAI-Fedramp`、
`chatgpt.com`、`OpenAI`、`CODEX_CLOUD_TASKS`、`codex cloud`、`Codex Cloud` 等旧控制面关键词。
验证：
`just fmt` 通过；
`CARGO_INCREMENTAL=0 cargo check -p codex-backend-client -p codex-cloud-tasks-client -p codex-cloud-tasks`
通过。磁盘仍约 38Gi 可用，未清理高价值编译缓存。

最新补充 37（2026-06-12 03:47 CST）：`uses_codex_backend` 语义收敛为 `uses_hosted_backend`。
这次是符号级机械重命名，覆盖 `login`、`model-provider`、`models-manager`、`app-server`、`codex-mcp`、
`app-server-transport`、`core-plugins`、`cloud-config`、`core/src/client.rs`、`image-generation-extension` 等调用链。
行为没有变：legacy token / agent identity / personal access token 仍被视作 hosted backend auth，但 Astral 登录入口已经拒绝这些
token-backed hosted 凭据；这个重命名主要是避免后续维护者误把它理解成 Codex/OpenAI 专属后端。注释中 “Codex backend auth”
也改为 hosted backend auth；`get_chatgpt_user_id` 这类旧 JWT claim accessor 暂不改名，因为它牵涉插件/cache 数据结构，
且当前只是用来识别 legacy hosted payload。
验证：
`just fmt` 通过；
`CARGO_INCREMENTAL=0 cargo check -p codex-login -p codex-models-manager -p codex-model-provider -p codex-app-server -p codex-core -p codex-mcp -p codex-app-server-transport -p codex-image-generation-extension -p codex-core-plugins -p codex-cloud-config`
通过。磁盘约 40Gi 可用；`target/debug/incremental` 0B，`target/tmp` 0B，`target/debug/deps` 约 109G，按用户要求暂不删除。

最新补充 38（2026-06-12 04:05 CST）：默认 HTTP client 去掉 ChatGPT/Cloudflare shim 和旧 `CODEX_*` originator fallback。
`codex-rs/codex-client/src/chatgpt_cloudflare_cookies.rs`、`codex-rs/codex-client/src/chatgpt_hosts.rs` 已删除，`codex-client`
不再导出 `with_chatgpt_cloudflare_cookie_store` 或 `is_allowed_chatgpt_host`。`codex-rs/login/src/auth/default_client.rs`
不再读取 `CODEX_INTERNAL_ORIGINATOR_OVERRIDE`，只接受 `ASTRAL_INTERNAL_ORIGINATOR_OVERRIDE`；默认 client 构造也不再经过
ChatGPT Cloudflare cookie shim。旧 OpenAI internal residency header
`x-openai-internal-codex-residency` 改为 Astral 自有的 `x-astral-residency`。
first-party originator 判定从 `codex-tui` / `codex_vscode` / `Codex *`、`codex_atlas` /
`codex_chatgpt_desktop` 收敛为 `astral-tui` / `astral_vscode` / `Astral *`、`astral_atlas` /
`astral_chat_desktop`；`exec` runtime 自报 originator/client_name/OTEL process name 从 `codex_exec` 改为
`astral_exec`，OTEL 低基数字段 allowlist 同步新增 Astral originator 值。相关测试 helper 同步改用 Astral env。
验证：
`just fmt` 通过；
`CARGO_INCREMENTAL=0 cargo check -p codex-client -p codex-login -p codex-exec -p codex-connectors -p codex-otel -p codex-app-server`
通过。磁盘约 39Gi 可用，仍未清理 `target/debug/deps`。

最新补充 39（2026-06-12 04:16 CST）：内部 user-agent API 从 Codex 命名收敛为 Astral 命名。
`get_codex_user_agent()` 已机械重命名为 `get_astral_user_agent()`，调用点覆盖 `mcp-server`、`cloud-tasks`、
`backend-client`、`app-server initialize` 和 websocket tests。上一条补充里的 default HTTP client / originator 清理后，
本轮复查确认以下旧入口在相关范围内已经消失：
`get_codex_user_agent`、`CODEX_INTERNAL_ORIGINATOR_OVERRIDE`、`with_chatgpt_cloudflare_cookie_store`、
`is_allowed_chatgpt_host`、`chatgpt_hosts`、`chatgpt_cloudflare`、
`x-openai-internal-codex-residency`、`codex_atlas`、`codex_chatgpt_desktop`、`connector_openai`。
验证：
`just fmt` 通过；
`CARGO_INCREMENTAL=0 cargo check -p codex-client -p codex-login -p codex-exec -p codex-connectors -p codex-otel -p codex-app-server -p codex-mcp-server -p codex-cloud-tasks -p codex-backend-client`
通过。磁盘约 39Gi 可用，`target/debug/incremental` 和 `target/tmp` 均为 0B。

最新补充 40（2026-06-12 04:29 CST）：auth keyring 与 Astral 数据隔离继续收敛。
`codex-rs/login/src/auth/storage.rs` 的 keyring service 从 `Codex Auth` 改为 `Astral Auth`，这样 Astral 不会读取旧
Codex keychain 项；测试里的示例 home 从 `~/.codex` 改为 `~/.astral-code`，对应 store key 更新为
`cli|f05172cba701f51c`。`find_codex_home()` 的实际实现此前已经只认 `ASTRAL_HOME` / `~/.astral-code`，
没有 `CODEX_HOME` fallback；本轮重点补上 keyring service 这一条容易漏的旧数据入口。
验证：
`just fmt` 通过；
`just test -p codex-login storage` 通过 17 个测试。该测试触发了约 2.7G incremental 缓存；按用户要求只清理
`/Users/oines/project/astral-code/codex-rs/target/debug/incremental`，未碰 `target/debug/deps` 或电脑其他目录。
清理后磁盘约 39Gi 可用，`target/tmp` 为 0B。

最新补充 41（2026-06-12 04:45 CST）：`CODEX_HOME` 环境变量残留清零。
实际 home 解析此前已经只认 `ASTRAL_HOME` / `~/.astral-code`；本轮把测试 harness、MCP tool schema 文案、
Windows sandbox helper 顶层错误日志、network-proxy/TUI 错误文案、skills/theme/pets 注释和测试失败信息中的
`CODEX_HOME` 改为 `ASTRAL_HOME`。关键点：没有修改 `CODEX_SANDBOX_*` 相关代码，也没有改 sandbox policy；Windows
sandbox 变更只影响错误日志找 home 目录时读取的环境变量。复查 `rg -n "CODEX_HOME" codex-rs -g '*.rs'` 已无结果。
验证：
`just fmt` 通过；
`CARGO_INCREMENTAL=0 cargo check -p codex-login -p codex-mcp-server -p codex-windows-sandbox -p codex-network-proxy -p codex-tui -p codex-core-skills -p codex-rmcp-client -p codex-skills-extension -p codex-skills -p codex-app-server-test-client`
通过。磁盘约 38Gi 可用，`target/debug/incremental` 和 `target/tmp` 均为 0B。

最新补充 42（2026-06-12 本轮）：DeepSeek 真实 smoke 和 provider/model 热切换底层链路推进。
OpenAI-compatible chat-completions stream parser 已修正一个真实 DeepSeek 兼容坑：DeepSeek SSE 首个 role-only
chunk 会带 `usage: null`，旧逻辑把它当 usage-only 完成事件，导致 `astral exec` 直接空输出。现在
`usage_from_chat(...)` 显式忽略 JSON null，并新增对应单测；真实 `deepseek-v4-pro` OpenAI-compatible
smoke 已返回 `OK_ASTRAL_OPENAI`。同时确认 provider 下面会有多个模型，不能把 `/model` 继续建成单层模型字符串。
本轮已把 app-server/core/protocol 的 thread settings override 扩成 `(model_provider, model)` 二元组：
`TurnStartParams` / `ThreadSettingsUpdateParams` / `ThreadSettingsOverrides` / `SessionSettingsUpdate` 都能携带
`model_provider`，core 会按 provider id 切换 `ModelProviderInfo`，从而改变后续 turn 的 base URL、auth source、
wire API 和 capability profile。TUI 内部 `AppEvent::UpdateModel` 也已改为 `{ model, model_provider }`，
Plan mode 的 reasoning scope 二级确认会保留 provider，不会在确认阶段丢失。当前旧 `/model` UI 仍只在当前
provider 内切模型，全部传 `model_provider: None`；完整 provider 分组 picker 仍未完成，下一步应做
“provider 列表 -> provider 内 models -> 提交 provider+model”的 UI/状态层。
验证：
`just fmt` 通过；
`just test -p codex-api stream_chunk_ignores_null_usage stream_chunk_maps_deepseek_reasoning_content_delta stream_chunk_maps_deepseek_cache_usage_fields`
通过；
`cargo build -p codex-cli` 通过；
本轮尝试 `CARGO_INCREMENTAL=0 cargo check -p codex-tui --lib`，但 TTY 会话长时间无输出且进程视图未见 cargo/rustc，
已用 Ctrl-C 停止，不能算通过；随后尝试 `just write-app-server-schema` 同样在编译阶段出现 TTY 会话无输出且进程视图
无 cargo/rustc 的异常状态，已 Ctrl-C 停止，schema 生成仍待补。随后按用户要求只删除
`/Users/oines/project/astral-code/codex-rs/target/debug/incremental`，磁盘从约 15Gi 回到约 19Gi 可用，
`codex-rs/target` 约 129G；未删除 `target/debug/deps` 或项目外文件。若后续再次逼近告警线，仍优先只清理
Astral-Code 项目内 incremental 缓存。

最新补充 43（2026-06-12 本轮）：模型能力 catalog 策略重新定案为“完全不内置预设”。
用户明确要求 Astral 不维护任何内置 provider/model 预设，不内置 DeepSeek/Kimi/小米/智谱等模型能力知识库，也不把
`/models` API 的薄 model id 列表当能力真相。后续目标应改为 user-declared models：用户在配置文件中显式声明
provider、model、wire API、context window、input modalities、reasoning/cache/tool 能力等。Astral 只负责：
解析/校验 schema、给出缺配置错误、按声明能力构建请求、在单模态模型下安全剥离图片上下文、保留用户 override。
未知模型默认必须保守，尤其 `input_modalities` 应视为 text-only，不能因为历史 Codex 兼容默认 `text+image` 而把图片塞给
单模态国产模型。完整实现还未完成；当前 `models-manager/models.json` 仍有 bundled entries，需要后续专门移除或降级成
测试 fixture / 空 catalog fallback。

真实 DeepSeek 联调口径：用户已授权后续真实测试可使用 DeepSeek 官方模型 `deepseek-v4-pro`。任何 API key
只能通过临时环境变量或本机未提交配置注入；不得写入仓库、fixture、文档、日志样例或 commit message。真实测试应在
独立实验目录和独立 `ASTRAL_HOME` 下进行，不删除用户文件，不复用正常工作配置。

本轮依赖检查备注：`just bazel-lock-update` 已执行成功；`just bazel-lock-check` 在本机因 `/usr/bin/python3`
是 3.9.6、不支持 `.github/scripts/run_bazel_with_buildbuddy.py` 里的 `str | None` 类型语法而失败。当前
`MODULE.bazel.lock` 没有 diff。

下一步优先继续处理 Agent Identity auth/storage 残留、connectors/apps 里其他 ChatGPT hosted 残留；
provider adapter 方向则继续补 Anthropic/chat-completions fixture 和国内模型兼容选项。
remote-control 主入口已经禁用，不再作为最高优先级，除非后续要把底层 `app-server-transport` 旧模块降级成独立
stub 或删除。
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
- `ReadTaskOutput`
- `SendTaskInput`
- `ListBackgroundTasks`
- `StopBackgroundTask`
- `Read`
- `Write`
- `Edit`
- `Glob`
- `Grep`
- `TodoWrite`
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
- 已有 `codex-rs/core/src/tools/handlers/astral_background_tasks.rs`。
- 已有 `codex-rs/core/src/tools/handlers/astral_file_tools.rs`。
- 已有 `codex-rs/core/src/tools/handlers/astral_todo_write.rs`。
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
- Claude Task v2：先用 Codex Goal/Multi-agent 能力，不追 v2 task 系统。
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

- `ReadTaskOutput` / `SendTaskInput` / `ListBackgroundTasks` / `StopBackgroundTask`
  - 模型侧：把后台命令观察、交互输入、任务找回和终止拆成清晰工具。
  - 运行时：复用 Codex `write_stdin` / UnifiedExec manager / terminate 能力。
  - 这是用户明确喜欢 Codex 的关键体验之一：ffmpeg 等长任务要持续汇报，不要像 Claude Code 那样沉默卡住。

- `Read` / `Write` / `Edit` / `Glob` / `Grep`
  - 运行时：复用 Codex filesystem、patch/search/sandbox context。
  - 支持 `environment_id`，允许目标环境路由。

- `TodoWrite`
  - 运行时：映射到 Codex `update_plan`。
  - 注意：Plan Mode 和 TodoWrite 不是一回事。Plan Mode 是长计划、用户批准后执行；TodoWrite 是执行过程中的 checklist / progress state。

- Multi-agent / subagent
  - 运行时：完整继承 Codex 原版 `multi_agents_v2` / `AgentControl`。
  - 模型侧：不再暴露 Astral 改名的 `Agent` / `SendMessage` / `TaskStop` 包装，直接使用 Codex 原版
    `spawn_agent`、`send_message`、`followup_task`、`wait_agent`、`interrupt_agent`、`list_agents`。
  - 取舍：这块不是 v1 核心 SFT 主路径，薄改名价值不如 Bash/Read/Edit/Grep/TodoWrite，后续不要继续深挖
    Claude Code background task / team / sidechain runtime。

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

## 最近完成的 apps feature gate slice

本轮完成的代码 slice：

> apps/connectors 的总开关不再依赖 ChatGPT/Codex backend auth。

已编辑文件：

- `codex-rs/features/src/lib.rs`
- `codex-rs/features/src/tests.rs`
- `codex-rs/chatgpt/src/connectors.rs`
- `codex-rs/core/src/connectors.rs`
- `codex-rs/core/src/session/turn_context.rs`
- `codex-rs/app-server/src/request_processors/apps_processor.rs`
- `codex-rs/app-server/src/request_processors/plugins.rs`

改动内容：

- 删除 `Features::apps_enabled_for_auth(has_chatgpt_auth)`，替换为 provider-neutral
  `Features::apps_enabled()`。
- `TurnContext::apps_enabled()` 不再读取 `AuthManager::current_auth_uses_codex_backend()`。
- `apps/list` 和 core connector discovery 先按 `Feature::Apps` 判断是否启用，再在确实需要时读取 auth
  用于 workspace/plugin/MCP 细节。
- plugin install 后的 app auth summary 不再要求当前登录态是 ChatGPT auth。
- `codex-chatgpt` 里的 local connector listing 继续只列 plugin apps / MCP accessible connectors，但不再用
  “假装有 ChatGPT auth”的方式绕过旧 gating。

为什么要做：

- Astral 是 provider-neutral/API-key 项目，本地 plugin apps 和 MCP apps 不应该因为没有 ChatGPT 登录态而
  被静默置空。
- 旧 gating 会让 `Feature::Apps` 默认开启也无法生效，只有 ChatGPT/Codex backend auth 才能真正看到 apps。
- 这一步不重新启用 hosted connector directory，也不绕过 workspace/plugin/MCP 的已有安全边界；只是把
  apps 总开关从 OpenAI/ChatGPT auth 概念中解耦。

验证：

- `just fmt`
- `CARGO_INCREMENTAL=0 cargo check --tests -p codex-features -p codex-chatgpt -p codex-core -p codex-app-server`
- `just test -p codex-features apps_follow_feature_flag`

## 最近完成的 tool suggest connector directory cleanup slice

本轮完成的代码 slice：

> tool suggest 不再读取 legacy ChatGPT connector directory cache。

已编辑文件：

- `codex-rs/core/src/connectors.rs`
- `codex-rs/core/src/connectors_tests.rs`
- `ASTRAL_CODE_PROGRESS.md`

改动内容：

- `list_tool_suggest_discoverable_tools_with_auth(...)` 不再调用旧
  `cached_directory_connectors_for_tool_suggest_with_auth(...)`。
- 删除旧 helper，以及它依赖的 `ConnectorDirectoryCacheContext` / `ConnectorDirectoryCacheKey` 读取逻辑。
- tool suggest 的 connector 候选现在由本地 loaded plugin app connector ids 和显式
  `[tool_suggest].discoverables` 生成，再走原有 filter/accessible 合并。
- 相关测试不再创建 dummy ChatGPT auth，改为验证无 directory cache 时仍能按 connector id fallback。

为什么要做：

- hosted connector directory 已经不该作为 Astral 默认控制面存在；tool suggest 不能为了补 connector metadata
  去读取旧 ChatGPT directory cache。
- 旧 helper 会把 `config.chatgpt_base_url`、ChatGPT account id、user id、workspace account 状态重新带回
  discoverable tools 路径。
- 这一步不影响本地 plugin/MCP tool suggest 能力，只移除 legacy hosted directory 参与。

验证：

- `just fmt`
- `CARGO_INCREMENTAL=0 cargo check --tests -p codex-core`
- `just test -p codex-core tool_suggest_uses_connector_id_without_directory_cache tool_suggest_includes_connectors_from_loaded_plugin_apps`

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
- 后续补充：`build_chatgpt_headers(...)` 已重命名为 `build_astral_auth_headers(...)`。
- 后续补充：cloud task auth 不再要求 `auth.uses_codex_backend()`，已登录的 Astral API-key auth 也会生成
  请求 headers。
- 后续补充：缺 auth 文案改为 `astral login --with-api-key`；error log 不再打印
  `ChatGPT-Account-Id`。
- 后续补充：`normalize_base_url(...)` 不再对 `https://chatgpt.com` / `https://chat.openai.com`
  自动追加 `/backend-api`。

为什么要做：

- cloud-tasks 是 remote/cloud control-plane 风险区。
- 旧实现缺少 env 时会直接 fallback 到 `https://chatgpt.com/backend-api`。
- Astral 不应该把 OpenAI hosted backend 当成默认控制面。
- 这一步先拔掉默认外联风险，不重写 cloud task backend 协议。

仍需后续处理：

- cloud tasks 的 URL parser 和部分 formatter 测试仍保留 `chatgpt.com` fixture，用于覆盖旧 URL 解析行为；
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

## 最近完成的 core-skills remote skill stub slice

本轮完成的代码 slice：

> `core-skills/src/remote.rs` hosted remote skill API 不再保留 ChatGPT `/hazelnuts` client 实现；
> Astral 只保留当前调用方需要的 unsupported API 边界。

已编辑文件：

- `codex-rs/core-skills/src/remote.rs`
- `codex-rs/core-skills/src/remote_tests.rs`
- `ASTRAL_CODE_PROGRESS.md`

改动内容：

- `list_remote_skills(...)` / `export_remote_skill(...)` 现在直接返回 Astral disabled。
- 删除旧 ChatGPT `/hazelnuts` HTTP 请求构造、Codex backend auth 检查、auth header 映射、download zip payload
  校验、zip 解压和路径防穿越 helper。
- 测试继续确认 list/export 都在缺少 auth 时返回 disabled；测试 fixture 不再使用 `chatgpt.example/backend-api`。

为什么要做：

- remote skill client 虽然此前已经被 guard 禁用，但文件里仍保留完整 ChatGPT hosted `/hazelnuts` API 客户端。
- Astral 的 skill runtime 要保留，但 hosted skill marketplace/export 控制面不能默认保留旧 ChatGPT 后端语义。
- 这一步和 remote plugin guard 保持同一原则：本地 skills/plugins/MCP 不动，OpenAI hosted 分发面默认切断。

后续风险：

- `RemoteSkillScope` / `RemoteSkillProductSurface` 等类型仍存在，用于当前编译边界；它们后续可随调用方一起删。
- `codex-core-skills` 的 Cargo 依赖里仍有旧实现留下的依赖，后续做依赖清理时再配合 Bazel lock 更新。
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

## 最近完成的 core-plugins remote legacy stub slice

本轮完成的代码 slice：

> `core-plugins/src/remote_legacy.rs` 不再保留旧 featured plugin fetch 和 enable/uninstall mutation
> HTTP client；只保留当前 manager 编译边界需要的 disabled stub。

已编辑文件：

- `codex-rs/core-plugins/src/remote_legacy.rs`
- `codex-rs/core-plugins/src/remote_legacy_tests.rs`
- `ASTRAL_CODE_PROGRESS.md`

改动内容：

- `fetch_remote_featured_plugin_ids(...)` 直接返回 `RemotePluginFetchError::ControlPlaneDisabled`。
- `enable_remote_plugin(...)` / `uninstall_remote_plugin(...)` 直接返回
  `RemotePluginMutationError::ControlPlaneDisabled`。
- 删除旧 `/plugins/featured`、`/plugins/{id}/enable`、`/plugins/{id}/uninstall` URL 构造、reqwest 请求、
  ChatGPT/Codex backend auth 校验、auth header 注入、mutation response 校验和 JSON decode。
- 新增三条窄测试，防止 legacy 入口重新触碰 hosted control-plane。

为什么要做：

- manager 的 featured plugin ids 路径已经被 `remote_plugin_background_sync_available() == false` no-op，
  但底层 legacy 模块仍保留完整 OpenAI hosted plugin service client。
- Astral 不需要旧 ChatGPT plugin marketplace 的 featured ids、enable/uninstall mutation。
- 这一步继续把“禁用但保留实现”的旧控制面变成“实现本身不可联网”的 stub。

后续风险：

- `remote_legacy` 模块名和 manager 里的 featured ids cache 结构仍存在，后续可以继续删调用链和 cache 类型。
- remote plugin catalog/share/detail 的新模块里仍有大量旧类型和测试 fixture，虽然当前入口已 disabled；
  完全删除需要单独切片，避免一次性打爆 app-server protocol 和 plugin list/read 测试。

## 最近完成的 app-server auth README cleanup slice

本轮完成的代码 slice：

> app-server README 的 auth/account 文档同步到 Astral 当前真实行为，不再把旧 ChatGPT login/control-plane
> 描述成可用 API。

已编辑文件：

- `codex-rs/app-server/README.md`
- `ASTRAL_CODE_PROGRESS.md`

改动内容：

- Auth endpoints 介绍从 Codex/ChatGPT account surface 改为 Astral auth surface。
- Authentication modes 只保留 `apiKey` 作为 Astral active supported mode。
- 删除 ChatGPT browser login、ChatGPT device-code login、ChatGPT login cancel flow 示例。
- `account/login/start` 文档改为只支持 `apiKey`。
- `account/updated` 文档改为只承诺 `apikey` 或 `null`。
- Rate limits / token usage 文档改为当前返回 `invalid_request`，不再展示 OpenAI quota window 示例。

为什么要做：

- protocol 里的 `LoginAccountParams` 实际已经只有 `ApiKey`，README 仍保留旧 ChatGPT login 文档会误导
  app-server 客户端继续调用已删除的 OpenAI auth 控制面。
- Astral 是 provider-neutral 项目，不能在公开 API 文档里把 ChatGPT OAuth 和 OpenAI quota 描述成主路径。

后续风险：

- app-server README 其他段落仍有 remote plugin、attestation、ChatGPT install URL 等旧例子，后续继续分段清理。
- schema 里 `AuthMode` 仍保留 legacy variants 用于识别/拒绝旧 payload，不代表 Astral 支持这些登录方式。

## 最近完成的 provider body override removal slice

本轮完成的代码 slice：

> provider-specific request body override 支持用 JSON null 删除 adapter 默认字段，增强国内 OpenAI-compatible
> / Anthropic-compatible 网关兼容性。

已编辑文件：

- `codex-rs/codex-api/src/agent_adapters/chat_completions.rs`
- `codex-rs/codex-api/src/agent_adapters/chat_completions_tests.rs`
- `codex-rs/codex-api/src/agent_adapters/anthropic.rs`
- `codex-rs/codex-api/src/agent_adapters/anthropic_tests.rs`
- `ASTRAL_CODE_PROGRESS.md`

改动内容：

- `apply_provider_body_overrides(...)` 从单纯 insert 改为：value 是 JSON null 时删除对应字段，否则覆盖/新增。
- chat-completions 测试覆盖删除默认 `stream_options`，同时保留新增 `temperature`。
- Anthropic Messages 测试覆盖删除默认 `stream`，同时保留新增 `temperature`。

为什么要做：

- 国内 OpenAI-compatible 网关常见问题是对 OpenAI 细字段支持不完整，例如不接受 `stream_options` 或某些
  Anthropic request body 字段。
- 继续保持 Astral 的 provider-neutral IR，不为某个厂商硬编码分支；由 provider 配置选择保留、覆盖或删除字段。

后续风险：

- TOML 原生不方便表达 null；如果用户配置层无法写出 JSON null，后续可以增加显式 remove list，例如
  `request_body_remove = ["stream_options"]`，再映射成 `Value::Null`。
- 仍需真实 DeepSeek / Anthropic-compatible gateway fixture 验证哪些字段默认应该保留、哪些应该可选。

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

## 最近完成的 curated plugin startup archive fallback slice

本轮完成的代码 slice：

> core-plugins curated startup sync 不再回退到 ChatGPT hosted export archive。

已编辑文件：

- `codex-rs/core-plugins/src/startup_sync.rs`
- `codex-rs/core-plugins/src/startup_sync_tests.rs`
- `ASTRAL_CODE_PROGRESS.md`

改动内容：

- 删除硬编码 `https://chatgpt.com/backend-api/plugins/export/curated`。
- `sync_openai_plugins_repo_with_transport_overrides(...)` 不再接收 backup archive URL。
- Git sync 失败后仍可尝试 GitHub API zipball fallback；GitHub HTTP 也失败时直接返回错误。
- 如果本地已有 curated snapshot，失败时只保留本地 snapshot，不再尝试 hosted archive bootstrap。
- 删除 backup archive zip 下载、export metadata 解析、archive 内 `.git/HEAD` ref 解析和对应测试。

为什么要做：

- 这是一个明确的 ChatGPT hosted backend fallback，属于 OpenAI 专有控制面残留。
- Astral 不能在本地 plugin startup sync 失败时悄悄去 `chatgpt.com/backend-api` 拉兜底 archive。
- 这一步只拆 ChatGPT backend fallback，不扩大到完整删除 curated GitHub sync；后者会影响 openai-curated
  marketplace 的本地缓存/安装语义，适合单独评估和分阶段迁移到 Astral marketplace。

验证：

- `just fmt`
- `CARGO_INCREMENTAL=0 cargo check --tests -p codex-core-plugins`
- `just test -p codex-core-plugins sync_openai_plugins_repo_keeps_snapshot_when_github_http_fails`

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
- 旧 ChatGPT app install URL fallback 已在 connectors/apps slice 中删除；如果后续
  `just test -p codex-app-server auth` 仍失败，需要按当前 Astral disabled/local-only 语义重新审查具体断言。
- 本机 `just bazel-lock-check` 的 Unix 包装脚本会调用
  `.github/scripts/run_bazel_with_buildbuddy.py`，该脚本使用 Python 3.10+ 的 `type | None` 注解语法；
  当前 `/usr/bin/python3` 是 3.9.6，会在真正执行 Bazel 前 TypeError。直接执行
  `bazel mod deps --lockfile_mode=error` 可以完成 lockfile 校验。
- `just test -p codex-app-server remote_control` 的旧失败来源已在 app-server 暴露层处理：测试改为验证
  Astral disabled 语义。底层 `app-server-transport` remote-control 旧模块仍待后续降级或删除。

### 最新补充 42（2026-06-12 05:05 CST）

完成 `ASTRAL_API_KEY` 相关 auth env telemetry 和内部 AuthManager 开关命名收口：

- `AuthEnvTelemetry` / `AuthEnvTelemetryMetadata` 字段从
  `codex_api_key_env_*` 改为 `astral_api_key_env_*`。
- `AuthManager` 内部开关和方法从 `enable_codex_api_key_env` /
  `codex_api_key_env_enabled()` 改为 `enable_astral_api_key_env` /
  `astral_api_key_env_enabled()`。
- feedback/OTEL/model-provider 请求日志 tag 从
  `auth.env_codex_api_key_*` / `auth_env_codex_api_key_*`
  改为 `auth.env_astral_api_key_*` / `auth_env_astral_api_key_*`。
- 清理了 `exec` auth env 测试函数名中的旧 Codex 命名。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check -p codex-login -p codex-feedback -p codex-otel -p codex-model-provider -p codex-core -p codex-exec`
  通过。
- 磁盘复查：`/Users/oines/project/astral-code` 所在卷仍约 38Gi 可用；`target/debug/incremental`
  和 `target/tmp` 为 0B，`target/debug/deps` 约 109G，暂不删除以免严重拖慢后续开发。

### 最新补充 43（2026-06-12 05:18 CST）

完成 MCP connector 授权提示的 provider-neutral/Astral 文案清理：

- `codex-mcp/src/auth_elicitation.rs` 不再提示用户去 ChatGPT 重新连接 connector，也不再说
  “在 Codex 中使用”。
- 授权提示改为 “your connector provider” / “Astral” 口径，保留 MCP connector auth
  elicitation 的原有流程和 metadata shape。
- 单元测试里的 fixture URL 从 `chatgpt.com/apps/...` 改为 `apps.example/...`，避免测试继续固化
  OpenAI/ChatGPT 专有入口。
- 运行了 `just fmt`。
- 运行了轻量编译检查：`CARGO_INCREMENTAL=0 cargo check -p codex-mcp` 通过。
- 磁盘复查：仍约 38Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 44（2026-06-12 05:32 CST）

完成 Astral Cloud Tasks 的剩余 user-agent / CLI copy 收口：

- `codex-cloud-tasks` 的运行时 user-agent suffix 从 `codex_cloud_tasks_*` 改为
  `astral_cloud_tasks_*`。
- cloud tasks fallback user-agent 从 `codex-cli` 改为 `astral`。
- 保留现有 cloud task 功能边界：release 路径仍要求显式配置
  `ASTRAL_CLOUD_TASKS_BASE_URL`；debug mock 才使用 localhost `/backend-api`。
- 顺手确认 `ASTRAL_CLOUD_TASKS_MODE` / `ASTRAL_CLOUD_TASKS_FORCE_INTERNAL` 已经替代旧
  `CODEX_CLOUD_TASKS_*`。
- 运行了 `just fmt`。
- 运行了轻量编译检查：`CARGO_INCREMENTAL=0 cargo check -p codex-cloud-tasks` 通过。
- 磁盘复查：仍约 38Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 45（2026-06-12 05:44 CST）

完成 CLI remote auth token env 测试口径的 Astral 化：

- `codex-rs/cli/src/main.rs` 中 `--remote-auth-token-env` 相关测试/fixture 从
  `CODEX_REMOTE_AUTH_TOKEN` 改为 `ASTRAL_REMOTE_AUTH_TOKEN`。
- 这是用户可见/可复制的 env 名称清理，不改变 remote control、app-server、token 读取函数或
  底层边界行为。
- 运行了 `just fmt`。
- 运行了轻量编译检查：`CARGO_INCREMENTAL=0 cargo check -p codex-cli` 通过。
- 磁盘复查：仍约 38Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B，暂不清理。

### 最新补充 46（2026-06-12 05:55 CST）

完成 app-server managed config debug/test env 的 Astral 化：

- `codex-rs/app-server/src/main.rs` 中 debug-only managed config env 从
  `CODEX_APP_SERVER_MANAGED_CONFIG_PATH` / `CODEX_APP_SERVER_DISABLE_MANAGED_CONFIG`
  改为 `ASTRAL_APP_SERVER_MANAGED_CONFIG_PATH` / `ASTRAL_APP_SERVER_DISABLE_MANAGED_CONFIG`。
- app-server integration test harness 和 strict/config RPC 测试同步改为新的 `ASTRAL_APP_SERVER_*`
  名称。
- 这是 app-server 测试/调试入口命名清理，不改变 managed config layer 的加载、禁用、strict config
  或 transport 行为。
- 运行了 `just fmt`。
- 运行了轻量编译检查：`CARGO_INCREMENTAL=0 cargo check -p codex-app-server` 通过。
- 磁盘复查：仍约 38Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B；
  `target/debug/deps` 约 109G，暂不删除以免显著拖慢开发。

### 最新补充 47（2026-06-12 06:13 CST）

完成 Astral 自定义 CA 环境变量迁移：

- 专属 CA override 从 `CODEX_CA_CERTIFICATE` 改为 `ASTRAL_CA_CERTIFICATE`。
- 覆盖范围包括：
  - `codex-client` 的 reqwest/rustls shared custom CA 逻辑、错误提示、日志字段和测试；
  - `custom_ca_probe` 子进程测试探针 env；
  - `astral doctor` 的网络环境检查；
  - `network-proxy` 向子工具链传播 CA bundle 的 env key 列表；
  - `login` 默认 HTTP client 注释。
- 仍保留通用 fallback `SSL_CERT_FILE`，并保持 “Astral override 优先，SSL_CERT_FILE 其次”
  的原有 precedence 行为。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check -p codex-client -p codex-cli -p codex-network-proxy`
  通过。
- 残留扫描确认没有 `CODEX_CA_CERTIFICATE` / `CODEX_CUSTOM_CA_PROBE_*`。
- 磁盘复查：约 37Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B，暂不清理。

### 最新补充 48（2026-06-12 06:32 CST）

完成 app-server test client 的外部 env / CLI 口径 Astral 化：

- `codex-rs/app-server-test-client/src/lib.rs` 中：
  - `CODEX_BIN` -> `ASTRAL_BIN`；
  - `CODEX_APP_SERVER_URL` -> `ASTRAL_APP_SERVER_URL`；
  - `CODEX_E2E_MODEL` -> `ASTRAL_E2E_MODEL`；
  - 默认 spawned CLI 从 `codex` 改为 `astral`；
  - 用户可见 flag/help/error 从 `--codex-bin` / `codex app-server` 改为
    `--astral-bin` / `astral app-server`。
- 保留 Cargo package/bin 名称 `codex-app-server-test-client`，避免这一轮牵出 workspace/bazel/CI
  级联改名；后续可以作为纯机械命名阶段单独处理。
- 运行了 `just fmt`。
- 运行了轻量编译检查：`CARGO_INCREMENTAL=0 cargo check -p codex-app-server-test-client`
  通过。
- 残留扫描确认该文件内没有 `CODEX_BIN` / `CODEX_APP_SERVER_URL` / `CODEX_E2E_MODEL`
  / `--codex-bin`。
- 磁盘复查：约 38Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 49（2026-06-12 06:40 CST）

完成 bundled sample skills 中 home 路径和 Astral 口径迁移：

- `codex-rs/skills/src/assets/samples/**` 中面向模型/用户的 `$CODEX_HOME`、
  `~/.codex`、`.codex/skills` 示例改为 `$ASTRAL_HOME`、`~/.astral-code`、
  `.astral-code/skills`。
- 覆盖 `imagegen`、`skill-installer`、`skill-creator`、`plugin-creator` 等 sample skill 文案。
- `plugin-creator` 中 `.codex-plugin/plugin.json` 仍保留，因为这更像现有插件 manifest 目录格式，
  不在本轮 home/env 命名清理范围内。
- 运行了 `just fmt`。
- 这是文档/技能素材改动，未跑 Rust 编译。
- 磁盘复查：约 38Gi 可用；未清理任何非 Astral-Code 目录。

### 最新补充 50（2026-06-12 06:47 CST）

完成 app-server / network-proxy / memories / responses-api-proxy README 的旧 home 路径清理：

- `app-server/README.md` 中 `$CODEX_HOME/app-server-control`、`CODEX_HOME/memories`、
  `/Users/*/.codex/...` 示例改为 `$ASTRAL_HOME` / `~/.astral-code`。
- `network-proxy/README.md` 中 managed MITM CA bundle 路径从 `$CODEX_HOME/proxy`
  改为 `$ASTRAL_HOME/proxy`。
- `memories/README.md` 中 memories git 路径改为 `~/.astral-code/memories/.git`。
- `responses-api-proxy/README.md` 的 legacy 示例配置路径改为 `~/.astral-code/config.toml`；
  该组件仍作为 legacy 残留，后续需要决定删除或隔离。
- 运行了 `just fmt`。
- 这是文档改动，未跑 Rust 编译。
- 磁盘复查：约 38Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 51（2026-06-12 06:55 CST）

完成 `codex-rs` 内旧 home/path fixture 的 Astral 化：

- 将测试/注释/示例中的 `~/.codex`、`/.codex/`、`.codex/skills`、`.codex/config.toml`
  路径迁移到 `~/.astral-code`、`/.astral-code/`、`.astral-code/skills`、
  `.astral-code/config.toml`。
- 覆盖 message history、rollout、arg0 `.env`、analytics/core-skills/core-plugins/tui
  测试 fixture、sandbox-summary、external-agent migration 等路径字符串。
- 保留 `.codex-plugin` manifest 目录名不变，避免混淆插件格式迁移和 Astral home 迁移。
- 运行了 `just fmt`。
- 复查 `rg -n "~/.codex|/\\.codex/|\\.codex/config|\\.codex/skills|CODEX_HOME" codex-rs`
  已无结果。
- 这是注释/测试 fixture/文档字符串迁移，未跑 Rust 大范围测试；后续统一收敛时再覆盖。
- 磁盘复查：约 38Gi 可用。

### 最新补充 52（2026-06-12 07:08 CST）

完成安装/doctor 管理器 env 命名迁移：

- `CODEX_MANAGED_BY_NPM` -> `ASTRAL_MANAGED_BY_NPM`。
- `CODEX_MANAGED_BY_BUN` -> `ASTRAL_MANAGED_BY_BUN`。
- `CODEX_MANAGED_PACKAGE_ROOT` -> `ASTRAL_MANAGED_PACKAGE_ROOT`。
- 覆盖 `doctor` 安装检查、doctor updates 文案和 `install-context` 的运行时安装来源判断。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check -p codex-cli -p codex-install-context` 通过。
- 该 check 顺带覆盖了 `codex-skills`、`codex-message-history`、`codex-arg0`、`codex-core-skills`、
  `codex-core-plugins`、`codex-tui` 等多项刚刚 touched 的 crate 编译。
- 残留扫描确认 `codex-rs` 中没有 `CODEX_MANAGED_BY_NPM` / `CODEX_MANAGED_BY_BUN` /
  `CODEX_MANAGED_PACKAGE_ROOT`。
- 磁盘复查：约 38Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 53（2026-06-12 07:25 CST）

完成 shell/unified exec 注入环境变量的 Astral 化：

- `CODEX_THREAD_ID` -> `ASTRAL_THREAD_ID`。
- `CODEX_CI` -> `ASTRAL_CI`。
- Rust 常量从 `CODEX_THREAD_ID_ENV_VAR` 改为 `ASTRAL_THREAD_ID_ENV_VAR`，覆盖
  `protocol::shell_environment`、`core::exec_env`、unified exec process manager、runtime snapshot
  restore 和相关测试。
- 保持注入时机、thread id 传递、shell snapshot 恢复和 UnifiedExec 行为不变；只改命名。
- 明确没有修改 `CODEX_SANDBOX_*`，这类 sandbox 边界 env 继续按原仓库约束保留。
- 运行了 `just fmt`。
- 运行了轻量编译检查：`CARGO_INCREMENTAL=0 cargo check -p codex-protocol -p codex-core`
  通过。
- 残留扫描确认 `codex-rs` 中没有 `CODEX_THREAD_ID` / `CODEX_CI`。
- 磁盘复查：约 38Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 54（2026-06-12 07:43 CST）

完成核心 per-turn / client metadata header 的 Astral 化：

- `x-codex-installation-id` -> `x-astral-installation-id`。
- `x-codex-turn-state` -> `x-astral-turn-state`。
- `x-codex-turn-metadata` -> `x-astral-turn-metadata`。
- `x-codex-parent-thread-id` -> `x-astral-parent-thread-id`。
- `x-codex-window-id` -> `x-astral-window-id`。
- `x-codex-ws-stream-request-start-ms` -> `x-astral-ws-stream-request-start-ms`。
- `x-codex-beta-features` -> `x-astral-beta-features`。
- Rust 常量同步从 `X_CODEX_*` 改为 `X_ASTRAL_*`，覆盖 core client、MCP turn metadata、
  websocket client metadata、app-server v2 tests、responses-api-proxy dump fixture 等主路径。
- 该切片只迁移 Astral 自己发送/回放的 per-turn metadata header；OpenAI/Codex 后端 rate-limit
  header、legacy proxy 兼容 header 和 remote-control 握手 header 暂未在这一刀处理。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check -p codex-core -p codex-api -p codex-app-server -p codex-responses-api-proxy`
  通过。
- 残留扫描确认没有 `x-codex-installation-id` / `x-codex-turn-state` /
  `x-codex-turn-metadata` / `x-codex-parent-thread-id` / `x-codex-window-id` /
  `x-codex-ws-stream-request-start-ms` / `x-codex-beta-features`。
- 磁盘复查：约 36Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 55（2026-06-12 08:14 CST）

完成 legacy remote plugin control-plane 的 OpenAI/ChatGPT 语义收敛：

- `core-plugins/src/remote.rs` 中的 hosted auth guard 从 `ensure_chatgpt_auth` 改为
  `ensure_hosted_auth`。
- remote plugin catalog 的错误消息从 `chatgpt authentication required...` 改为
  `hosted authentication required for legacy remote plugin catalog...`。
- curated remote collection 函数从
  `fetch_openai_curated_remote_collection_marketplace` 改为
  `fetch_astral_curated_remote_collection_marketplace`，并同步 app-server 调用点。
- remote plugin 请求里残留的产品 SKU 常量从 `CODEX_PRODUCT_SKU = "codex"` 改为
  `ASTRAL_PRODUCT_SKU = "astral-code"`。
- 该模块仍保持 `remote_plugin_control_plane_disabled() == true`，没有重新启用 legacy hosted
  remote plugin control-plane，也没有触碰 sandbox / exec / approval 边界。
- 残留扫描确认该 slice 中没有
  `ensure_chatgpt_auth` / `fetch_openai_curated_remote_collection_marketplace` /
  `OPENAI_CURATED_REMOTE_COLLECTION_KEY` / `CODEX_PRODUCT_SKU` /
  `chatgpt authentication`。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check -p codex-core-plugins -p codex-app-server`
  通过。
- 磁盘复查：约 36Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 56（2026-06-12 08:20 CST）

完成 auth/client 侧一刀 ChatGPT 语义收敛：

- `AuthMode::has_chatgpt_account()` 改为 `AuthMode::has_legacy_hosted_account()`。
- `CodexAuth::is_chatgpt_auth()` 改为 `CodexAuth::is_legacy_hosted_account_auth()`。
- `models-manager` 里依赖该判断的 remote model source-of-truth 逻辑同步改名，并把注释改为
  legacy hosted account auth。
- TUI app-server account update 路径同步从 `AuthMode::has_chatgpt_account` 改为
  `AuthMode::has_legacy_hosted_account`。
- `core/src/client.rs` 里 401 恢复注释从 ChatGPT token refresh 改为 external API-key auth
  refresh；auth telemetry 中旧 token-backed 模式从 `"Chatgpt"` 改为 `"LegacyHosted"`。
- 保留 `AuthMode::Chatgpt` wire variant，当前仍作为 legacy payload 的识别/拒绝标记；没有重新启用
  OAuth/ChatGPT 登录，也没有引入旧 Codex 数据兼容读取。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check -p codex-app-server-protocol -p codex-login -p codex-core -p codex-tui`
  通过。
- 磁盘复查：约 37Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 57（2026-06-12 08:28 CST）

完成 backend/cloud-tasks 侧一刀 OpenAI/Codex 命名空间收敛：

- 审计 `backend-client`：默认不会把 ChatGPT host 隐式重写为 `/backend-api`；只有显式传入包含
  `/backend-api` 的 base URL 时才使用 hosted path style。
- `backend-client` 相关注释从 “ChatGPT hosts” 改为 legacy hosted roots，行为不变。
- `cloud-tasks-client` 的 `CODEX_STARTING_DIFF` 改为 `ASTRAL_STARTING_DIFF`，不保留旧 env fallback。
- `cloud-tasks` TUI 的 `CODEX_TUI_ROUNDED` 改为 `ASTRAL_TUI_ROUNDED`，不保留旧 env fallback。
- `cloud-tasks` 仍要求 `ASTRAL_CLOUD_TASKS_BASE_URL`；debug mock 只默认到 localhost，不会默认访问
  `chatgpt.com/backend-api`。
- 残留扫描确认没有 `CODEX_STARTING_DIFF` / `CODEX_TUI_ROUNDED` / `ChatGPT hosts`。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check -p codex-backend-client -p codex-cloud-tasks-client -p codex-cloud-tasks`
  通过。
- 磁盘复查：约 37Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 58（2026-06-12 08:33 CST）

完成 Agent Identity 专有 hosted auth 残留的一刀隔离：

- 审计 `agent-identity`：`fetch_agent_identity_jwks` 和 `register_agent_task` 已经先经过
  `hosted_agent_identity_control_plane_disabled() == true`，默认不会访问外部 hosted control-plane。
- `login` 层的 `from_agent_identity_jwt` 当前仍直接返回 unsupported，不会恢复旧 Agent Identity 登录。
- 将 Agent Identity JWT issuer 从真实 `chatgpt.com/codex-backend/agent-identity` 改为
  `https://legacy-hosted.invalid/agent-identity`，避免 Astral 代码继续信任真实 OpenAI issuer。
- Agent Identity JWKS URL 测试里的真实 `chatgpt.com/backend-api` 改为 `hosted.example/backend-api`。
- 残留扫描确认 `codex-rs/agent-identity/src/lib.rs` 中没有 `chatgpt.com` / `codex-backend`。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check -p codex-agent-identity -p codex-model-provider`
  通过。
- 磁盘复查：约 37Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 59（2026-06-12 08:38 CST）

完成 app-server remote-control 的一刀审计和残留收敛：

- 审计确认 app-server 启动层已经强制将 legacy hosted remote-control 置为 disabled；即使传入
  `--remote-control`，也只记录警告并继续以 disabled 状态启动。
- `RemoteControlRequestProcessor` 的 enable / pairing / clients list / revoke 入口继续直接返回
  `legacy hosted remote control is disabled in Astral until a provider-neutral control plane exists`。
- remote-control transport 里的订阅游标 header 从 `x-codex-subscribe-cursor` 改为
  `x-astral-subscribe-cursor`。
- remote-control URL 归一化测试中用于“非 localhost 必须拒绝”的真实 OpenAI/ChatGPT 域名替换为
  通用 `hosted.example` / `remote.example` / `localhost.evil.example` 域名，保留安全断言。
- 残留扫描确认 `app-server-transport/src/transport/remote_control` 中没有
  `x-codex-subscribe-cursor` / `chatgpt.com` / `api.chatgpt-staging.com` /
  `chat.openai.com` / `evilchatgpt`。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check -p codex-app-server-transport`
  通过。
- 磁盘复查：约 37Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 60（2026-06-12 08:41 CST）

完成协议调试 / realtime fixture 中真实 ChatGPT backend URL 的替换：

- `response-debug-context` 测试里的
  `https://chatgpt.com/backend-api/codex/models` /
  `https://chatgpt.com/backend-api/codex/responses` 改为
  `https://hosted.example/backend-api/codex/...`。
- `codex-api` bridge/realtime tests 里的
  `https://chatgpt.com/backend-api/codex` 改为
  `https://hosted.example/backend-api/codex`。
- 保留 `/backend-api/codex` 路径形状，用于测试 backend-style URL 分支，但不再使用真实 OpenAI /
  ChatGPT 域名。
- 残留扫描确认 `response-debug-context/src` 和 `codex-api/src` 中没有 `chatgpt.com/backend-api`
  或真实 `chatgpt.com` fixture。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check -p codex-api -p codex-response-debug-context`
  通过。
- 磁盘复查：约 37Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 61（2026-06-12 08:46 CST）

完成 remote-control websocket handshake header 的 Astral 化：

- `app-server-transport/src/transport/remote_control/websocket.rs` 中运行时握手 header 改名：
  - `x-codex-server-id` -> `x-astral-server-id`
  - `x-codex-name` -> `x-astral-name`
  - `x-codex-protocol-version` -> `x-astral-protocol-version`
- 同步更新 `app-server-transport/src/transport/remote_control/tests.rs` 的断言。
- 复查 `app-server-transport/src/transport/remote_control`，确认旧 websocket handshake header 名不再残留。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check -p codex-app-server-transport`
  通过。
- 磁盘复查：约 37Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 62（2026-06-12 08:57 CST）

完成一组低风险、用户可见 / 运行时调试命名的 Astral 化：

- `codex-exec` 的纯文本启动摘要从 `OpenAI Codex v...` 改为 `Astral-Code v...`。
- TUI 调试 session log 环境变量改名：
  - `CODEX_TUI_RECORD_SESSION` -> `ASTRAL_TUI_RECORD_SESSION`
  - `CODEX_TUI_SESSION_LOG_PATH` -> `ASTRAL_TUI_SESSION_LOG_PATH`
- TUI keyboard enhancement 禁用开关改名：
  - `CODEX_TUI_DISABLE_KEYBOARD_ENHANCEMENT` -> `ASTRAL_TUI_DISABLE_KEYBOARD_ENHANCEMENT`
- git patch 内部调试配置环境变量改名：
  - `CODEX_APPLY_GIT_CFG` -> `ASTRAL_APPLY_GIT_CFG`
- git baseline 初始化提交身份从
  `Codex <noreply@openai.com>` 改为
  `Astral-Code <noreply@astral-code.dev>`。
- 复扫确认上述旧 env / 输出名 / OpenAI 邮箱不再残留于非 snapshot 源码。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check -p codex-exec -p codex-tui -p codex-git-utils`
  通过，用时 4m36s。
- 磁盘复查：约 37Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 63（2026-06-12 09:01 CST）

完成 host-owned apps MCP 控制面里残留 OpenAI/Codex header/env 的清理：

- `codex-mcp/src/mcp/mod.rs` 中 connector bearer token 环境变量改名：
  - `CODEX_CONNECTORS_TOKEN` -> `ASTRAL_CONNECTORS_TOKEN`
- host-owned apps MCP 的 product SKU header 改名：
  - `X-OpenAI-Product-Sku` -> `X-Astral-Product-Sku`
- 确认 `host_owned_codex_apps_enabled` 当前仍固定返回 `false`，因此这不改变普通本地 MCP server
  行为，也不会重新启用 OpenAI/ChatGPT hosted apps 控制面。
- 复扫 `codex-mcp/src`、`core/src`、`app-server/src`、`config/src`，确认旧
  `CODEX_CONNECTORS_TOKEN` 和 `X-OpenAI-Product-Sku` 不再残留。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check -p codex-mcp`
  通过，用时 2m00s。
- 磁盘复查：约 37Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 64（2026-06-12 09:10 CST）

完成 `responses-api-proxy` 的 provider-neutral 降级：

- `responses-api-proxy` CLI 不再默认转发到
  `https://api.openai.com/v1/responses`。
- `--upstream-url` 现在是必填参数；proxy 只转发到用户显式指定的 upstream。
- CLI `about` 从 `Minimal OpenAI responses proxy` 改为
  `Minimal provider-neutral Responses proxy`。
- stdin key 缺失错误示例从 `OPENAI_API_KEY | codex ...` 改为
  `ASTRAL_API_KEY | astral ...`。
- crate README 改成 Astral/provider-neutral 示例：
  - 使用 `~/.astral-code/config.toml`
  - 使用 `astral -p proxy` / `astral exec`
  - 使用通用 provider upstream 示例，不再写死 OpenAI endpoint
- npm package 发布身份从 `@openai/codex-responses-api-proxy` 改为
  `@astral-code/responses-api-proxy`，仓库 URL 改为
  `github.com/oines/astral-code`。
- 复扫 `responses-api-proxy` crate，确认 OpenAI 默认 URL、`@openai` 包名、旧
  `OPENAI_API_KEY` 示例和 `openai/codex` 链接不再残留。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check -p codex-responses-api-proxy`
  通过，用时 1m01s。
- 磁盘复查：约 37Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 65（2026-06-12 09:14 CST）

完成一组 opt-in 诊断 env / 文档命名迁移：

- `exec-server` 环境配置测试示例从 `CODEX_LOG` 改为 `ASTRAL_LOG`。
- rollout trace 开关从
  `CODEX_ROLLOUT_TRACE_ROOT` / `CODEX_ROLLOUT_TRACE_ROOT_ENV`
  改为
  `ASTRAL_ROLLOUT_TRACE_ROOT` / `ASTRAL_ROLLOUT_TRACE_ROOT_ENV`。
- `rollout-trace` crate 顶层说明和 README 中的用户可见描述改为 Astral 语义：
  - tracing 是 Astral 本地诊断，不上传遥测
  - `state.json` 由 `astral debug trace-reduce` 生成
  - 热路径描述从 Codex session/rollout 改为 Astral session/rollout
- 保留内部 `CodexTurnId` 等 schema/模型标识，未在本 slice 硬拆，避免扩大 trace schema
  破坏面。
- 明确未触碰 `CODEX_ESCALATE_SOCKET`，它属于 shell escalation 内部协议，后续需要单独评估。
- 复扫 `exec-server` 与 `rollout-trace`，确认旧 `CODEX_LOG` 和
  `CODEX_ROLLOUT_TRACE_ROOT` 不再残留。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check -p codex-exec-server -p codex-rollout-trace`
  通过，用时 1m04s。
- 磁盘复查：约 37Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 66（2026-06-12 09:22 CST）

完成 bundled skills / app-server test-client 中旧 Codex 用户目录和 thread env 的清理：

- `skill-installer` 示例脚本不再读取 `CODEX_HOME` 或 `~/.codex`：
  - `install-skill-from-github.py` 改为 `$ASTRAL_HOME/skills`，默认
    `~/.astral-code/skills`
  - `list-skills.py` 改为 `$ASTRAL_HOME/skills`，默认
    `~/.astral-code/skills`
- skill installer GitHub request user-agent 从 `codex-skill-*` 改为
  `astral-skill-*`。
- skill installer 临时目录从 `/tmp/codex` 改为 `/tmp/astral-code`。
- `app-server-test-client/scripts/live_elicitation_hold.sh` 不再读取
  `CODEX_THREAD_ID`，改为 `ASTRAL_THREAD_ID`，仍保留通用 `THREAD_ID` fallback。
- 复扫 `skill-installer` 和 `app-server-test-client/scripts`，确认旧
  `CODEX_HOME`、`~/.codex`、`CODEX_THREAD_ID`、`codex-skill-*` 不再残留。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check -p codex-skills -p codex-app-server-test-client`
  通过，用时 2m25s。
- 磁盘复查：约 37Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 67（2026-06-12 09:30 CST）

完成 Windows / debug config 路径身份对齐：

- TUI debug config 测试路径从旧 Codex/OpenAI 位置改为 Astral 位置：
  - `/etc/codex/...` -> `/etc/astral-code/...`
  - `C:\repo\.codex` -> `C:\repo\.astral-code`
  - `C:\users\alice\.codex\config.toml` -> `C:\users\alice\.astral-code\config.toml`
  - `C:\ProgramData\OpenAI\Codex\requirements.toml` ->
    `C:\ProgramData\Astral-Code\requirements.toml`
- Windows sandbox 受保护 workspace 元目录从 `.codex` 对齐到 `.astral-code`：
  - `protect_workspace_codex_dir` -> `protect_workspace_astral_dir`
  - allow/spawn/setup/helper 测试中的受保护目录同步改为 `.astral-code`
  - command-runner cwd junction 目录同步改为 `.astral-code/.sandbox/cwd`
- Windows app runtime cache 路径从 `%LOCALAPPDATA%\OpenAI\Codex\bin` 改为
  `%LOCALAPPDATA%\Astral-Code\Astral\bin`，注释同步为 `astral.exe`。
- Windows setup-main 顶层错误日志 env 从 `CODEX_HOME` 改为 `ASTRAL_HOME`。
- 使用带引号 / 路径分隔符的固定字符串复扫，确认上述文件中不再保留旧 `.codex`
  路径字面量、`OpenAI\Codex` 或 `/etc/codex`。
- 未修改 `CODEX_SANDBOX_*`，未改 sandbox 策略算法，只改 Astral 项目路径身份。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check -p codex-windows-sandbox -p codex-tui`
  通过，用时 2m31s。
- 磁盘复查：约 37Gi 可用；`target/debug/incremental` 和 `target/tmp` 仍为 0B。

### 最新补充 68（2026-06-12 09:39 CST）

完成 app-server test-client 与 skill-installer 的 OpenAI/Codex 默认语义清理：

- `app-server-test-client/README.md` 不再指导构建或启动 `codex app-server`：
  - debug binary 示例改为 `cargo build -p codex-cli --bin astral`
  - 启动参数改为 `--astral-bin ./target/debug/astral`
  - app-server log 路径改为 `/tmp/astral-app-server-test-client/app-server.log`
- `app-server-test-client` 的少量用户可见运行时标识同步为 Astral：
  - toy app-server client name 从 `codex-toy-app-server` 改为 `astral-toy-app-server`
  - 当前可执行文件解析错误文案去掉 `codex-app-server-test-client` 专名
- `skill-installer` 默认 listing 不再访问 `openai/skills`：
  - `DEFAULT_REPO` 改为 `oines/astral-code`
  - `DEFAULT_PATH` 改为 `codex-rs/skills/src/assets/samples`
  - 文档改成 Astral-Code bundled sample skills，安装示例改为
    `--repo oines/astral-code --path codex-rs/skills/src/assets/samples/<skill-name>`
  - 安装后提示改为 “Restart Astral to pick up new skills.”
- `skill-installer/agents/openai.yaml` 的短描述去掉 `openai/skills` 默认源；文件名暂不改，
  因为 `agents/openai.yaml` 是当前 skills 元数据约定的一部分，牵到 skill-creator 和 plugin validator，
  后续需要单独规划整体迁移。
- 复扫本 slice 涉及目录，确认不再残留：
  `openai/skills`、`skills/.curated`、`skills/.experimental`、`Restart Codex`、
  `--codex-bin`、`target/debug/codex`、`codex app-server`、
  `/tmp/codex-app-server-test-client`、`codex-toy-app-server`。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check -p codex-skills -p codex-app-server-test-client`
  通过，用时 1m23s。
- 磁盘复查：约 37Gi 可用；`codex-rs/target` 约 113G，`target/debug/incremental` 为 0B。

### 最新补充 69（2026-06-12 09:52 CST）

完成 skills 元数据文件名从 `agents/openai.yaml` 到 `agents/astral.yaml` 的 Astral-native 迁移：

- `core-skills` loader 的 skill metadata 文件常量从 `openai.yaml` 改为 `astral.yaml`。
  Astral 不做旧 `openai.yaml` fallback，符合“新项目不兼容旧 Codex 数据”的原则。
- bundled sample skills 的 metadata 文件全部移动为 `agents/astral.yaml`：
  - `imagegen`
  - `openai-docs`
  - `plugin-creator`
  - `skill-creator`
  - `skill-installer`
- `skill-creator` 工具链同步改名：
  - `generate_openai_yaml.py` -> `generate_astral_yaml.py`
  - `write_openai_yaml(...)` -> `write_astral_yaml(...)`
  - reference 文档 `openai_yaml.md` -> `astral_yaml.md`
  - 新 skill 初始化输出 `agents/astral.yaml`
- `plugin-creator` validator 现在校验 `skills/<name>/agents/astral.yaml`。
- core / app-server 测试 fixture 中的 skill metadata 写入路径同步改为 `astral.yaml`。
- 全仓复扫确认除本进度文档历史记录外，不再有：
  `openai.yaml`、`agents/openai`、`generate_openai_yaml`、`write_openai_yaml`、
  `openai_yaml`。
- 这条补充取代第 68 条里“文件名暂不改”的临时判断；迁移已经完成。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check --tests -p codex-core-skills -p codex-skills -p codex-app-server -p codex-core`
  通过，用时 6m06s。
- 磁盘复查：约 37Gi 可用；`codex-rs/target` 约 113G，`target/debug/incremental` 为 0B。

### 最新补充 70（2026-06-12 09:59 CST）

完成一组模型可见 skills 文案的 Astral 化：

- `core-skills/src/render.rs` 中技能上下文预算 warning 不再写
  “Codex can still see every skill”，改为 “Astral can still see every skill”。
  这是可能进入模型上下文和 UI warning 的文案，属于高价值品牌残留清理。
- `skill-creator` bundled skill 的模型可见说明和新 skill 模板文案从 Codex 改为 Astral：
  - “extends Codex's capabilities” -> “extends Astral's capabilities”
  - “Codex is already very smart” -> “Astral is already very smart”
  - “Codex reads / references / produces” -> “Astral reads / references / produces”
  - 新 skill 初始化模板中的资源说明同步改为 Astral
- 复扫 `core-skills/src/render.rs` 与 `skill-creator` 相关文件，确认已清掉这些模型可见 Codex 品牌句式。
- 运行了 `just fmt`。
- 运行了窄测试：
  `just test -p codex-core-skills budgeted_rendering_token_budget_truncation_warning_mentions_two_percent`
  通过，1 个测试通过、102 个跳过，用时 1m44s。
- 运行了轻量脚本检查：
  - `python3 .../generate_astral_yaml.py --help`
  - `python3 .../init_skill.py --help`
  均可正常输出帮助，说明 `generate_astral_yaml` 重命名后的 import 路径未断。
- 测试后磁盘剩余约 35Gi，`codex-rs/target/debug/incremental` 增至约 1.6G；按用户要求只清理
  Astral-Code 项目内低影响增量缓存，删除该 incremental 目录后磁盘约 36Gi 可用。

### 最新补充 71（2026-06-12 10:07 CST）

完成 core-skills / system skills cache 中旧 Codex 路径身份的进一步收敛：

- embedded system skills cache marker 从
  `.codex-system-skills.marker` 改为 `.astral-system-skills.marker`。
  这是写入 `$ASTRAL_HOME/skills/.system` 的真实运行时文件名，不再保留旧 Codex marker。
- `core-skills` loader 中系统 config/skills 注释从 `/etc/codex/...` 改为
  `/etc/astral-code/...`。
- `core-skills` loader 中“System skills are written by Codex itself”的注释改为 Astral。
- `core-skills` 测试中的项目本地 config/skills 目录从 `.codex` 改为 `.astral-code`：
  - `REPO_ROOT_CONFIG_DIR_NAME`
  - disabled project layer fixture
  - manager 的 repo/user roots fixture
- `core-skills` 测试中的系统 config fixture 从 `etc/codex/config.toml` 改为
  `etc/astral-code/config.toml`。
- 复扫 `skills/src/lib.rs`、`core-skills/src/*.rs` 和 `core-skills/src/*_tests.rs`：
  旧 `.codex` project config/skills、`/etc/codex`、`.codex-system-skills.marker`、
  `CODEX_HOME`、`Codex itself` 已清掉。
- 剩余 `.codex-plugin/plugin.json` 只属于 plugin manifest 目录约定，本 slice 未动；这需要作为
  plugin manifest 兼容/重命名问题单独评估，避免破坏插件加载面。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check --tests -p codex-skills -p codex-core-skills`
  通过，用时 1m33s。
- 磁盘复查：约 36Gi 可用；`codex-rs/target` 约 114G，`target/debug/incremental` 为 0B。

### 最新补充 72（2026-06-12 10:20 CST）

推进 OpenAI/ChatGPT hosted control-plane 清理中的 remote plugin share 这一刀：

- 审计 `core-plugins/src/remote.rs`、`core-plugins/src/remote/share.rs`、
  `core-plugins/src/remote/share/checkout.rs` 和 app-server plugin request processor。
- 确认 `remote_plugin_control_plane_disabled()` 已经恒为 `true`，app-server 侧
  `remote_plugin_control_plane_enabled()` 也恒为 `false`；运行时不会触发 hosted remote
  plugin control-plane。
- 在此基础上进一步去掉 `remote/share.rs` 中旧的远程分享实装，而不是只靠 runtime guard：
  - 删除 workspace plugin upload URL 请求/响应结构。
  - 删除 `.tar.gz` 打包上传流程。
  - 删除 Azure blob PUT 上传路径。
  - 删除 `/public/plugins/workspace` create/update/delete 路径。
  - 删除 `/ps/plugins/workspace/created` list 路径。
  - 删除 `/ps/plugins/{id}/shares` targets update 路径。
- `save_remote_plugin_share`、`list_remote_plugin_shares`、`delete_remote_plugin_share`、
  `update_remote_plugin_share_targets` 现在直接返回
  `RemotePluginCatalogError::ControlPlaneDisabled`。
- 保留 `load_plugin_share_remote_ids_by_local_path` 只读能力，用于读取历史/本地 mapping 时保持
  app-server 类型链路稳定；不再写入或删除该 mapping。
- 简化 `remote/share/checkout.rs`：
  - 删除旧的 remote detail fetch + bundle download + personal marketplace 写入 checkout 实装。
  - `checkout_remote_plugin_share` 直接返回 `ControlPlaneDisabled`。
  - 保留返回类型，避免 app-server/API 类型面产生无关大改。
- 删除 `remote/share/tests.rs` 中所有旧 hosted 行为测试，因为这些测试的目标已经与 Astral 的
  “无 OpenAI hosted control-plane”目标相反。
- 清理 share/checkout 切除后暴露出来的死代码：
  - 删除 `plugin_bundle_archive.rs` 中仅用于分享上传的 bundle packer 和 size-limited writer。
  - 删除 `remote/share/local_paths.rs` 中仅用于 share save/delete 的 mapping 写入/删除函数。
  - 删除 `remote_bundle.rs` 中仅用于 share checkout 的
    `download_and_extract_remote_plugin_bundle_to_path` 和 checkout-to-path 解压函数。
- 复扫 `core-plugins/src/remote/share*`，旧 share 关键词
  `public/plugins/workspace`、`ps/plugins/.../shares`、`chatgpt-account-id`、
  `archive_plugin_for_upload`、`download_and_extract_remote_plugin_bundle_to_path`
  已不再存在。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check --tests -p codex-core-plugins`
  通过，用时约 8 秒；之前因为切除产生的 dead-code warning 已消失。
- 磁盘复查：约 36Gi 可用，`target/debug/incremental` 为 0B，不需要清理缓存。

### 最新补充 73（2026-06-12 10:33 CST）

继续收敛 OpenAI/ChatGPT hosted control-plane：切除后台 remote installed plugin bundle sync 的
旧托管同步实装。

- 审计 `core-plugins/src/remote/remote_installed_plugin_sync.rs`。
- 原状态：函数入口已经被 `remote_plugin_control_plane_disabled()` 拦住，但函数体仍保留：
  - hosted installed plugin list fetch。
  - workspace/global 两个 scope 的 remote bundle download URL 获取。
  - remote bundle 下载并安装到本地 cache。
  - stale remote plugin cache 清理。
- 新状态：
  - `sync_remote_installed_plugin_bundles_once` 直接返回
    `RemotePluginCatalogError::ControlPlaneDisabled`。
  - 删除该函数内的 hosted fetch/install/cache cleanup 实装。
  - 删除 `remove_stale_remote_plugin_caches` 和
    `is_remote_plugin_cache_mutation_in_flight` 私有函数。
  - 删除对应的 stale cache cleanup 测试，这些测试已经与 Astral 的“无 hosted remote control-plane”
    目标相反。
  - 保留 `RemotePluginCacheMutationGuard`、in-flight 去重和 mutation guard 结构；这些属于本地并发边界，
    不触达 OpenAI 控制面，可以继续为未来 provider-neutral/plugin runtime 使用。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check --tests -p codex-core-plugins`
  通过，用时约 8 秒。
- 重要剩余：`core-plugins/src/remote.rs` 主 catalog/detail/install/uninstall HTTP 实装仍在文件中，
  但入口已经被 disabled guard 拦住；下一轮建议把这块拆成更干净的 Astral stub，避免旧代码长期滞留。

### 最新补充 74（2026-06-12 10:52 CST）

完成 `core-plugins/src/remote.rs` 主 hosted catalog/detail/install/uninstall 实装切除。

- 原状态：
  - `remote.rs` 入口已经被 disabled guard 拦住。
  - 但文件内仍保留完整 OpenAI hosted remote plugin catalog client：
    - `/ps/plugins/list`
    - `/ps/plugins/workspace/shared`
    - `/ps/plugins/installed`
    - `/ps/plugins/{id}`
    - `/ps/plugins/{id}/skills/{skill}`
    - `/ps/plugins/{id}/install`
    - `/plugins/{id}/uninstall`
  - 还保留 `OAI-Product-Sku` header、`build_reqwest_client`、`authenticated_request`、
    `send_and_decode`、remote catalog disk cache、response DTO、remote response -> app-server
    model 转换逻辑。
- 新状态：
  - 删除 `remote/catalog_cache.rs`。
  - 删除 `remote.rs` 中所有 hosted HTTP helper、response DTO、catalog cache 读写、remote detail
    转换、install/uninstall mutation 实装。
  - `fetch_remote_marketplaces`、`fetch_and_cache_global_remote_plugin_catalog`、
    `fetch_astral_curated_remote_collection_marketplace`、`fetch_remote_installed_plugins`、
    `fetch_remote_plugin_detail`、`fetch_remote_plugin_share_context`、
    `fetch_remote_plugin_detail_with_download_urls`、`fetch_remote_plugin_skill_detail`、
    `install_remote_plugin`、`uninstall_remote_plugin` 现在全部直接返回
    `RemotePluginCatalogError::ControlPlaneDisabled`。
  - `has_cached_global_remote_plugin_catalog` 直接返回 `false`。
  - `cached_global_remote_discoverable_plugins` 直接返回空列表。
  - 保留公开类型、remote marketplace 常量、remote plugin id validation、以及
    `group_remote_installed_plugins_by_marketplaces`。这些仍被 app-server/manager 类型面使用，
    且不触达 hosted control-plane。
  - `remote.rs` 当前约 522 行；本轮相关 8 个文件净删约 2900 行。
- 复扫 `core-plugins/src/remote.rs` 和 `core-plugins/src/remote/**`：
  `ps/plugins`、`public/plugins`、`backend-api`、`authenticated_request`、
  `send_and_decode`、`build_reqwest_client`、`OAI-Product-Sku`、`chatgpt-account-id`、
  hosted response DTO 名称均已清掉。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check --tests -p codex-core-plugins`
  通过，且无 warning。

### 最新补充 75（2026-06-12 11:02 CST）

推进 hosted backend 默认路径切断：调整 `backend-client` 的 path style 推断。

- 审计 `backend-client/src/client.rs`。
- 原状态：
  - `PathStyle::from_base_url` 会在 base URL 包含 `/backend-api` 时自动选择 `HostedApi`。
  - `HostedApi` 会把账户、usage、profile、task、config bundle 等请求路由到 `/wham/*`。
  - 这意味着只要配置里出现 legacy hosted backend-api 风格 URL，通用 backend client 就会自动进入
    OpenAI hosted 后端路径。
- 新状态：
  - `PathStyle::from_base_url` 永远返回 `PathStyle::CodexApi`。
  - 新增测试 `base_url_does_not_infer_legacy_hosted_path_style`，确认
    `https://hosted.example/backend-api` 不再自动推断为 `HostedApi`。
  - 显式 `with_path_style(PathStyle::HostedApi)` 仍保留，避免一刀删掉所有 legacy 类型面；但默认/自动路径
    不再因 URL 字符串进入 `/wham/*`。
- 运行了 `just fmt`。
- 运行了轻量编译检查：
  `CARGO_INCREMENTAL=0 cargo check --tests -p codex-backend-client`
  通过。
- 磁盘复查：约 36Gi 可用，`target/debug/incremental` 为 0B。
- 当前总 diff：约 328 个文件，`+3037 / -8392`。

### 最新补充 76（2026-06-12 11:35 CST）

补强 Claude-ish 文件工具对 Codex/Astral 可替换执行后端的保护证据。

- 审计当前 `Read` / `Write` / `Edit` / `Glob` / `Grep` handler：
  - 非 `Read` 路径统一从 `turn_environment.environment.get_filesystem()` 取
    `ExecutorFileSystem`。
  - `Write` / `Edit` / `Glob` / `Grep` 都把 `&dyn ExecutorFileSystem` 继续传给内部函数。
  - `Write` 最终调用 `fs.write_file(...)`。
  - `Edit` 最终调用 `fs.read_file(...)` 和 `fs.write_file(...)`。
  - `Read` 文本路径走同一 filesystem 抽象；图片路径走 `ViewImageHandler`，带
    `environment_id`，继续由对应环境读取。
- 新增 `RecordingFileSystem` 测试后端，证明 Astral file tools 不直接触达本地磁盘：
  - `write_uses_executor_file_system`
  - `edit_uses_executor_file_system`
- 两个测试都断言：
  - tool output 正常。
  - 调用记录只包含 `read_file` / `write_file` trait 方法。
  - temp dir 里没有生成真实本地文件。
- 结论：
  - 当前 Claude-ish 文件工具只是换了模型侧 schema/名字/结果手感。
  - runtime 仍然继承 Codex 的 `Environment -> ExecutorFileSystem` 边界。
  - 未来 SSH/container/VM/K8s/exec-server 文件系统后端仍可替换；不能为了工具 flavor 直接绕回
    `std::fs`。
- 运行了 `just fmt`。
- 验证：
  - 第一次误用文件名 filter 跑了
    `just test -p codex-core astral_file_tools_tests`，编译完成但 0 tests matched，nextest
    以 exit code 4 返回。
  - 随后用函数名公共片段跑：
    `just test -p codex-core executor_file_system`
    通过，2 个测试通过。
- 磁盘复查：
  - core 测试冷启动后可用空间从约 36Gi 降到 32Gi。
  - 仅清理 Astral-Code 内部低价值缓存 `codex-rs/target/debug/incremental`。
  - 清理后可用空间约 35Gi，`codex-rs/target` 约 115Gi。

### 最新补充 77（2026-06-12 12:05 CST）

继续切断 cloud tasks 中手写的 OpenAI hosted `/backend-api -> /wham/*` 路径。

- 审计 `cloud-tasks` / `cloud-tasks-client`：
  - `cloud-tasks/src/env_detect.rs` 仍会在 `base_url.contains("/backend-api")` 时手写访问
    `/wham/environments` 和 `/wham/environments/by-repo/...`。
  - `cloud-tasks/src/lib.rs` 启动日志仍把这种路径称为 `path_style=wham`。
  - debug mock 默认 base URL 仍是 `http://localhost/backend-api`。
  - `cloud-tasks-client/src/http.rs` 的错误提示 URL 仍会把 `/backend-api` 映射到 `/wham/tasks/...`。
  - `cloud-tasks/src/util.rs::task_url` 仍把 `/backend-api` 特殊剥离为旧 hosted UI root。
- 新状态：
  - 新增 `util::codex_api_url(base_url, path)`：
    - base 已经以 `/api/codex` 结尾时直接拼子路径。
    - 其他 base 一律拼为 `{base}/api/codex/{path}`。
    - 即使 base 是 `.../backend-api`，也不会再进入 `/wham/*`。
  - `env_detect` 的 repo environment lookup 和 global environment list 都统一用
    `util::codex_api_url(...)`。
  - `init_backend` debug mock 默认 base 改为 `http://localhost`。
  - startup log 固定为 `path_style=codex-api`，不再根据 URL 内容推断 hosted style。
  - `cloud-tasks-client` 的 details error URL 不再生成 `/wham/tasks/...`。
  - `task_url` 不再把 `/backend-api` 特殊剥离为 hosted UI root；它现在把该 URL 当普通 root。
- 复扫：
  - `cloud-tasks/src/env_detect.rs`、`cloud-tasks/src/util.rs`、
    `cloud-tasks-client/src/http.rs` 中已无 `/wham` 残留。
  - 当前只剩测试用的 `/backend-api` 字面量，用于证明不再特殊改写。
- 运行了 `just fmt`。
- 验证：
  - `just test -p codex-cloud-tasks codex_api_url`
    通过，1 个测试通过。
  - `just test -p codex-cloud-tasks format_task_list_lines_formats_urls`
    通过，1 个测试通过。
  - `codex-cloud-tasks-client` 在上述测试编译链中成功编译，覆盖了 `details_path` 类型改动。
- 磁盘复查：
  - 可用空间约 31Gi。
  - `target/debug/incremental` 约 4.0Gi。本轮暂不清理，保留热编译缓存；若继续跌到 25Gi
    左右再清理更划算。

### 最新补充 78（2026-06-12 12:34 CST）

删除 `codex-cloud-config` 中 test-only 的旧 ChatGPT-hosted remote bundle 实现。

- 审计结果：
  - 生产入口 `cloud_config_bundle_loader(...)` / `cloud_config_bundle_loader_for_storage(...)`
    已经只返回 `CloudConfigBundleLoader::default()`，不再发远程请求。
  - 但 crate 内仍保留 `#[cfg(test)]` 的旧实现：
    `backend.rs`、`cache.rs`、`metrics.rs`、`service.rs`、`validation.rs` 和两组大测试。
  - 这些文件包含旧 hosted bundle retry、auth recovery、signed cache、ChatGPT identity cache key 等
    OpenAI 控制面逻辑，虽然只在测试编译中出现，但已经不再符合 Astral 的目标形态。
- 新状态：
  - `cloud-config/src/lib.rs` 只保留 `bundle_loader`。
  - 删除旧 test-only remote bundle service/cache/backend/validation/metrics 以及对应测试文件：
    - `backend.rs`
    - `cache.rs`
    - `cache_tests.rs`
    - `metrics.rs`
    - `service.rs`
    - `service_tests.rs`
    - `validation.rs`
  - `bundle_loader` 参数从 `_hosted_base_url` 收敛为 `_base_url`。
  - crate 文档改为 provider-neutral disabled hook，不再描述 ChatGPT-hosted control plane。
  - `cloud-config/Cargo.toml` 依赖从旧 backend/cache/service 所需的一大组依赖收窄到：
    - `codex-config`
    - `codex-login`
  - `Cargo.lock` 中 `codex-cloud-config` 条目同步移除旧依赖。
- 复扫：
  - `codex-rs/cloud-config/src` 中已无 `chatgpt`、`ChatGPT`、`hosted`、`backend`、
    `BundleClient`、`CloudConfigBundleService`、`CloudConfigBundleCache` 等残留命中。
- 运行了：
  - `just fmt`
  - `just bazel-lock-update`
  - `PATH=/opt/homebrew/bin:$PATH just bazel-lock-check`
  - `just test -p codex-cloud-config --no-tests pass`
- 备注：
  - 第一次直接跑 `just bazel-lock-check` 会用 macOS `/usr/bin/python3` 3.9.6，无法解析脚本里的
    `str | None` 类型语法并失败；用 Homebrew Python 3.14 放到 PATH 后检查通过。
  - `MODULE.bazel.lock` 无 diff。
  - 当前可用空间约 28Gi，`target/debug/incremental` 约 5.3Gi。暂不清，避免下一轮继续冷编。

### 最新补充 79（2026-06-12 12:43 CST）

模型 catalog 的 legacy ChatGPT source-of-truth 分支已删除。

- 旧状态：
  - `OpenAiModelsManager::apply_remote_models(...)` 在 remote `/models` 返回至少一个可见模型，且当前
    auth mode 是 legacy ChatGPT/hosted account 时，会把 remote catalog 整体当成唯一真相。
  - 这会让旧 Codex hosted catalog 语义凌驾于 Astral 的 bundled provider-neutral catalog 之上，也会让
    `get_model_info(...)` 对 bundled 模型错误地走 fallback metadata。
- 新状态：
  - provider `/models` 输出始终作为 bundled catalog 的 overlay：同 slug 覆盖，不同 slug 追加。
  - 每次 refresh 都从 bundled catalog 重新合并 remote models，因此 remote 里删除的 provider-only model
    仍会被正确移除；但 bundled metadata 不会因为 legacy hosted auth 形状被整体替换掉。
  - `ModelsEndpointClient` 的刷新能力抽象、provider auth、command auth、cache/ETag 行为保留不动。
  - 这次改动不触碰 exec-server、sandbox、PTY、UnifiedExec 或文件执行后端抽象。
- 验证：
  - `just fmt` 通过。
  - `just test -p codex-models-manager refresh_available_models_merges_visible_provider_catalog_with_bundled_catalog refresh_available_models_merges_cached_provider_catalog_with_bundled_catalog get_model_info_keeps_bundled_metadata_when_provider_catalog_refreshes refresh_available_models_preserves_bundled_catalog_for_empty_provider_remote refresh_available_models_merges_hidden_only_provider_remote_with_bundled_catalog`
    通过，5 个测试通过。
- 磁盘：
  - 当前可用空间约 29Gi，`codex-rs/target/debug/incremental` 约 5.3Gi。暂未清理，保留热编译缓存；
    如果继续下降到告警线，再只清理 Astral-Code 项目内构建缓存。

### 最新补充 80（2026-06-12 12:55 CST）

external API-key auth 现在严格覆盖 cached hosted auth，不再失败后回退到旧 ChatGPT/hosted token。

- 旧状态：
  - `AuthManager::auth()` 会先尝试 `resolve_external_api_key_auth()`。
  - 如果 external API-key provider 返回 `None` 或报错，会继续返回 `auth_cached()`。
  - 这意味着在测试或异常状态下，外部 API key provider 失效后，模型 catalog 等路径仍可能用 cached
    legacy ChatGPT/hosted auth 继续请求。
- 新状态：
  - 一旦配置了 external API-key auth，`AuthManager::auth()` 就只返回 external provider 的解析结果。
  - external provider 失败时返回 unauthenticated，不再 fallback 到 cached hosted credentials。
  - `auth_mode()` / `get_api_auth_mode()` 仍报告 `ApiKey`，因此上层知道当前配置意图是 provider-neutral
    API key，而不是 legacy hosted account。
  - `models-manager` 的 unresolved external API-key 测试同步改成“不抓 remote models”。
- 验证：
  - `just fmt` 通过。
  - `just test -p codex-login external_api_key_auth_failure_does_not_fall_back_to_cached_auth`
    通过，1 个测试通过。
  - `just test -p codex-models-manager refresh_available_models_skips_network_when_external_api_key_is_unresolved refresh_available_models_skips_network_when_external_api_key_overrides_cached_hosted_auth`
    通过，2 个测试通过。
- 磁盘：
  - 本轮测试触发较重共享 crate 编译，跑完后可用空间降到约 25Gi，`debug/incremental` 涨到约 8.1Gi。
  - 已按用户要求只清理 Astral-Code 项目内 `codex-rs/target/debug/incremental`。
  - 清理后可用空间约 31Gi；未删除项目外文件，也未删除 `debug/deps`。

### 最新补充 81（2026-06-12 13:05 CST）

模型 `/models` refresh 不再由 legacy hosted/ChatGPT auth 触发。

- 旧状态：
  - `ModelsEndpointClient` 暴露 `uses_hosted_backend()`。
  - `OpenAiModelsManager::should_refresh_models()` 会因为 hosted backend auth 为 true 而刷新远程模型目录。
  - 这让旧 ChatGPT/hosted auth 形状仍然参与 Astral 的模型目录控制面，即使实际目标是 provider-neutral
    `/models`。
- 新状态：
  - 删除 `ModelsEndpointClient::uses_hosted_backend()`。
  - `/models` refresh 只由 provider-neutral 能力触发：
    - provider command auth；
    - provider env / bearer token auth。
  - 测试 endpoint 默认改为 `has_provider_auth = true` 来表达“这个 provider 自己具备远程模型刷新能力”，
    而不是靠 hosted account。
  - `model-provider` 的 `OpenAiModelsEndpoint` 不再需要提供 hosted refresh 判断。
- 保留边界：
  - provider `/models`、cache、ETag 行为保留。
  - 注意：本段当时仍保留 bundled catalog overlay；该口径已在最新补充 44 中撤销，运行时不得恢复
    bundled catalog overlay。
  - 本轮没有触碰 exec-server、sandbox、PTY、UnifiedExec、approval 或文件执行后端。
  - `AuthManager::current_auth_uses_hosted_backend()` 目前仍只影响 picker 里 hosted-only model visibility，
    后续可以单独收敛。
- 验证：
  - `just fmt` 通过。
  - `just test -p codex-models-manager refresh_available_models_merges_visible_provider_catalog_with_bundled_catalog refresh_available_models_skips_network_without_provider_refresh_auth refresh_available_models_skips_network_when_external_api_key_overrides_cached_hosted_auth refresh_available_models_skips_network_when_external_api_key_is_unresolved refresh_available_models_fetches_with_provider_auth`
    通过，5 个测试通过。
  - `just test -p codex-model-provider models_endpoint`
    通过，2 个测试通过。
- 磁盘：
  - 当前可用空间约 30Gi，`codex-rs/target/debug/incremental` 约 1.5Gi。暂不清理。

### 最新补充 82（2026-06-12 13:18 CST）

模型 picker 不再因为 cached legacy hosted auth 展示 hosted-only / non-API 模型。

- 旧状态：
  - `ModelPreset::filter_by_auth(models, chatgpt_mode)` 暴露 `chatgpt_mode` 参数。
  - `ModelsManager::build_available_models(...)` 会根据
    `AuthManager::current_auth_uses_hosted_backend()` 决定是否展示 `supported_in_api = false` 的模型。
  - 这意味着只要缓存里有 legacy ChatGPT/hosted auth，Astral 的模型 picker 就可能显示 API/provider
    模式不可用的 hosted-only 模型。
- 新状态：
  - `ModelPreset::filter_by_auth(...)` 改为 `ModelPreset::filter_api_supported(...)`。
  - 过滤器无 auth 参数，固定移除 `supported_in_api = false` 的模型。
  - `ModelsManager::build_available_models(...)` 总是使用 provider/API 可用模型集。
  - app-server v2 `model/list` 测试 helper 同步改为 `filter_api_supported(...)`，不再携带
    `chatgpt_mode = false` 这种旧形状。
- 保留边界：
  - 模型 catalog overlay、provider `/models` refresh、cache/ETag 行为不变。
  - 本轮没有触碰 exec-server、sandbox、PTY、UnifiedExec、approval 或文件执行后端。
  - `AuthManager::current_auth_uses_hosted_backend()` 目前只剩 image-generation hosted-only gate 使用，
    后续如继续清 OpenAI hosted extension，可单独处理。
- 验证：
  - `just fmt` 通过。
  - `just test -p codex-protocol model_preset_filter_api_supported_removes_hosted_only_models`
    通过，1 个测试通过。
  - `just test -p codex-models-manager static_manager_hides_models_not_supported_in_api_even_with_cached_hosted_auth`
    通过，1 个测试通过。
  - app-server v2 `model/list` 只改了测试 helper 调用，未跑 app-server 测试，避免触发更重编译；后续阶段性
    集中测试时需要覆盖。
- 磁盘：
  - 当前可用空间约 29Gi，`codex-rs/target/debug/incremental` 约 3.1Gi。暂不清理。

### 最新补充 83（2026-06-12 13:39 CST）

完成一轮 hosted extension / 执行后端边界确认。

- image-generation 默认安装入口已从 app-server extension registry 移除：
  - `codex-rs/app-server/src/extensions.rs` 不再默认安装 legacy OpenAI-hosted image generation extension。
  - `codex-rs/app-server/Cargo.toml` / `codex-rs/core/Cargo.toml` 移除对
    `codex-image-generation-extension` 的 app-server/core dev 依赖。
  - `codex-rs/ext/image-generation/src/extension.rs` 保留 crate 壳，但默认 `available = false` 且不暴露工具。
- 执行后端边界确认：
  - 本轮没有重写或改变 exec-server、app-server process exec、Environment、ExecBackend、UnifiedExec、
    PTY、sandbox 或 approval 的执行语义。
  - 当前工作树里 `unified_exec` / `exec_env` / `shell_environment` 有少量 Astral 命名级 diff：
    `CODEX_THREAD_ID` -> `ASTRAL_THREAD_ID`、`CODEX_CI` -> `ASTRAL_CI`，以及测试专用类型/方法的
    `#[cfg(test)]` 收敛；这些不是执行后端抽象重构。
  - Astral/Claude-ish 的 `Bash` 语义仍应映射到 Codex 原本统一执行链路，不绕过可替换执行后端。
  - app-server 全量测试误触发后，`process_exec::*`、`thread_shell_command::*`、approval replay、
    sandbox/thread setting 相关用例均通过，侧面说明执行后端抽象没有被本轮改动打坏。
  - 这也符合后续远程设备、容器、SSH 工作区等 harness 目标：工具 flavor 可以换，但落地必须继续走
    Codex 的执行后端抽象。
- 验证：
  - `just fmt` 通过。
  - `just bazel-lock-update` 通过。
  - `PATH=/opt/homebrew/bin:$PATH just bazel-lock-check` 通过。
  - `just test -p codex-image-generation-extension` 通过，9 个测试通过；由于 extension 默认禁用，
    backend/tool 类型出现 dead code warning，暂记为后续瘦身项。
  - 误触发 `just test -p codex-app-server --no-tests pass` 后，实际跑了 app-server suite：
    773 个测试运行，702 passed、70 failed、1 timed out、13 skipped。失败集中在：
    OpenAI/remote plugin marketplace、hosted app list、Bedrock 旧 `gpt-5.5` catalog 假设、
    image/web search capability 旧预期、local compact metadata 旧预期、OpenAI model reroute reason
    旧预期、Astral 默认 prompt 文案差异、MCP/resource 个别旧测试漂移。
    这些失败暂按“测试预期尚未 Astral 化”处理，不作为执行后端退化证据。
- 磁盘：
  - 因全量 app-server suite 产生构建压力，可用空间降到约 21Gi。
  - 已仅删除项目内低风险缓存 `codex-rs/target/debug/incremental`，释放约 9.8Gi；当前可用空间约 29Gi。
  - 代价是后续 Rust 增量编译会慢一些，但不影响源码和测试结果本身。

### 最新补充 84（2026-06-12 14:02 CST）

完成 provider capability / Bedrock catalog 的 Astral 化小切片。

- Bedrock 静态模型目录不再强依赖 bundled catalog 里必须存在 `gpt-5.5`：
  - `codex-rs/model-provider/src/amazon_bedrock/catalog.rs` 将旧的
    `bundled_openai_model(...)` 改为 `bundled_reference_model(...)`。
  - 优先复用目标 slug 的 bundled metadata；缺失时 fallback 到 `gpt-5.4`，再 fallback 到 catalog
    第一个模型。
  - 这样 Astral bundled catalog 可以以 DeepSeek/provider-neutral 模型为默认，不需要为了 Bedrock 测试把
    OpenAI `gpt-5.5` 加回模型目录。
  - Bedrock 对外仍暴露 Mantle model id：`openai.gpt-5.5` / `openai.gpt-5.4`，但显示名和描述改成
    Bedrock-specific，避免把 fallback metadata 的 display name 泄露给用户。
- 默认 provider capability 测试同步到 Astral 语义：
  - app-server v2 `modelProvider/capabilities/read` 默认预期现在是
    `namespace_tools = true`、`image_generation = false`、`web_search = false`。
  - 这和已禁用 legacy OpenAI-hosted image-generation extension 的方向一致，也避免 UI/客户端误以为默认
    provider 提供 hosted image/web search。
- 保留边界：
  - 本轮没有触碰 exec-server、sandbox、PTY、UnifiedExec、approval 或文件/命令执行后端。
  - 改动只在 model-provider catalog 和 app-server capability 测试预期。
- 验证：
  - `just fmt` 通过。
  - `just test -p codex-model-provider amazon_bedrock` 通过：17 个测试通过，16 个 skipped。
  - `just test -p codex-app-server model_provider_capabilities_read` 通过：2 个测试通过，784 个 skipped。
- 磁盘：
  - 测试后可用空间约 25Gi，`codex-rs/target/debug/incremental` 约 3.3Gi。
  - 暂不清理，保留热编译缓存；若继续下降到危险区再只清 Astral-Code 项目内低价值缓存。

### 最新补充 85（2026-06-12 14:22 CST）

完成 remote marketplace 外露 id 的 Astral 化切片。

- 旧状态：
  - remote plugin 控制面已被禁用，但全仓仍有大量 `openai-curated-remote` 作为远程 marketplace id、
    缓存路径、测试 fixture 和模型可见 plugin id 后缀。
  - 这会让 Astral 在 remote plugin disabled stub 下仍暴露 OpenAI 命名，和“新项目、不兼容旧 Codex 数据”
    的方向冲突。
- 新状态：
  - `codex-rs/core-plugins/src/remote.rs` 的 `REMOTE_GLOBAL_MARKETPLACE_NAME` 改为
    `astral-curated-remote`。
  - app-server、core-plugins、core plugin discoverable / request install 测试 fixture 中的
    `openai-curated-remote` 已机械替换为 `astral-curated-remote`。
  - app-server remote plugin warning 文案也同步改成 `astral-curated-remote collection fetch failed...`。
  - `rg "openai-curated-remote|OpenAI Curated Remote|openai-curated remote" codex-rs -g '*.rs' -g '*.toml'`
    已无命中。
- 保留边界：
  - 本轮不改变 remote plugin 控制面仍 disabled 的事实。
  - 本轮不触碰 exec-server、sandbox、PTY、UnifiedExec、approval 或文件/命令执行后端。
  - 本轮只收口 remote marketplace 身份和相关测试 fixture，不重新接回任何 hosted 网络路径。
- 验证：
  - `just fmt` 通过。
  - `just test -p codex-core-plugins remote_bundle` 通过：15 个测试通过，171 个 skipped。
  - `just test -p codex-core-plugins discoverable` 通过：6 个测试通过，180 个 skipped。
  - 未跑 app-server/plugin 全套；那块仍有大量“旧 OpenAI hosted 行为预期”需要后续统一改成
    Astral disabled/unsupported 语义。
- 磁盘：
  - 测试后可用空间约 24Gi，`codex-rs/target/debug/incremental` 约 5.0Gi。
  - 暂时保留热缓存；若继续下降，再只清 Astral-Code 项目内 `target/debug/incremental`。

### 最新补充 86（2026-06-12 14:50 CST）

收尾 app-server remote plugin disabled guard 的校验顺序。

- 旧状态：
  - remote plugin control-plane 已经禁用，但部分 app-server 入口在 disabled guard 前后的错误优先级仍不够干净。
  - 对 Astral 来说，远程 hosted 控制面应该被挡住，但纯本地的参数校验仍应保留，否则非法 plugin id 会被
    “remote plugin not enabled” 掩盖，调试和客户端行为都不够明确。
- 新状态：
  - `plugin/read` 的 remote 分支在返回 disabled 之前先校验 `plugin_name`。
  - `plugin/skill/read` 在返回 disabled 之前先校验 `remote_plugin_id` 和空 `skill_name`。
  - `plugin/install` 的 remote 分支在返回 disabled 之前先校验 `remote_plugin_id`。
  - 真正会加载 config、读取 auth、请求 remote catalog 或下载 bundle 的路径仍在 disabled guard 之后，
    没有重新打开任何 hosted 网络控制面。
- 保留边界：
  - 不改变 local marketplace plugin read/install 行为。
  - 不触碰 exec-server、sandbox、PTY、UnifiedExec、approval 或文件/命令执行后端。
  - 不恢复旧 OpenAI/ChatGPT remote plugin control-plane。
- 验证：
  - `just fmt` 通过。
  - `just test -p codex-app-server plugin_install_rejects_invalid_remote_plugin_name` 通过：
    1 个测试通过，785 个 skipped。
  - `just test -p codex-app-server plugin_read_rejects_invalid_remote_plugin_name` 通过：
    1 个测试通过，785 个 skipped。
  - `git diff --check` 通过。
- 磁盘：
  - 测试后可用空间约 23Gi，`codex-rs/target/debug/incremental` 约 6.3Gi。
  - 目前继续保留热缓存；若可用空间继续明显下降，再只清 Astral-Code 项目内构建缓存。

### 最新补充 87（2026-06-12 15:03 CST）

收敛 app-server API 文档里的 OpenAI/ChatGPT 专有集成指引。

- 旧状态：
  - `codex-rs/app-server/README.md` 仍把 app-server 描述成 Codex / OpenAI VS Code extension 的接口。
  - 初始化文档还要求 `clientInfo.name` 对接 OpenAI Compliance Logs Platform，并链接
    `chatgpt.com/admin/api-reference#tag/Logs:-Codex`。
  - `plugin/uninstall` 仍描述“转发到 ChatGPT plugin backend”。
  - schema generation 示例仍使用 `codex app-server`，部分示例 `modelProvider` 仍写 `openai`。
- 新状态：
  - app-server 文档入口改成 `astral app-server` / Astral-Code。
  - `clientInfo.name` 改为 provider-neutral 的稳定客户端标识说明。
  - remote plugin uninstall 文档改为当前 Astral 事实：legacy hosted remote-plugin uninstall disabled，
    未来只有显式 provider-neutral marketplace 才可支持。
  - 示例命令、`ASTRAL_HOME`、`.astral-code` skill/config 路径和示例 `modelProvider: "astral"` 已同步。
  - attestation 文档从 `x-oai-attestation` / ChatGPT Codex 改为 provider-neutral attestation metadata。
- 保留边界：
  - 这是 docs-only 切片，不改 app-server wire schema，不重命名 `codexHome` 等现存 API 字段。
  - 不触碰 exec-server、sandbox、PTY、UnifiedExec、approval 或文件/命令执行后端。
- 验证：
  - `rg "codex app-server|OpenAI|ChatGPT|openai.chatgpt|x-oai-attestation|chatgpt.com/admin|modelProvider\\\": \\\"openai\\\"|/Users/me/openai|api.openai.com" codex-rs/app-server/README.md`
    只剩一条有意保留的 “Legacy ChatGPT OAuth ... not accepted by Astral”。
  - `git diff --check` 通过。

### 最新补充 88（2026-06-12 本轮）

修正 Claude-ish 文件工具 schema 的执行环境表述。

- 旧状态：
  - `Read` 的模型可见描述写着 “local image from the filesystem”。
  - `Write` 的模型可见描述写着 “local filesystem”。
  - 这和用户明确要求的 harness 目标不一致：Astral agent 在本机运行，但最终作用对象应由
    Codex/Astral 的 Environment / ExecBackend / exec-server 抽象决定，可以是本机、远程设备、容器或未来别的后端。
- 新状态：
  - `Read` 改为 “active execution environment”。
  - `Write` 改为 “active execution environment”。
  - `Edit` 也明确是 “active execution environment”。
  - schema 里已有的 `environment_id` 参数保留，继续表达多执行环境目标选择。
- 保留边界：
  - 只改模型可见 tool description 和 schema 测试，不改文件工具 runtime。
  - 没有绕过 Codex 原有的 `ExecutorFileSystem`、Environment、ExecBackend、sandbox 或 approval 边界。
  - `Bash` / 后台任务工具组仍走 Codex/Astral 的 PTY / UnifiedExec 长任务观察能力。
- 验证：
  - `just fmt` 通过。
  - `just test -p codex-tools astral` 通过：7 个测试通过，90 个 skipped。
  - `git diff --check` 通过。
- 磁盘：
  - 测试后可用空间降到约 21Gi，`codex-rs/target/debug/incremental` 约 7.4Gi。
  - 已按用户要求只删除 Astral-Code 项目内
    `/Users/oines/project/astral-code/codex-rs/target/debug/incremental`。
  - 清理后可用空间约 27Gi；未删除 `debug/deps`，未删除项目外文件。

### 最新补充 89（2026-06-12 本轮）

硬化 Anthropic Messages stream 对 Claude extended thinking `signature_delta` 的兼容。

- 旧状态：
  - Anthropic stream parser 支持 `text_delta`、`input_json_delta`、`thinking_delta`。
  - 真实 Claude / Anthropic extended thinking stream 还可能出现 `signature_delta`。
  - Astral 当前内部 `ContentDelta` 没有 signature delta 表示；旧行为会把它当 unknown delta 报错，导致
    `/anthropic` stream 在 reasoning 签名事件上提前失败。
- 新状态：
  - `signature_delta` 现在被安全忽略，返回 `None`，不产生模型输出、不计入文本/工具增量。
  - 这和 Claude Code 的处理方向一致：signature 是 cryptographic/authentication metadata，不是普通模型输出。
  - 没有扩展 Agent IR，也没有把 signature 拼进 reasoning text，避免污染 compact/history/token 轨迹。
- 保留边界：
  - `ContentBlock::Reasoning { signature }` 的非流式/完整 block 结构仍保留。
  - 只改 Anthropic adapter 的 stream parser，不改 chat-completions adapter，不改 core session/runtime。
- 验证：
  - 对照 `/Users/oines/project/claude-code/services/api/claude.ts` 和
    `/Users/oines/project/claude-code/utils/messages.ts` 确认 Claude Code 对 `signature_delta` 单独处理且不计普通输出。
  - `just fmt` 通过。
  - `just test -p codex-api anthropic` 通过：6 个测试通过，135 个 skipped。
  - `git diff --check` 通过。
- 磁盘：
  - 测试后可用空间约 27Gi，`codex-rs/target/debug/incremental` 约 515M。
  - 暂不继续清理。

### 最新补充 90（2026-06-12 本轮）

硬化 OpenAI-compatible chat-completions stream 的国内网关兼容性。

- 旧状态：
  - stream parser 要求每个 chunk 都有 `choices[].delta`。
  - 有些兼容网关的最终 chunk 可能只有 `finish_reason`，没有 `delta`。
  - 有些兼容网关会直接 SSE 输出 `{ "error": { "message": "..." } }`，而不是 `choices`。
  - 旧行为会把这些情况报成 parser 结构错误，丢掉 provider 的真实错误或正常终止原因。
- 新状态：
  - `{"error": {"message": "..."}}` 会映射为 `StopReason::Error`，再由 agent SSE mapper 转成终止错误。
  - 缺少 `delta` 但包含 `finish_reason` 或 `usage` 的 chunk 现在可以正常处理。
  - 原有 DeepSeek `reasoning_content`、cache usage、tool_calls 增量和 usage-only chunk 逻辑不变。
- 保留边界：
  - 只改 chat-completions provider adapter，不改 core session、tool runtime、sandbox 或 exec。
  - 仍然保留对真正 malformed chunk 的 `MissingField("delta")` 校验：没有 `delta`、也没有
    `finish_reason` / `usage` 的 choices 不会被静默吞掉。
- 验证：
  - `just fmt` 通过。
  - `just test -p codex-api chat_completions` 通过：12 个测试通过，131 个 skipped。
  - `git diff --check` 通过。
- 磁盘：
  - 测试后可用空间约 26Gi，`codex-rs/target/debug/incremental` 约 678M。
  - 暂不清理。

### 最新补充 91（2026-06-12 本轮）

清理 app-server-protocol 里的 OpenAI 示例值和生成 schema 注释。

- 旧状态：
  - `Thread.modelProvider` 的协议注释仍写着 `for example, 'openai'`。
  - app-server protocol 序列化测试中仍使用 `model_provider: "openai"`、`api.openai.com`、
    `github.com/openai/example.git`、`openai-curated-remote` 等示例值。
  - 这些不是 runtime 行为，但会继续把 OpenAI 作为 Astral 协议层的默认 mental model 暴露在 fixture
    和生成类型里。
- 新状态：
  - `Thread.modelProvider` 注释示例改成 `astral`。
  - 相关 protocol 测试示例改成 provider-neutral 或 Astral 命名：
    `api.provider.example`、`github.com/astral-code/example.git`、`astral-curated-remote`。
  - 重新生成 app-server schema fixtures，使 JSON/TypeScript 生成件与源码注释一致。
- 保留边界：
  - 只改测试值、注释和生成 schema fixture，不改变 app-server v2 wire shape。
  - 不改 exec-server、sandbox、approval、Plan Mode、Goal Mode 或 compact。
  - 不新增旧 Codex 数据兼容路径。
- 验证：
  - `just fmt` 通过。
  - `just write-app-server-schema` 已执行并更新 fixture。
  - `just test -p codex-app-server-protocol` 通过：222 个测试通过，0 个 skipped。
  - `git diff --check` 通过。
- 磁盘：
  - 测试后可用空间约 24Gi，`codex-rs/target/debug/incremental` 约 2.7G。
  - 下一步清理这块低价值增量缓存，避免继续挤压开发空间。

### 最新补充 92（2026-06-12 本轮）

清理 realtime 默认 backend prompt 的 OpenAI 身份句。

- 旧状态：
  - `codex-rs/prompts/templates/realtime/backend_prompt.md` 默认身份写的是
    `You are Codex, an OpenAI general-purpose agentic assistant...`。
  - 这不是普通注释或测试名，而是 realtime 会话会加载的模型可见默认 prompt，会直接影响模型上下文和
    Astral 的品牌/轨迹手感。
- 新状态：
  - 默认身份改为 `You are Astral, a provider-neutral agentic assistant...`。
  - 保留原有 realtime “统一 assistant / backend 执行 / 用户可 steer”的工作模型，不改交互协议。
  - 对应 `realtime_prompt` 测试断言同步改为 Astral/provider-neutral 身份。
- 保留边界：
  - 不改 `prepare_realtime_backend_prompt` 的 override 规则。
  - 不改 realtime transport、WebRTC/WebSocket、auth、sandbox、exec-server。
  - 这次只处理模型可见身份文案，不碰 Claude-ish tool schema。
- 验证：
  - `just fmt` 通过。
  - `just test -p codex-core realtime_prompt` 通过：4 个测试通过，2634 个 skipped。
  - `git diff --check` 通过。
- 磁盘：
  - 因为刚清过 incremental 后触发 `codex-core` 重编译，本轮测试耗时较长。
  - 测试后可用空间约 21Gi，`codex-rs/target/debug/incremental` 约 3.5G。
  - 下一步清理 Astral-Code 自己的 incremental 缓存，避免磁盘继续告警。

### 最新补充 93（2026-06-12 本轮）

清理主模型指令模板的 Codex/OpenAI 身份残留。

- 旧状态：
  - `codex-rs/core/templates/model_instructions/gpt-5.2-codex_instructions_template.md` 第一句仍是
    `You are Codex, a coding agent based on GPT-5...`。
  - 这是主 agent model instructions 模板，不是普通文档；如果后续从模板再生成或派发指令，会把 Codex
    身份重新带回模型上下文。
- 新状态：
  - 第一句改成 `You are Astral, a coding agent running in astral-code...`。
  - `codex-rs/prompts`、`codex-rs/core/templates`、`codex-rs/core/*.md`、
    `codex-rs/protocol/src/prompts` 范围内已经扫不到 `You are Codex` 或
    `OpenAI general-purpose` 身份句。
- 保留边界：
  - 只改模型可见身份句，不重写整份开发者指令。
  - 不改 runtime、tool schema、sandbox、exec-server 或 provider adapter。
  - 文件名里保留 `gpt-5.2-codex`，这是现有模型/模板命名遗留，后续模型 catalog 收敛时统一处理。
- 验证：
  - `rg -n "You are Codex|OpenAI general-purpose|You are Astral|You are a coding agent running in astral-code"`
    已确认 prompt/model-instructions 区域只剩 Astral/provider-neutral 身份句。
  - `git diff --check` 通过。
- 磁盘：
  - 本 slice 未触发新的 Rust 编译。
  - 清理 incremental 后可用空间约 24Gi。

### 最新补充 94（2026-06-12 本轮）

收敛 login auth 主模块里的 OpenAI 命名残留。

- 旧状态：
  - `codex-rs/login/src/auth/manager.rs` 中拒绝旧 hosted 凭据的常量仍叫
    `UNSUPPORTED_OPENAI_AUTH_MESSAGE`。
  - 运行时行为已经是 Astral-only：ChatGPT / Agent Identity / Personal Access Token 存储凭据都会被拒绝，
    但源码命名仍把这条路径标成 OpenAI auth。
- 新状态：
  - 常量重命名为 `UNSUPPORTED_LEGACY_HOSTED_AUTH_MESSAGE`。
  - 错误消息保持 provider-neutral：`Stored upstream hosted credentials are not supported by Astral. Use API key auth instead.`
  - `from_auth_dot_json`、`from_agent_identity_jwt`、`from_personal_access_token` 三条 legacy hosted auth
    拒绝路径统一使用新的 legacy-hosted 常量。
- 保留边界：
  - 行为不放宽：旧 ChatGPT/PAT/AgentIdentity 凭据仍然不可用。
  - 不改 token payload 里的历史 `chatgpt_*` claim 字段；那是旧 token 结构解析壳，不是新 Astral
    auth 入口。
  - 不碰 sandbox、exec-server、approval 或 provider adapter。
- 验证：
  - `just fmt` 通过。
  - `just test -p codex-login stored_chatgpt_auth_without_api_key_is_rejected` 通过：1 个测试通过，49 个 skipped。
  - `git diff --check` 通过。
- 磁盘：
  - 因清过 incremental 后触发 `codex-login` 依赖链重编译，本轮测试耗时较长。
  - 测试后可用空间约 21Gi，`codex-rs/target/debug/incremental` 约 2.7G。
  - 下一步清理 Astral-Code 自己的 incremental 缓存。

### 最新补充 95（2026-06-12 本轮）

收敛 connectors / plugins 缓存 key 的 ChatGPT 用户字段命名。

- 旧状态：
  - `AccessibleConnectorsCacheKey`、`ConnectorDirectoryCacheKey`、`FeaturedPluginIdsCacheKey`
    都使用 `chatgpt_user_id` 字段。
  - Astral API-key/provider-neutral 主路径下这个值基本为 `None`，但 cache key 是 Astral 自己的内部
    状态，不应该继续把 ChatGPT 当默认语义。
- 新状态：
  - 这三个 cache key 字段统一改成 `legacy_user_id`。
  - 仍然通过 `CodexAuth::get_chatgpt_user_id` 从旧 token payload 读取历史 claim；这个方法名和
    `chatgpt_*` claim 暂时保留，因为它描述的是旧 token 结构，不是新的 Astral auth 入口。
  - `ConnectorDirectoryCacheKey` 的序列化字段随之变成 `legacy_user_id`，新项目不维护旧 cache key
    兼容路径。
- 保留边界：
  - 不改 connector/app 功能行为。
  - 不改旧 auth fixture 里的 `chatgpt_user_id` claim。
  - 不改 MCP、plugin runtime、sandbox 或 exec-server。
- 验证：
  - `just fmt` 通过。
  - `rg -n "chatgpt_user_id" core/src/connectors.rs connectors/src/lib.rs core-plugins/src/manager.rs`
    只剩读取旧 claim 的 `CodexAuth::get_chatgpt_user_id` 调用。
  - `git diff --check` 通过。
  - 未额外跑 `codex-core` / `codex-core-plugins` 测试，避免再次触发长时间重编译和磁盘压力；后续阶段性测试
    应覆盖这三个 crate。
- 磁盘：
  - 当前可用空间约 23Gi。

### 最新补充 96（2026-06-12 本轮）

收敛 memory write/read 路径里的模型可见 Codex/OpenAI 文案。

- 旧状态：
  - `codex-rs/memories/write/templates/memories/consolidation.md` 仍把 memory workspace 描述为
    Codex 管理，并在 retrieval-bias 例子中使用 `api.openai.org/v1/files`、`OpenAI Internal Slack`。
  - `codex-rs/memories/write/src/prompts.rs` 的 fallback prompt 仍写
    `Consolidate Codex memories`。
  - `codex-rs/memories/write/src/workspace.rs` 生成的 diff 文件头仍写
    `Generated by Codex`。
  - `codex-rs/ext/memories/src/tools/{ad_hoc_note,list,read,search}.rs` 的 tool schema 描述仍把
    memory store/file 叫 Codex memories。
- 新状态：
  - memory consolidation prompt 和 workspace diff 头统一改成 Astral。
  - OpenAI 专有 retrieval 例子改成 provider-neutral 的 `api.example.internal/v1/files`、
    `Internal Slack`。
  - memory extension 四个 tool 描述统一改成 Astral memory store/file。
- 保留边界：
  - 不改 memory 存储根目录、不改 `codex_home` 内部结构字段、不改 metrics 名、不改 crate 名。
  - 不改 local compact、memory consolidation 行为、sandbox、exec-server 或 provider adapter。
  - 这次只处理模型/agent 会看到的文案，不把内部历史命名强行机械清空。
- 验证：
  - `just fmt` 通过。
  - `just test -p codex-memories-extension` 通过：15 个测试全部通过。
  - `git diff --check` 通过。
  - 定向 `rg` 已确认上述 memory prompt/tool schema 里旧的 Codex/OpenAI 示例文案被覆盖。
- 测试成本记录：
  - `codex-memories-extension` 这次冷编译触发了 `codex-api`、`app-server-protocol`、
    `codex-core` 等共享依赖，编译耗时约 23 分 34 秒，实际测试只跑约 0.05 秒。
  - 后续类似“纯模型可见文案”改动，优先使用 `fmt + diff-check + targeted rg`，除非改到逻辑路径，
    避免每个小 slice 都触发重编译。
- 磁盘：
  - 测试后可用空间约 21Gi，`codex-rs/target/debug/incremental` 约 2.2G。
  - 已只清理 Astral-Code 自己的 `codex-rs/target/debug/incremental`，可用空间回到约 22Gi。

### 最新补充 97（2026-06-12 本轮）

收敛 CLI/login 中残留的 legacy hosted auth 用户可见文案。

- 旧状态：
  - `astral login --with-api-key` 的 help 仍写 `Astral-managed auth`，容易和旧 hosted managed auth
    语义混在一起。
  - remote exec-server 注册时如果拿到旧 ChatGPT / Agent Identity auth，错误消息直接点名
    `ChatGPT and Agent Identity auth are disabled in Astral`。
  - `codex-rs/login/src/auth/manager.rs` 的 unauthorized refresh 注释仍写
    `managed ChatGPT/OAuth token refresh`。
- 新状态：
  - `--with-api-key` help 改成 `Astral API-key auth`。
  - remote exec-server auth 错误改成 `legacy hosted credentials are disabled in Astral`。
  - unauthorized refresh 注释改成 `managed legacy-hosted/OAuth token refresh`。
- 保留边界：
  - 不改 `is_supported_exec_server_remote_auth` 判断：remote exec-server 仍只接受 API-key auth。
  - 不改旧 token fixture、`chatgpt_user_id` 结构字段或测试 helper 名；这些描述的是旧 token payload。
  - 不改 app-server、exec-server、sandbox、approval 或 provider adapter。
- 验证：
  - `just fmt` 通过。
  - `git diff --check` 通过。
  - 定向 `rg` 确认 `managed ChatGPT/OAuth`、`ChatGPT and Agent Identity auth are disabled`、
    `Astral-managed auth` 已不再出现在相关 CLI/login 文件中。
- 测试：
  - 未跑额外 Rust 测试；本 slice 仅改变 help/error/comment 文案，上一条 memory slice 已暴露冷编译成本偏高。

### 最新补充 98（2026-06-12 本轮）

修正 DeepSeek/OpenAI-compatible 示例配置，避免真实 smoke test 走错协议或 URL。

- 旧状态：
  - `docs/example-config.md` 仍是旧占位文档，或示例里使用过期的 `wire_api = "chat"`。
  - 示例 `base_url` 未明确带 `/v1`，会让 `chat/completions` endpoint 拼到错误路径。
  - 示例表结构没有展示真实的 provider catalog 写法。
- 新状态：
  - 示例改成：
    - `model = "deepseek-v4-pro"`
    - `model_provider = "deepseek"`
    - `[model_providers.deepseek]`
    - `base_url = "https://api.deepseek.com/v1"`
    - `env_key = "ASTRAL_API_KEY"`
    - `wire_api = "chat_completions"`
- 保留边界：
  - 不改内置 Astral provider：默认仍是 `https://api.deepseek.com/v1` + `ASTRAL_API_KEY`
    + `WireApi::ChatCompletions`。
  - 不改 provider runtime 或 adapter。
- 验证：
  - `git diff --check` 通过。
  - 定向 `rg` 确认 docs / README 中不再有用户可复制的 `wire_api = "chat"` 或错误 DeepSeek base URL。
  - 剩余 `wire_api = "chat"` 只在 model-provider-info 的负向测试里，用来验证旧值会报错。

### 最新补充 99（2026-06-12 本轮，已撤销）

补齐 `deepseek-v4-flash` 的 Astral 内置模型识别路径，方便后续快速真实 smoke test。

注意：这个决定已在最新补充 44 中撤销。Astral 不再内置 `deepseek-v4-flash`，也不再从
`deepseek-v4-pro` 派生任何隐藏模型预设。后续真实 smoke test 应通过用户配置或临时测试配置显式声明
provider、model、base URL、context window 和 modality。

- 旧状态：
  - 内置 `models.json` 只有 `deepseek-v4-pro`。
  - 用户提供的 `deepseek-v4-flash` slug 如果直接使用，会落到 fallback model metadata，
    UI 和 runtime 仍能跑，但会丢失 Astral 默认模型能力描述、reasoning 默认值、parallel tool
    calling、图片输入等元数据。
- 当时状态（已撤销）：
  - `codex-rs/models-manager/src/manager.rs` 在加载 bundled catalog 时，从
    `deepseek-v4-pro` 派生一个 `deepseek-v4-flash` 内置条目。
  - flash 继承 Astral 的基础 instructions、Claude-ish tool flavor 指令、sandbox/exec 元数据、
    图片输入和 parallel tool calling 能力。
  - flash 的显示名、描述、默认 reasoning effort 和优先级独立设置：
    `DeepSeek V4 Flash`、快速 coding/smoke 用途、默认 low reasoning、排在 pro 之后。
- 保留边界：
  - 不复制大段 `models.json`，避免巨型重复 JSON。
  - 不改 provider adapter、exec-server、sandbox、approval、compact 或 tool runtime。
  - 不把未知模型 fallback 行为删除，第三方自定义 slug 仍可继续使用 fallback metadata。
- 当时验证：
  - 已执行 `just fmt`。
  - 未跑重测试；这是 bundled catalog 派生逻辑，后续真实 smoke test 会覆盖。

### 最新补充 100（2026-06-12 本轮）

锁定并开始落地后台终端工具拆分、多模态降级和上下文窗口策略。这三点都是 Astral
面向国产/多 provider 模型的核心 harness 约束，不是 UI 文案层的小改名。

- 后台终端工具命名正式锁定：
  - `Bash`：启动命令，返回 `task_id`。
  - `ReadTaskOutput`：只读取/轮询输出。
  - `SendTaskInput`：只向 stdin 写入交互输入，例如 `y\n`、REPL 命令。
  - `ListBackgroundTasks`：列出 live/recent 后台任务，解决模型忘记 `task_id` 的问题。
  - `StopBackgroundTask`：按 `task_id` 干净终止任务。
- 实现方向：
  - 不重写 Codex exec 骨架。
  - `task_id` 在 v1 内部直接映射 UnifiedExec process/session id。
  - `Bash` 继续走 Codex `UnifiedExecProcessManager + Environment + ExecBackend + approval/sandbox`。
  - `ReadTaskOutput` / `SendTaskInput` 复用原 `write_stdin` 后端语义，但模型侧 schema 不再把“看输出”和“写输入”
    混在 `Monitor` 一个工具里。
  - `ListBackgroundTasks` 只读 UnifiedExec manager snapshot。
  - `StopBackgroundTask` 复用 UnifiedExec terminate 路径。
- 当前代码状态：
  - `codex-rs/tools/src/astral_flavor.rs` 已加入上述四个后台任务工具 schema，并把旧 `Monitor` 从
    Astral core tool list 移出；后续又删除了 `Monitor` schema helper、导出常量和旧 handler 文件，避免
    继续留下半新半旧的模型可调用面。
  - `codex-rs/core/src/tools/handlers/astral_background_tasks.rs` 已新增四个 Astral-native handler。
  - `codex-rs/core/src/unified_exec/*` 已补充 background task snapshot 和 terminate/list 所需的公开 manager
    能力。
  - `codex-rs/core/src/tools/spec_plan.rs` 已把 UnifiedExec 注册面切到新工具组。
  - 实现审计确认新 handler 没有 `std::process` / 本地 `ps` / 本地 kill 旁路，只通过
    `WriteStdinHandler` 和 `unified_exec_manager` 进入 Codex 可替换执行后端。
- 多模态策略正式锁定：
  - Astral 内部原始历史不删除图片、不破坏多模态 session。
  - 每次请求发送前，根据当前模型声明能力做“请求投影”。
  - 当前模型是单模态或能力未知时，图片/视觉片段降级为有界文本占位，而不是原样塞 image block 让 API
    报错或让 session 变成只能用多模态模型。
  - 切回多模态模型时，原始图片上下文仍可恢复。
- 上下文窗口策略正式锁定：
  - 不维护任何内置 provider/model 预设 catalog。
  - 用户在 provider/model 配置中声明 `context_window`、`max_output_tokens`、输入模态、工具能力、reasoning/cache
    能力等。
  - 未声明能力走保守默认：文本-only、小窗口、少做激进上下文塞入。
  - 这样国内新模型快速出现时，只要仍走 `/anthropic` 或 OpenAI-compatible `/v1/chat/completions`，
    Astral 不需要每月追内置表。
- 验证：
  - `just fmt` 已通过。
  - 修复 `cwd` move 后，`CARGO_INCREMENTAL=0 cargo check -p codex-core --lib` 通过。
  - 删除旧 `Monitor` 残影后，`just fmt && CARGO_INCREMENTAL=0 cargo check -p codex-tools -p codex-core --lib`
    通过。

### 最新补充 101（2026-06-12 本轮）

推进模型能力声明、多模态安全降级和后台任务生命周期语义。

- 多模态/单模态策略从“决策”推进到配置主路径：
  - 新增 `model_input_modalities` 配置字段，可在 `config.toml` 中声明当前模型输入能力，例如
    `["text"]` 或 `["text", "image"]`。
  - `ConfigToml -> Config -> ModelsManagerConfig -> ModelInfo` 链路已接通。
  - `codex-rs/core/config.schema.json` 已通过 `just write-config-schema` 更新。
- 未知模型能力默认更保守：
  - `model_info_from_slug(...)` 的 unknown/fallback model 现在默认 `input_modalities = ["text"]`。
  - OpenAI-compatible `/models` 只返回 id 的 listing 也默认 `["text"]`。
  - 只有 bundled catalog、Astral models response 或用户配置明确声明 image 时，才认为模型可接收图片。
- 这与现有 context 投影机制配合：
  - `ContextManager::for_prompt(&model_info.input_modalities)` 已经在 prompt snapshot 上调用
    `strip_images_when_unsupported(...)`。
  - 该路径是 clone 后 normalize，原始 `raw_items()` 历史仍保留图片；单模态模型只收到文本占位，切回多模态模型
    后仍可恢复原始图片上下文。
  - 这正是用户要求的“不要像 Claude Code 那样一旦 session 混入图片，单模态模型就废掉”。
- 后台任务生命周期语义确认：
  - `Bash` 启动的是当前 Astral session scoped task，不是 durable job。
  - 本地 PTY 的 `UnifiedExecProcess::drop()` 会调用 `terminate()`；TUI 正常 `/exit` 或关闭进程时，当前 harness
    持有的后台命令应随 session 清理。
  - stdio exec-server 客户端 drop 已有测试覆盖会终止 spawned server process tree。
  - 远端/常驻 exec-server 也会走 terminate/unregister 语义，但如果网络断开或远端 daemon 失联，未来还需要
    session lease/heartbeat TTL 才能把“退出一定清理”做成强保证。
  - 因此 `ListBackgroundTasks` 只承诺找回当前 harness/session 还持有的任务；未来如需跨 session 持久任务，应另建
    `Job` / `Workflow` 语义，不要偷换现有 PTY task。
- 验证：
  - `just fmt` 通过。
  - `CARGO_INCREMENTAL=0 cargo check -p codex-config -p codex-models-manager -p codex-api -p codex-core --lib`
    通过。
  - `just write-config-schema` 通过。
  - `just test -p codex-models-manager model_input_modalities` 通过。
  - `just test -p codex-models-manager unknown_model_defaults_to_text_only_input` 通过。
  - `just test -p codex-api parses_openai_models_list_response` 通过。
- 磁盘：
  - schema 生成后可用空间降到约 15Gi。
  - 已只清理 Astral-Code 项目内 `codex-rs/target/debug/incremental`，随后测试再次生成的 incremental 也已清理。
    当前可用空间约 16Gi。

### 最新补充 102（2026-06-12 本轮）

重开 Goal 后，目标口径已经从“无限清 OpenAI 残留”收敛为“最终可用验收”。新目标强调：

- `astral-code` / `astral` 是全新项目。
- 继承 Codex 的 app-server、exec-server、UnifiedExec、PTY、sandbox、approval、Plan Mode、Goal Mode、
  local compact、MCP、skills/plugins 和可替换执行后端。
- provider-neutral 主循环必须真实支持 `/anthropic` 和 OpenAI-compatible `/v1/chat/completions`。
- Claude-ish core tools 与后台任务工具要能真实闭环。
- Codex 原生 Goal tools 直接继承，不重新设计。
- OpenAI/hosted 清理只针对实际运行控制面，不追求所有历史命名和 legacy fixture 字符串清零。
- 最终完成条件包括 DeepSeek 等真实模型端到端测试、CLI/TUI、compact、`/model` 切换、Plan/Goal、MCP/skills/plugins
  基础路径都能真实完成任务。

本轮完成第一个新 Goal 下的原子切片：收口 app-server hosted remote plugin control-plane。

- 修改文件：
  - `codex-rs/app-server/src/request_processors/plugins.rs`
  - `codex-rs/app-server/src/request_processors.rs`
  - `codex-rs/app-server/src/message_processor.rs`
- 行为变化：
  - `plugin/list` 只列本地 marketplace，不再构造 hosted service config，不再尝试 remote/global/shared/vertical catalog fetch。
  - `plugin/installed` 只读取本地 installed/suggested plugins，不再启动 remote installed bundle sync，不再从 remote cache
    合并 marketplace。
  - `plugin/read` 的 remote marketplace 分支直接返回 Astral 不支持 hosted control-plane。
  - `plugin/install` 的 remote marketplace 分支直接返回 Astral 不支持 hosted control-plane。
  - `plugin/uninstall` 对 remote plugin id 直接返回 Astral 不支持 hosted control-plane。
  - 本地 plugin install/uninstall/read、plugin MCP OAuth login、skills、apps auth 检测和本地 marketplace 流程保持不动。
- 删除代码：
  - remote installed visible scope/filter conflict helper。
  - app-server remote installed loader。
  - app-server remote install/uninstall 方法。
  - remote marketplace/detail 转换 helper。
  - remote bundle install error helper。
  - `PluginRequestProcessor` 中只服务 remote install telemetry 的 `analytics_events_client` 字段。
- 验证：
  - `just fmt` 通过。
  - `CARGO_INCREMENTAL=0 cargo check -p codex-app-server --lib` 通过且无新增 warning。
- 下一刀建议：
  - 继续清理 `plugin/share/*` 和 `plugin/skill/read` 中仍然保留的 remote share control-plane 死分支，或者转向
    Guardian -> Astral approval reviewer。优先级上，若继续同一模块，先把 `remote_plugin_control_plane_enabled()`
    及 share 相关 remote helper 清掉。

### 最新补充 103（2026-06-12 本轮）

继续同一模块第二个原子切片：清理 app-server `plugin/share/*` 与 `plugin/skill/read` 的 hosted share
control-plane 死分支。

- 修改文件：
  - `codex-rs/app-server/src/request_processors/plugins.rs`
  - `codex-rs/app-server/src/request_processors.rs`
- 行为变化：
  - `plugin/skill/read` 直接返回 Astral 不支持 hosted control-plane。
  - `plugin/share/save`、`plugin/share/updateTargets`、`plugin/share/checkout`、`plugin/share/delete`
    直接返回 Astral 不支持 hosted control-plane。
  - `plugin/share/list` 保持 disabled 行为，返回空列表。
  - 不改本地 plugin、MCP、skills、apps、本地 marketplace install/uninstall/read。
- 删除代码：
  - `remote_plugin_control_plane_enabled()`。
  - remote share discoverability/target/principal 参数转换。
  - remote share checkout/list/save/update/delete 运行链路。
  - remote skill detail fetch 运行链路。
  - remote plugin catalog error -> JSON-RPC 映射 helper。
  - `RemotePluginServiceConfig` / `RemotePluginCatalogError` / remote share summary/context import。
- 验证：
  - `just fmt` 通过。
  - `CARGO_INCREMENTAL=0 cargo check -p codex-app-server --lib` 通过。
- 结果：
  - app-server 插件请求处理层已经不再保留 hosted remote marketplace/share/install/uninstall 的实际运行控制面。
  - 后续如果继续清理 remote/cloud，可转向 `core-plugins/src/remote*` 命名残留、`memories/write`、account/auth
    残留或 Guardian -> Astral approval reviewer。

### 最新补充 104（2026-06-12 本轮）

Guardian / auto-review 从“硬禁用旧 hosted Guardian”推进到“Astral 显式 opt-in 当前 provider reviewer”。

- 修改文件：
  - `codex-rs/core/src/guardian/review.rs`
  - `codex-rs/core/src/guardian/tests.rs`
  - `codex-rs/core/src/session/tests/guardian_tests.rs`
  - `codex-rs/core/src/session/tests.rs`
- 行为变化：
  - `routes_approval_to_guardian_with_reviewer(...)` 不再无条件返回 `false`。
  - 只有 `approvals_reviewer = "auto_review"` 且 `Feature::GuardianApproval` 开启时，approval 才会进入本地 reviewer。
  - `AskForApproval::Granular(...)` 仍然拒绝进入 reviewer，避免和细粒度审批策略冲突。
  - reviewer session 继续复用现有 `build_guardian_review_session_config(...)`：继承当前 config、当前
    `model_provider` 和当前 active model，不恢复 OpenAI hosted Guardian / catalog override / 外部控制面。
  - feature 未开启时，即使配置 `auto_review`，`RequestPermissions` 仍走普通用户审批事件。
- 顺手修复：
  - `session_settings_model_provider_update_rejects_unknown_provider` 不再用 `expect_err` 要求
    `SessionConfiguration: Debug`，改成 match 取错误。这个是窄测编译时暴露的测试写法问题，不改生产类型。
- 验证：
  - `just fmt` 通过。
  - `just test -p codex-core routes_approval_to_guardian` 通过 3 个测试。
  - `just test -p codex-core request_permissions_uses_user_approval_when_auto_review_feature_is_disabled request_permissions_user_approval_wait_stops_when_cancelled`
    通过 2 个测试。
- 结果：
  - Goal 中“将 Guardian/auto-review Astral 化为可选当前模型 approval reviewer”已经完成第一阶段可用闭环。
  - 后续重点不是继续拆 Guardian 名字，而是真实 E2E 验证 auto-review 在 DeepSeek/Anthropic/chat-completions
    provider 下的审批请求体、失败恢复和 timeout 行为。

### 最新补充 105（2026-06-12 本轮）

模型 provider catalog 向“无厂商预设、用户显式声明”收敛。

- 修改文件：
  - `codex-rs/model-provider-info/src/lib.rs`
  - `codex-rs/model-provider-info/src/model_provider_info_tests.rs`
  - `codex-rs/core/src/config/config_tests.rs`
- 行为变化：
  - `astral` bootstrap provider 不再内置 `https://api.deepseek.com/v1` 作为默认 base URL。
  - `ASTRAL_BASE_URL` 仍可作为显式环境配置来源；用户也可以在 `model_providers.<id>.base_url` 中声明 provider。
  - `ModelProviderInfo::to_api_provider(...)` 在 provider 没有 `base_url` 时返回明确配置错误，不再静默 fallback 到任何厂商 URL。
  - `merge_configured_model_providers(...)` 现在允许用户配置的同名 provider 覆盖 bootstrap provider。
  - 这意味着 `astral`、`anthropic`、`amazon-bedrock` 等 bootstrap id 不再是权威预设；用户可以用自己的
    `/anthropic` 或 OpenAI-compatible `/v1/chat/completions` endpoint 覆盖它们。
- 保留边界：
  - 本 slice 暂不删除 `ollama` / `lmstudio` bootstrap id，因为 `--oss` 本地选择路径仍依赖这两个 provider id。
  - DeepSeek URL 只作为测试里的“用户配置示例字符串”存在，不再是运行时默认。
- 验证：
  - `just fmt` 通过。
  - `just test -p codex-model-provider-info` 通过 23 个测试。
  - `just test -p codex-core load_config_allows_configured_provider_to_override_bootstrap_provider` 通过 1 个测试。
- 结果：
  - 符合“国产模型很多、能力和上下文窗口由用户配置声明，Astral 不维护内置厂商预设”的方向。
  - 后续还要继续审计模型 catalog / model picker 的 OpenAI preset 文案和默认模型列表，避免 UI 继续暗示只有
    GPT/Claude 固定 catalog。

最新补充 44（2026-06-12 本轮）：模型 catalog 运行时正式改成 provider/user-declared。

- 完成项：
  - `OpenAiModelsManager::new(...)` 不再从 bundled `models.json` 初始化运行时模型目录，启动时目录为空。
  - provider `/models` 或 cache 命中后会替换当前运行时目录，不再和旧 bundled GPT/OpenAI catalog overlay。
  - provider 返回空目录时，运行时目录保持为空，不再 fallback 到 bundled catalog。
  - provider 删除模型后，下一次刷新会真实移除旧模型，避免 `/model` 列表保留 provider 已经撤掉的条目。
  - unknown model fallback 不再声明 `context_window = 272000` / `max_context_window = 272000`；窗口大小必须来自
    provider catalog 或用户配置。
  - unknown model fallback 仍默认 `input_modalities = [text]`，与“单模态模型遇到图片时降级成文本占位”的方向一致。
  - 删除 `bundled_models_response()` 里从 `deepseek-v4-pro` 克隆 `deepseek-v4-flash` 的隐藏预设逻辑。
  - TUI 模型选择文案从 “built-in models / codex -m” 改成 “catalog models / astral -m”。
- 保留边界：
  - `bundled_models_response()` 仍作为测试/fixture loader 存在，但不再参与 runtime `OpenAiModelsManager` 默认目录。
  - 本 slice 没有改 sandbox、exec-server、UnifiedExec、Plan/Goal、MCP 或 tool runtime。
- 验证：
  - `just fmt` 通过。
  - `just test -p codex-models-manager` 通过 35 个测试。
- 结果：
  - 更贴近“内置完全不做模型/provider 预设”的要求。
  - 后续 `/model` provider 分组 UI 应该基于用户配置的 provider 列表和各 provider catalog，而不是 bundled catalog。

最新补充 45（2026-06-12 本轮）：`/model` 热切换的 provider+model 基础链路收口。

- 完成项：
  - `AppEvent::PersistModelSelection` 新增可选 `model_provider`，跨 provider 选择时可以同时保存 provider 和 model。
  - TUI `UpdateModel { model_provider: Some(...) }` 现在会立即同步 App/ChatWidget 的本地 provider 状态，不再只等
    app-server 回推 `ThreadSettingsUpdated`。
  - config 写入 helper `build_model_selection_edits(...)` 支持写入 `model_provider`，避免用户在 TUI 中切到另一个
    provider 后重启又回到旧 provider。
  - Plan mode reasoning scope 的 all-modes 路径保留并传递 provider，当前 provider 内切模型继续传 `None`，旧行为不变。
  - `astral exec` 的 `TurnStartParams` 补齐 `model_provider: None`，保持沿用当前线程 provider 的行为。
- 没做的事：
  - 这还不是完整的跨 provider 分组 model picker。当前 app-server `model/list` 仍主要返回当前 provider catalog；
    provider 分组 picker 需要额外设计 catalog 获取方式，不能在 TUI 里伪造。
  - 不改 ThreadManager、exec-server、sandbox、UnifiedExec 或工具执行后端。
- 验证：
  - `just fmt` 通过。
  - `cargo check -p codex-tui -p codex-exec` 通过。
  - `just test -p codex-tui model_selection_edits_can_persist_provider accepted_model_migration_persists_target_default_reasoning_effort`
    通过 2 个测试。
  - `just test -p codex-tui plan_reasoning_scope_popup_all_modes_persists_global_and_plan_override` 通过 1 个测试。
- 磁盘：
  - TUI 编译后磁盘可用空间降到约 15Gi。
  - 已只清理 Astral-Code 项目内 `codex-rs/target/debug/incremental`，可用空间回到约 19Gi。
  - 未删除 `codex-rs/target/debug/deps`，避免显著拖慢后续开发。
- 结果：
  - 这是一块基础设施补强，不继续深挖 UI 细节，避免在 `/model` picker 上死循环。
  - 下一步回到主线硬块：Claude-ish tool/result shape、后台任务工具闭环、单模态图片降级、真实 DeepSeek E2E。

## 剩余高优先级工作

1. 审计并清理剩余 OpenAI/ChatGPT auth/config 面
   - core config 和 remote/cloud 模块中剩余的 `chatgpt_base_url`
   - app-server account docs/tests 中残留的 ChatGPT auth 语义
   - `AuthMode::ChatGPT` / PAT / Agent Identity 是否需要彻底删除、隔离或标记 legacy unsupported

2. 审计 remote/cloud control-plane
   - `backend-client` 默认 URL 推断已不再自动进入 `/wham/*`；后续可评估是否彻底删除显式
     `HostedApi` variant。
   - `cloud-config` 旧 hosted remote bundle service/cache/backend 已删除；后续只剩全仓命名复扫。
   - `cloud-tasks` 默认 `/wham/*` hosted 路径已切断；后续只需复扫旧 auth/header 语义。
   - `core-plugins/src/remote*` 主 hosted catalog/share/sync 已降级为 Astral disabled stub。
   - app-server `plugin/list`、`plugin/installed`、`plugin/read`、`plugin/install`、`plugin/uninstall`
     的 hosted remote marketplace/install/uninstall 运行路径已切断并删除死 helper。
   - app-server `plugin/share/*`、`plugin/skill/read` 的 hosted share/skill-read 运行路径已切断并删除死 helper。
   - `memories/write`
   - 目标：默认路径不能静默访问 `chatgpt.com/backend-api`。
   - app-server remote control 暴露入口已禁用；下一刀建议转向 `memories/write`、`cloud-config`
     或 app-server account/auth 残留。

3. 推进 provider-neutral protocol
   - Anthropic Messages stream/tool_use/tool_result。
   - OpenAI-compatible chat-completions stream/tool_calls。
   - usage、stop reason、error recovery 映射。
   - Responses legacy adapter 去中心化。

4. 硬化 Claude-ish tool result
   - 必要时对照 `/Users/oines/project/claude-code` 源码。
   - 必要时真实跑 Claude Code 抓 fixture。
   - 优先校准 `Bash`、`ReadTaskOutput`、`SendTaskInput`、`ListBackgroundTasks`、`StopBackgroundTask`、
     `Read`、`Edit`、`TodoWrite`、`RequestPermissions` 的 schema/result shape。
   - `Read` / `Write` / `Edit` / `Glob` / `Grep` 的 runtime boundary 已有
     `ExecutorFileSystem` 测试保护；后续改 schema/result 时不能绕过这个抽象。
   - multi-agent/subagent 暂不继续 Claude-ish 化，保持 Codex 原版工具面。

5. 验证 terminal agentic 体验
   - 后台长命令持续 monitor。
   - y/n prompt 可写 stdin。
   - ffmpeg 等长任务有进度输出。
   - 后台 shell 可通过 Codex UnifiedExec 路径继续控制和终止；模型侧使用
     `ReadTaskOutput` / `SendTaskInput` / `ListBackgroundTasks` / `StopBackgroundTask`，不要为了 Claude
     `TaskStop` 重写 subagent。
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
   - `codex-rs/models-manager`：继续收敛旧 ChatGPT auth fallback 测试和 hosted-only model filtering；
     provider `/models`、cache、ETag 必须保持可用，但不要恢复 bundled catalog overlay。
   - `codex-rs/login` / `codex-rs/app-server-protocol`：评估 `AuthMode::ChatGPT`、PAT、
     Agent Identity 等 legacy token 类型是否彻底删除、隔离或保持 unsupported compatibility shell。
   - `codex-rs/core/src/config` / `codex-rs/config`：继续收敛旧 ChatGPT auth/config 字段，注意不要破坏新的
     `ASTRAL_BASE_URL` / provider-neutral 配置。

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

> 完成 Astral-Code 最终可用版：以 astral-code/astral 为全新项目，继承 Codex 的 app-server、exec-server、
> UnifiedExec、PTY、sandbox、approval、Plan Mode、Goal Mode、local compact、MCP、skills/plugins 和可替换执行后端；
> 完成 provider-neutral 主循环，真实支持 `/anthropic` 与 OpenAI-compatible `/v1/chat/completions`；实现
> Claude-ish core tools 与后台任务工具；保留并接通 Codex 原生 Goal tools；移除实际运行控制面的 OpenAI/hosted
> 专有依赖；将 Guardian/auto-review Astral 化为可选当前模型 approval reviewer；隔离 `/v1/responses` 为 legacy
> adapter；支持用户配置模型能力和单模态图片降级；完成 DeepSeek 等真实模型端到端测试，确保 CLI/TUI、工具闭环、
> 长任务、交互输入、权限提权、compact、`/model` 切换、Plan/Goal、MCP/skills/plugins 基础路径都能真实完成任务。

目前还没有达到 complete 条件。当前最重要的是按原子提交继续推进剩余硬块，不再把所有历史命名残留都当成 v1
blocker；真正 blocker 是运行控制面、provider/tool 真实闭环和端到端验收。

## 最新补充 46：全局航向校准与两个风险收口

用户提醒不要卡在单点死循环后，重新对照 Goal 做了一轮全局校准：

1. 当前主线不是继续无限清理历史 OpenAI/Codex 命名残留，而是把 Astral-Code 做到真实可用：
   - provider-neutral 主循环可真实调用国内常用 OpenAI-compatible API 和 `/anthropic`/Messages API。
   - Claude-ish core tools 能形成模型顺手的轨迹形状。
   - terminal agentic 体验保持 Codex 的 UnifiedExec/PTY 丝滑度。
   - 单模态模型不会因为历史中存在图片而废掉整个 session。
   - 最后用 DeepSeek 等真实模型做最小端到端 smoke。

2. `Bash` 与后台任务工具不是绕过 Codex 的脏 adapter：
   - 模型侧暴露 `Bash`、`ReadTaskOutput`、`SendTaskInput`、`ListBackgroundTasks`、`StopBackgroundTask`。
   - handler 侧改写参数后复用 Codex 原生 `ExecCommandHandler`、`WriteStdinHandler` 和
     `UnifiedExecProcessManager`。
   - `Bash(run_in_background=true)` 通过短 `yield_time_ms` 快速返回，普通 tool output 会出现
     `Task running with task_id ...`。
   - `ReadTaskOutput` 是对同一 task 的空 stdin poll。
   - `SendTaskInput` 是对同一 task 写 stdin，例如 `y\n`。
   - `ListBackgroundTasks` 读取 UnifiedExec 进程表，包含 `task_id`、`command`、`cwd`、`status`、
     `exit_code`、`tty`、`elapsed_ms`、`last_used_ms_ago`。
   - `StopBackgroundTask` 调用 UnifiedExec 的 `terminate_process`。
   - 结论：这条线保持了可替换执行后端抽象，没有直接绑死本地终端或本地磁盘。

3. 单模态图片降级路径已确认：
   - `model_info.input_modalities` 已经是模型能力声明入口。
   - 主循环构造请求前使用 `history.for_prompt(&turn_context.model_info.input_modalities)`。
   - `for_prompt` 在模型不支持图片时会把 message/tool output 中的图片替换为文本占位。
   - MCP tool result 也已有 `sanitize_mcp_tool_result_for_model`，在不支持图片时把 MCP 图片块替换为
     `<image content omitted because you do not support image input>`。
   - provider-neutral/Anthropic request 是从已经净化过的 prompt 转为 AgentRequest，因此不应绕过这层。

4. 下一步优先级：
   - 等当前 focused core 测试自然结束，确认后台任务工具窄测结果。
   - 若无失败，将后台任务工具标记为完成，不继续打磨小 UX。
   - 进入真实 smoke：临时 `ASTRAL_HOME` + DeepSeek OpenAI-compatible provider + 最小 `astral exec`
     工具调用任务。
   - `/anthropic` smoke 需要先确认目标 provider 的实际 Messages-compatible base path；不要假设 DeepSeek 官方一定提供。
   - 更新文档、原子提交、push。

5. 恢复注意：
   - 不要把 `/model` 分组 UI、历史命名残留、旧 Codex 数据兼容当作当前 blocker。
   - 不要重改 subagent；用户已决定保持 Codex 原版。
   - 不要重做 compact；目前保留 Codex local compact。
   - 不要破坏 sandbox、approval、exec-server、UnifiedExec、Environment/ExecBackend。

## 最新补充 47：DeepSeek smoke 抓到 hidden legacy tool 泄漏并修复

完成了一轮真实 DeepSeek OpenAI-compatible smoke：

1. 基础工具闭环通过：
   - 使用临时 `ASTRAL_HOME` 和 `--ephemeral`，不污染用户主配置。
   - `ASTRAL_BASE_URL=https://api.deepseek.com/v1`
   - 模型 `deepseek-v4-flash`
   - prompt 要求模型调用 `Bash` 执行 `printf astral-smoke-ok`。
   - 结果：模型调用 `Bash`，UnifiedExec 执行成功，tool_result 回灌成功，最终回答 `astral-smoke-ok`。
   - usage 中出现较高 cached input，说明当前 prompt 形状可以获得 provider 侧缓存收益。

2. 后台任务 smoke 第一次暴露问题：
   - prompt 要求模型启动持续后台命令，然后 `ListBackgroundTasks`、`ReadTaskOutput`、`StopBackgroundTask`。
   - 模型启动了后台命令并拿到 ID，但仍尝试/引用了 legacy `write_stdin` 路径，且生成过“没有
     StopBackgroundTask 工具”的错误判断。
   - 根因不是 visible tool spec，而是 hidden/dispatch-only runtime 虽然不在模型工具列表里，registry 仍允许模型幻觉旧工具名后直接 dispatch。
   - 这会让残留 Codex flavor 绕开 Astral facade，破坏我们想要的 Claude-ish/Astral 工具轨迹。

3. 修复：
   - 在 `ToolRegistry::dispatch_any_with_terminal_outcome` 边界拒绝直接调用 `ToolExposure::Hidden` 工具。
   - hidden tools 仍然注册，供 Astral-native handler 内部复用：
     - `Bash` 内部复用 `exec_command` / `shell_command`
     - `ReadTaskOutput` / `SendTaskInput` 内部复用 `write_stdin`
   - 但模型直接调用 hidden tool 时，会得到可恢复错误：
     - `exec_command` / `shell_command` → 提示使用 `Bash`
     - `write_stdin` → 提示使用 `ReadTaskOutput` / `SendTaskInput`
     - `update_plan` → 提示使用 `TodoWrite`
     - `request_user_input` → 提示使用 `AskUserQuestion`
     - `request_permissions` → 提示使用 `RequestPermissions`

4. 验证：
   - `just fmt`
   - `just test -p codex-core -E 'test(hidden_tools_are_not_directly_callable_by_model) or test(astral_bash) or test(astral_background)'`
     - 7 passed。
   - 重建 `astral` debug CLI。
   - 再次运行后台任务 DeepSeek smoke：
     - 模型启动 `while true; do echo astral-loop-ok; sleep 1; done`
     - 拿到 task id
     - 读取到重复输出
     - 停止任务
     - 最终回答 `stopped-after-astral-loop-ok`
   - JSON UI 将被停止的命令显示为 `exit_code=-1/status=failed`，这是当前终止进程呈现语义，不影响工具闭环。

5. 磁盘：
   - 重建 CLI 后磁盘降到约 13Gi 可用。
   - 已清理 `codex-rs/target/debug/incremental`。
   - 清理后约 17Gi 可用。

当前结论：Claude-ish Bash/background task 工具链已经从“schema 看起来对”推进到“真实模型能跑通长任务、读取输出并停止任务”。下一步应进入 `/anthropic` 真实/模拟 smoke、compact/Plan/Goal 快速验收和最终端到端收口。

## 最新补充 48：DeepSeek `/anthropic` / Messages API smoke 通过

继续完成 provider-neutral 真实 smoke：

1. 使用自定义临时 provider：
   - provider id：`deepseek-anthropic`
   - `base_url = "https://api.deepseek.com/anthropic/v1"`
   - `env_key = "ASTRAL_API_KEY"`
   - `wire_api = "anthropic_messages"`
   - 模型：`deepseek-v4-flash`
   - 临时 `ASTRAL_HOME` + `--ephemeral` + `--ignore-user-config`

2. 结果：
   - `astral exec` 成功走 Anthropic Messages adapter。
   - 模型调用 `Bash` 执行 `printf anthropic-smoke-ok`。
   - UnifiedExec 返回 tool result。
   - 模型最终回答 `anthropic-smoke-ok`。
   - usage 正常返回，且 cached input 很高。

3. 观察到的小问题：
   - DeepSeek Anthropic-compatible base 下 `/models` 返回 404，导致模型列表刷新打印 error。
   - 会话继续 fallback 到 unknown model metadata，不影响执行。
   - 后续 polish 可以让 provider 声明“禁用 /models refresh”或把 404 降级为非致命 warning，但这不是当前核心 blocker。

当前结论：OpenAI-compatible `/v1/chat/completions` 与 Anthropic-compatible `/v1/messages` 两条真实模型路径均已跑通最小 CLI + Bash tool 闭环。

## 最新补充 49：compact 快速验收暴露测试基座迁移问题

对 compact/宿主模式做了一次 focused 测试：

```bash
just test -p codex-core -E 'test(compact) or test(model_visible_core_tools_convert_to_provider_neutral_astral_names)'
```

结果：

- 76 tests run
- 46 passed
- 2 flaky 后通过
- 30 failed

通过的部分说明：

- compact 纯函数和历史重建类测试通过，包括：
  - `content_items_to_text_*`
  - `build_token_limited_compacted_history_*`
  - `process_compacted_history_*`
  - `reconstruct_history_matches_live_compactions`
  - `model_visible_core_tools_convert_to_provider_neutral_astral_names`

失败模式：

- 失败集中在 integration suite：
  - `suite::compact::*`
  - `suite::compact_resume_fork::*`
  - 部分 `suite::pending_input::*`
  - `suite::window_headers::*`
- 错误基本都是 `timeout waiting for event`。

初步判断：

- 这些测试大量使用 `core_test_support::responses::*` 和 Responses SSE mock。
- `TestCodexBuilder` / compact suite 里仍有 `responses_mock_model_provider(...)`，测试 fixture 仍按 `/responses`
  形状和事件流写。
- Astral 运行控制面已经默认转到 provider-neutral，真实 smoke 已经验证 `/v1/chat/completions` 和 `/v1/messages`
  都能通。
- 因此这批失败更像“旧 Responses compact integration fixture 没迁移到 Astral provider-neutral 测试形状”，不应立即误判为
  local compact 逻辑坏掉。

后续处理建议：

1. 不要继续在这个点死循环。
2. 单独开一个 coherent slice 迁移 compact integration tests：
   - 新增 provider-neutral compact mock helpers，覆盖 chat-completions 和/或 Anthropic Messages stream。
   - 保留少量 Responses legacy tests，只验证 legacy adapter。
   - 把 compact request-shape snapshot 从 ResponsesRequest 改成 AgentRequest/adapter body snapshot。
3. 如果要做真实 runtime compact smoke，使用临时 `ASTRAL_HOME` 和小上下文窗口配置触发 local compact，而不是依赖旧
   `/responses` mock。

当前结论：local compact 逻辑继承路径仍然存在，纯函数/历史重建验收通过；真正剩余 blocker 是 integration test
fixture/provider-neutral 迁移和真实 compact smoke，而不是重新设计 compact。

## 最新补充 50：provider `/models` 不可用退化为正常能力缺失

对齐全局目标后，没有继续卡在 compact fixture 失败点，而是先处理真实 provider-neutral 路径里已经暴露的一个小而关键的兼容坑：
DeepSeek `/anthropic` base 下 inference 可以正常工作，但 `/models` 返回 404，旧路径会把它记录成
`failed to refresh available models` 错误噪声。

本轮改动：

1. `codex-models-manager` 新增 `RemoteModelCatalog`：
   - `Catalog { models, etag }` 表示 provider 返回了远端模型目录。
   - `Unavailable` 表示 provider/gateway 不暴露模型目录 endpoint。

2. `ModelsEndpointClient::list_models(...)` 改为返回 `RemoteModelCatalog`。
   - 真错误仍然走 `CoreResult` 返回。
   - “没有 `/models`”不再伪装成错误。

3. `OpenAiModelsManager::fetch_and_update_models(...)` 遇到 `Unavailable` 时：
   - 不清空当前内存 catalog。
   - 不写空 cache。
   - 记录普通 info：`models endpoint unavailable; keeping current model catalog`。

4. `codex-model-provider` 的 OpenAI-compatible `/models` endpoint 将 HTTP 404 / 405 映射为 `Unavailable`。
   - 这覆盖 DeepSeek Anthropic-compatible base 这类“支持 Messages/inference，但不支持模型目录”的网关。
   - 其他 HTTP 错误仍然按原错误链路上抛。

这符合此前锁定的产品方向：Astral 不维护内置模型预设，模型能力、上下文窗口、多模态能力由用户配置或 provider catalog 提供；
当 provider 没有 catalog 时，运行时应该继续使用用户声明/当前模型 fallback，而不是把 `/models` 404 当成会话错误。

验证：

- `just fmt` 通过。
- `just test -p codex-models-manager refresh_available_models_keeps_current_catalog_when_provider_catalog_unavailable` 通过 1 个测试。
- `just test -p codex-model-provider model_catalog_unavailable_accepts_missing_provider_catalog_routes` 通过 1 个测试。

磁盘维护：

- 本轮窄测后 `codex-rs/target` 约 135G，其中 `debug/deps` 约 131G。
- 只清理了 Astral-Code 项目内低风险 `codex-rs/target/debug/incremental`（约 1.3G）。
- 清理后磁盘剩余约 18Gi；未删除项目外文件，也未删除 `debug/deps`。

当前下一步：

1. 原子提交并 push 当前 provider catalog 兼容切片。
2. 不继续扩大 `/models` 配置面，除非真实 TUI `/model` 使用时还有痛点。
3. 转入真实 smoke 补洞：Plan/Goal/local compact/TUI 基础路径优先，旧 Responses compact integration fixture 单独排后处理。

## 最新补充 51：provider catalog 404 修复的真实 DeepSeek `/anthropic` smoke 通过

在提交 `d06957cfc9 Handle providers without model catalogs` 后，使用临时 `ASTRAL_HOME` 重新跑了一次真实
DeepSeek Anthropic-compatible smoke：

- provider id：`deepseek-anthropic`
- `base_url = "https://api.deepseek.com/anthropic/v1"`
- `wire_api = "anthropic_messages"`
- 模型：`deepseek-v4-flash`
- prompt：要求模型用 `Bash` 执行 `printf catalog-unavailable-smoke-ok`

结果：

- `MODEL_CATALOG_NOISE_ABSENT`。
- 没有再出现 `failed to refresh available models`、`unexpected status 404` 或 `/models` 错误噪声。
- `Bash` 工具调用成功，UnifiedExec 执行 `/bin/zsh -lc 'printf catalog-unavailable-smoke-ok'`。
- 最终 agent message 为 `catalog-unavailable-smoke-ok`。

仍然存在的 warning：

- `Unknown model deepseek-v4-flash is used. This will use fallback model metadata.`
- `Model personality requested but model_messages is missing, falling back to base instructions.`

这两个 warning 和当前决策一致：Astral 不内置国产模型预设；如果用户希望上下文窗口、多模态、personality/base
instructions 更准确，需要在用户配置的 model catalog / model capability override 中声明。它们不影响真实执行闭环。

磁盘状态：

- `cargo run` 重建 CLI 后磁盘剩余约 16Gi。
- `codex-rs/target/debug/incremental` 仍为 0B，没有可安全清理的增量缓存。
- 后续应避免继续跑 core/TUI 重编级测试，直到磁盘空间更宽松或必须验收。

## 最新补充 52：未知模型 fallback metadata 降噪

真实 DeepSeek `/anthropic` smoke 消除了 `/models` 404 噪声后，还剩两个 provider-neutral 场景下很常见的 warning：

- `Unknown model deepseek-v4-flash is used. This will use fallback model metadata.`
- `Model personality requested but model_messages is missing, falling back to base instructions.`

这不是运行错误，而是 Astral 当前“不维护内置模型预设、允许用户自行声明模型能力”的正常退化路径。继续用 WARN 级别会让国内模型用户误以为会话有问题，尤其是每轮都会重复出现。

本轮改动：

1. `models-manager::model_info_from_slug(...)`
   - 未知模型 fallback metadata 从 `warn!` 降为 `info!`。
   - 真实请求错误、认证错误、协议错误仍然通过各自错误链路上报。

2. `ModelInfo::get_model_instructions(...)`
   - 如果模型来自 fallback metadata，并且缺少 `model_messages`，personality fallback 从 `warn!` 降为 `debug!`。
   - 如果是正式 catalog 模型缺少 personality 模板，仍然保留 `warn!`，避免隐藏 catalog 数据问题。

验证：

- `just fmt` 通过。
- `just test -p codex-models-manager model_info` 通过 16 个测试。
- `just test -p codex-protocol get_model_instructions` 通过 3 个测试。

磁盘维护：

- 这轮窄测触发 `codex-protocol` 共享依赖重编后，磁盘剩余约 14Gi。
- 只清理了 Astral-Code 项目内 `codex-rs/target/debug/incremental`（约 1.4G）。
- 清理后磁盘剩余约 15Gi；没有删除 `debug/deps`，因为它是后续开发速度的大头缓存。

当前取舍：

- 暂不为了确认日志消失再次跑真实 DeepSeek smoke，因为这会重建 CLI 并进一步压缩磁盘。
- 下一轮真实端到端验收时顺带观察输出即可。

## 最新补充 53：TUI Goal / Plan / compact UI 基础路径当前验收通过

为了继续朝“最终可用”推进，而不是卡在旧 `/responses` compact fixture 迁移点，本轮先跑了一个轻量 TUI
过滤测试，覆盖 Goal、Plan Mode 和 compact-running 时的用户输入排队行为。

命令：

```bash
CARGO_INCREMENTAL=0 just test -p codex-tui -E 'test(goal_slash_command_emits_set_goal_event) or test(plan_implementation_popup_shows_after_proposed_plan_output) or test(submit_user_message_queues_while_compaction_turn_is_running)'
```

结果：

- 5 tests run
- 5 passed
- 2785 skipped

实际命中的测试：

- `goal_slash_command_emits_set_goal_event`
- `queued_goal_slash_command_emits_set_goal_event_after_thread_starts`
- `restored_queued_goal_slash_command_emits_set_goal_event`
- `plan_implementation_popup_shows_after_proposed_plan_output`
- `submit_user_message_queues_while_compaction_turn_is_running`

覆盖意义：

- `/goal` slash 命令仍会发出 `SetThreadGoalObjective`，没有被 Astral 改造破坏。
- queued/restored queued goal 路径仍能在线程启动后正确落到目标 thread。
- Plan Mode 的 `<proposed_plan>` / plan item 输出后，TUI 仍能展示执行计划弹窗。
- compact turn 运行中提交用户输入时，TUI 会先按当前机制尝试 steer，遇到“compact turn 不可 steer”后回退到排队，
  保持 Codex 原有 local compact 边界行为。

磁盘：

- 本次 TUI 窄测需要重编 `codex-core` / `codex-tui` / app-server 相关链路，用时约 6 分钟。
- 使用 `CARGO_INCREMENTAL=0`，`codex-rs/target/debug/incremental` 仍为 0B。
- 测试后磁盘剩余约 13Gi；目前不再继续跑 TUI/core 大测试，避免挤爆磁盘。

当前结论：

- Goal Mode 和 Plan Mode 的 TUI 基础路径有当前测试证据证明仍继承可用。
- local compact 的 TUI 排队/steer 边界也有当前测试证据。
- 剩余 compact blocker 仍是 core integration fixture 迁移到 provider-neutral mock，而不是 TUI compact 交互坏掉。

## 最新补充 54：provider-neutral compact core fixture 已补齐 chat-completions 路径

为了验证 compact 不是只在旧 `/v1/responses` fixture 下自洽，本轮在 `codex-core` compact integration suite 中新增了一条
OpenAI-compatible `/v1/chat/completions` wire path 测试：

- 新增 chat-completions mock provider：`wire_api = "chat_completions"`。
- mock SSE 走标准 chat-completions chunk 形状：`choices[].delta.content` + `finish_reason` + `usage`。
- 测试流程仍是 compact 的核心三段：
  1. 第一轮用户输入，模型返回普通 assistant 文本。
  2. 手动 `/compact`，第二次请求带 `SUMMARIZATION_PROMPT`，模型返回摘要。
  3. compact 后继续用户输入，第三次请求体必须带原始用户消息、摘要前缀消息和新用户消息。

测试断言：

- 第一次 chat-completions 请求包含原始用户消息。
- 第二次 compact 请求包含 `SUMMARIZATION_PROMPT`。
- 第三次 compact 后请求包含：
  - 原始用户消息
  - `SUMMARY_PREFIX + SUMMARY_TEXT`
  - compact 后的新用户消息
- 第三次请求不再包含：
  - compact 前 assistant 输出
  - compact 触发用的 `SUMMARIZATION_PROMPT`

验证命令：

```bash
CARGO_INCREMENTAL=0 just test -p codex-core summarize_context_round_trips_through_chat_completions
```

结果：

- 1 test run
- 1 passed
- 2642 skipped

意义：

- local compact 的“无状态 API 每次请求上下文真实形状”现在已经有 provider-neutral 证据。
- 至少对国内模型最常用的 OpenAI-compatible `/v1/chat/completions`，compact 不依赖 `/responses` 请求体。
- 当前还没有新增 Anthropic Messages compact fixture；因为真实 DeepSeek `/anthropic` 工具调用 smoke 已通过，下一步优先级应放到后台任务工具和最终端到端任务闭环，而不是继续膨胀 compact 测试。

磁盘：

- 本次 `codex-core` 窄测触发一次较长重编，用时约 8 分钟编译 + 17 秒测试。
- 测试后磁盘剩余约 12Gi。
- `codex-rs/target/debug/incremental` 仍为 0B；没有可清的低风险增量缓存。
