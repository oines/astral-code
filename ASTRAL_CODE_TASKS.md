# Astral-Code 执行任务清单

生成日期：2026-07-03
代码基线：`main` @ `b5e3d35bf8`（工作区干净；file:line 引用以此为准，若已漂移以 `ASTRAL_CODE_REVIEW.md` 中的上下文描述定位）
依据：`ASTRAL_CODE_REVIEW.md`（v2，含 Claude Code 源码对照）。每个任务的完整分析、file:line 证据和 Claude Code 侧参照都在该报告中，本清单只给执行所需的最小信息。
范围：bug 修复、契约测试、上游安全收割、rename。**不含**：TUI 重构、产品化（另行规划）。

## 一口气执行模式（维护者已授权，2026-07-03）

本清单授权**全程自主执行，不停下来等用户拍板**。规则：

- 执行范围：批次 0 → 批次 1 → 批次 2 → 批次 3（F1）。按批次顺序推进，批次内小 PR 分块，每批次自行验收通过后进入下一批。
- **不在本次范围**（需用户配合，跳过并留档）：A12 的真实抓包部分（见 A12 拆分）、G1（需 DeepSeek key）、F2（条件未触发）。
- 所有原"报回/汇报后再动"的停车点改为：**按文档给定的 fallback 规则继续执行，并记入执行报告**，不阻塞。
- 遇到文档未覆盖的判断题：按全局裁决原则处理；实在拿不准的，选保守方案（跳过并记录），继续推进。
- 结束时输出一份执行报告：完成项清单（对应任务 ID）、跳过项及原因、保守处理的判断点、留给用户的遗留项。

## 执行约束（全局）

- **总目标与裁决原则**：项目的长期运行稳定性与可维护性优先。遇到清单未覆盖的判断题时按此裁决：宁可删掉半吊子的"保护"也不保留会静默单向退化的机制；宁可失败可见（日志/报错）也不静默吞掉；宁可少一个特性也不留一条没测试的状态分支。
- 每次 Rust 编辑后跑 `just fmt`；每个任务跑对应 focused tests，不跑 full suite。
- 不改变 sandbox / approval / PTY / exec-server 的安全语义；文件工具一律走 `ExecutorFileSystem` 抽象，禁止 shell out 到本地 find/rg。
- 对齐 Claude Code 行为时以 `~/project/claude-code`（还原 TS 源码）为 ground truth，不凭印象。
- 标注 **[DECISION]** 的任务涉及用户拍板，做成可配置项并保持当前默认值，不擅自改默认。
- 阶段依赖：Phase 1 的四个工作流（A/B/C/D）互相独立可并行；Phase 2 与 Phase 1 文件冲突面小，可穿插；**Phase 3 必须在 Phase 2 完成之后**。

---

## Phase 0：`ResponseItem` 一族改名（整个计划的第一个 PR，先于批次 1）

### Z1【已拍板】`ResponseItem`/`ResponseEvent`/`ResponseInputItem` 类型标识符改名
- 背景：该类型现已是 provider-neutral 的 canonical 对话历史表示，`Response*` 前缀误导读者以为耦合 OpenAI Responses API（本次审查的 agent 就曾因此误判架构）。维护者拍板：现在改，趁 v1 未发布、消费方全为内部时付清。
- 关键事实：**改类型标识符不动序列化格式**——rollout/协议落盘的是 serde tag 值和字段名。因此本任务 serde 属性、variant tag、字段名**一个不动**，纯编译期变更。
- **已知陷阱（Codex 复核发现，已验证）：enum variant 名在 `rename_all` 下本身就是序列化格式**。`RolloutItem::ResponseItem`（`protocol/src/protocol.rs:2858`，`#[serde(tag = "type", content = "payload", rename_all = "snake_case")]`）的落盘 tag `"response_item"` 由 variant 名派生——若 rename variant，必须显式加 `#[serde(rename = "response_item")]` 保住旧 tag。执行时须**全仓 audit 所有引用被改名类型的 enum variant**（另一已知处：`core/src/session/input_queue.rs:18`，需确认是否进序列化面），凡处于 `rename_all`/tag 派生下的，rename variant 时一律显式 pin 旧 tag。同理检查 ts-rs 导出名是否派生自 variant 名。
- **格式不变的验收加一道硬检查**：改名前用当前代码序列化一组代表性 `RolloutItem`（覆盖 ResponseItem variant）存为 fixture，改名后反序列化+再序列化，字节级一致；配合旧 rollout resume 测试。
- **定名（已拍板，2026-07-03）**：`ResponseItem` → `TranscriptItem`，`ResponseInputItem` → `TranscriptInputItem`，`ResponseEvent` → `ModelStreamEvent`（stream 事件不与 Item 家族同词根，与 agent-protocol 的 `AgentStreamEvent` 形成 wire-local / core 消费的对仗）。
- 步骤：
  1. 冲突扫描确认三个新名在全仓无占用；**撞名 fallback（已授权，不停车）**：Item 家族按 `TranscriptItem` → `HistoryItem` → `ConversationItem` 顺序取第一个无冲突者，Event 按 `ModelStreamEvent` → `ModelEvent` 顺序，选用了 fallback 的在执行报告中说明占用处；
  2. 纯机械 rename PR，不夹任何功能变更；落地期间冻结其它合并；
  3. schema 重新生成，diff 审查确认只有类型名变化。
- 验收：旧 rollout 文件 resume 测试通过；schema diff 仅类型名；`just fmt` + 受影响 crate 编译通过。
- 排序理由：A11 要给该类型加字段、B/C 工作流大量引用它——rename 先行，后续全部写在新名上，避免每个在途分支 rebase。

## Phase 1A：协议适配层（优先级最高，打在国产网关兼容主目标上）

> **测试基建勘误**：`AGENTS.md` 的 "Integration tests (core)" 一节（`core_test_support::responses`、`mount_sse_once`、`ev_response_created`、断言 `/responses` POST）是 Responses 时代的指引，**协议层任务的新测试不要照它写**。仓里已有 provider-neutral 对应物（`mount_chat_completions_sse_once` 一族及 Anthropic messages fixture，见 `core-test-support` 与既有 compact 的 chat-completions/anthropic fixture 用例）。新测试一律建在 chat-completions / anthropic messages mock 上；`responses` helper 只用于维护存量 Responses 路径测试。

### A1【P0】usage chunk 不得终止流
- 位置：`codex-rs/codex-api/src/agent_adapters/chat_completions.rs:208-213`、`codex-rs/codex-api/src/sse/agent.rs:549-556, 580-588`
- 问题：`finish_reason.is_some() || usage.is_some()` 即发 `MessageStop`。SiliconFlow/Fireworks 等每 chunk 带累积 usage 的网关，响应在第一个 chunk 后被静默截断。
- 修法：usage 只旁路累积（记录最新值）；`MessageStop` 仅由 `finish_reason` / `[DONE]` / 空 choices+usage（且已见 finish_reason）驱动。
- 验收：新增"每 chunk 带 usage 的完整多 chunk 流"测试；现有 `chat_stream_merges_finish_reason_with_empty_choices_usage_chunk` 保持通过。

### A2【P0】未知 SSE 输入不致命
- 位置：`codex-rs/codex-api/src/agent_adapters/anthropic.rs:181, 613, 714`
- 问题：未知 event / content block / delta 类型 hard error 杀流；`redacted_thinking`、`server_tool_use` 等真实类型会触发。
- 修法：未知 event → `Ok(None)` + warn；未知 block/delta → 跳过该 index（占位，`content_block_stop` 时不产出 item）；`redacted_thinking` 安全跳过或透传。
- 验收：新增未知事件/未知 block/`redacted_thinking` 三个容错测试，断言流继续且已知内容完整。

### A3【P0】429 重试
- 位置：`codex-rs/model-provider-info/src/lib.rs:359-365`（`retry_429: false`）、`codex-rs/codex-api/src/api_bridge.rs:80-108`、`codex-rs/protocol/src/error.rs:187`
- 问题：429 body 不匹配 OpenAI 专属形状时映射为不可重试的 `RetryLimit`，任何层都不重试，`retry-after` 被忽略。
- 修法：解析 `retry-after` header，带退避重试（HTTP 层或映射为可重试错误带 delay）。
- 验收：wiremock 测试：429+retry-after 后重试成功；429 无 retry-after 走指数退避。

### A4【P0】Anthropic max_tokens 不硬编码 + 截断可观测
- 位置：`codex-rs/core/src/client.rs:101, 524-531`（硬编码 4096）、`codex-rs/codex-api/src/sse/agent.rs:404-413`（MaxTokens 当正常结束）
- 修法：`max_tokens` 优先级：provider/模型配置 > 模型目录（model_capabilities）> 保守默认；`StopReason::MaxTokens` 发出可观测告警事件（用户可见提示输出被截断）。
- 验收：配置注入测试 + MaxTokens 告警测试。

### A5【P1】Anthropic `message_start` usage 参与 merge
- 位置：`anthropic.rs:139-148`（丢弃）、`:163-169`（只从 message_delta 取）
- 问题：旧规范 anthropic 兼容端点只在 `message_start` 报 input_tokens → 上层 token 统计为 0 → 自动 compact 永不触发。
- 修法：`MessageStart` 携带并暂存 usage，`MessageStop` 时字段级 merge（delta 优先）。
- 验收：修正 `anthropic_tests.rs:789-803`（现在断言 `input_tokens: None`，这是固化的错误行为）；新增"usage 只在 message_start"的测试。

### A6【P1】reasoning 配置映射 + thinking 块投影门控
- 位置：`anthropic.rs:64-130`（忽略 `request.reasoning`）、`:536-542`（历史 Reasoning 无条件投影为 thinking）
- 修法：`ReasoningConfig.effort` → `thinking{type:enabled, budget_tokens}` 显式映射（budget 与 max_tokens 约束校验）；thinking 未启用或 signature 缺失时，历史 Reasoning 块降级为丢弃/文本，禁止产出无 signature 的 thinking block。
- 验收：跨 provider 历史投影测试（chat-completions 会话切 Anthropic 不再产生非法请求）。

### A7【P1】`reasoning_content` 回传按 flavor 门控
- 位置：`chat_completions.rs:496-512, 369-370`
- 修法：flavor 能力表决定是否在 assistant 历史消息回传该字段（deepseek 开、generic_openai 关），提供 per-provider 覆盖开关。
- 验收：generic flavor 请求体断言无 `reasoning_content`；deepseek flavor 断言有。

### A8【P1】tool_calls 缺 index 时按 id 顺序分配
- 位置：`chat_completions.rs:929-937`（`unwrap_or(0)`）、`sse/agent.rs:210-227`（同 index 覆盖）
- 验收：无 index 双并行 tool_calls 测试，两个调用都完整保留。

### A9【P1】图片 tool result 的 user 消息统一后置
- 位置：`chat_completions.rs:524-561`
- 修法：图片拆出的 user 消息收集后统一 append 到该轮全部 tool 消息之后，避免 `tool, user, tool` 序列。
- 验收：并行 tool call 含图测试，消息序列为 `assistant(tool_calls), tool, tool, user(images)`。

### A10【P1】`anthropic-version` 允许 provider http_headers 覆盖
- 位置：`codex-rs/codex-api/src/endpoint/agent.rs:72-75`、`endpoint/session.rs:54-58`
- 验收：provider 配置自定义 anthropic-version 的请求头测试。

### A11【契约，排序提前：A1-A4 之后、A5-A10 之前】`ResponseItem` 中立化收尾 + 无损往返 property test
- **排序原因：A6 的 thinking/signature 修复必须建立在本任务的结构化字段上，否则会先基于字符串走私机制实现再返工。**
- 位置：`sse/agent.rs:367`（`"anthropic_signature:"` 前缀走私）、`core/src/agent_request.rs:37`
- 修法：signature、redacted_thinking、cache 标记等成为 `ResponseItem` 的结构化字段（或 `provider_metadata` 扩展位），删除字符串前缀编码；对每个 wire 建 property test：adapter → `ResponseItem` → adapter 恒等。
- 注意：该类型出现在 rollout 持久化中，新字段需向后兼容（serde default）。类型名以 Z1 定名后的为准（Z1 先行落地）。
- 验收：roundtrip property test 通过；旧 rollout 文件可正常 resume。

### A12【契约】provider SSE golden fixture 回放
- 建 `codex-rs/codex-api/tests/` 下的 SSE 回放基线：DeepSeek `/v1`、DeepSeek `/anthropic`、SiliconFlow、GLM、Kimi、MiniMax 各一份真实抓包 fixture（脱敏），断言解析出的 item 序列与 usage。抓包可复用 `.cache/harness-bench-runs` 的 capture proxy 基建。
- 注意：当前 `codex-api/tests/clients.rs` 只覆盖 chat completions，需补 Anthropic messages 端到端。
- **一口气执行时拆两半**：
  - **A12a（本次做）**：搭好回放测试框架 + 按各家公开文档行为构造 wiremock 合成 fixture（每家一份，文件名/注释明确标注 `synthetic`，不计入 golden 基线）。
  - **A12b（跳过留档）**：真实抓包替换合成 fixture——需要各 provider 的 API key 和网络环境，需用户配合。**禁止手工编造 fixture 冒充真实抓包**；执行报告中列出待抓包的 provider 清单。

### A13【P2】小项打包
- 两个 adapter 的 `apply_provider_body_overrides` 去重（`anthropic.rs:361-372` / `chat_completions.rs:608-619`）。
- flavor 推断结果打日志（`model_provider_info.rs:432-464`）。
- 每 chunk 带 `role:"assistant"` 不重发 `MessageStart`（`chat_completions.rs:162-170`）。
- query_params URL encode（`codex-api/src/provider.rs:61-71`）。
- anthropic_cache_fold：历史重写（compact/回滚）时 reset 状态；撞 400 时摘掉 fold 原地重试而非直接失败（`core/src/anthropic_cache_fold.rs:57-66`、`client.rs:584-590`）。

---

## Phase 1B：Session memory compact（三个"没抄全"点 + 配套）

### B1【P0】legacy compact 后重置 session memory 基线
- 位置：`codex-rs/core/src/compact.rs:252-258 → 361-380`（legacy 路径不碰 state）；参照 CC 在全部三个 legacy compact 成功点显式 `setLastSummarizedMessageId(undefined)`（`commands/compact/compact.ts:110-112, :200`、`services/compact/autoCompact.ts:294-296, 323-325`）
- 修法：legacy compact 成功后调用等价的 `record_post_compact_baseline`（清 boundary、重置 token/tool-call 基线）。
- 验收：集成测试：SM compact 失败 → legacy fallback → 后续提取与 SM compact 正常工作（整条链）。

### B2【P0】等待运行中提取改为轮询 + 超时容忍
- 位置：`codex-rs/core/src/session_memory.rs:482-506`（一次性 sleep 15s，超时判失败）；参照 CC 1s 轮询、15s 上限、超时/stale 后照样继续（`sessionMemoryUtils.ts:12, 89-105`）
- 修法：复用 `wait_for_extraction_completion`（`:508-538`）的轮询实现；超时不再返回 Err，继续 SM compact。进程内提取直接 await 任务句柄而非文件标记。
- 验收：提取 2s 完成 → compact 等待 <3s；提取超时 → compact 仍走 SM 路径。

### B3【P0，已定稿】删除 SM compact 层熔断，失败降级为可观测信号
- 位置：`session_memory.rs:48, 286-320`；参照 CC：SM 失败零成本静默回退不计数（`sessionMemoryCompact.ts:621-629`）
- 决策依据（维护者已确认按此执行）：SM compact 失败在 B2 修复后是零成本事件（纯内存计算 + 回退 legacy 兜底），熔断只有下行没有上行，且"打开后阻止尝试、清零又依赖尝试成功"是自锁结构，还持久化在 state.json 里重启不恢复——违反"不留静默单向退化机制"原则。
- 修法：删除 SM 层熔断（含 state.json 中的失败计数持久化，注意旧 state.json 反序列化兼容）；失败改为 warn 日志 + metrics 计数，保留观测性但不触发自动关闭。
- 验收：连续多次 SM 失败后，下一次 auto compact 仍尝试 SM 路径；失败有日志可查；熔断相关旧测试删除或改写。

### B3b【P2，单独 PR】autocompact 层 thrashing 保护
- 背景：`ASTRAL_CODE_PROGRESS.md` 中"实现或继承类似 `rapid_refill_breaker` 保护语义"的 TODO 的正确落点。防的是 compact 短时间内反复触发（context 已不可挽救仍在烧 token），与 SM 熔断是两码事。
- 参照 CC：autocompact 整体层、内存态、只统计 legacy compact 失败、成功清零、3 次上限（`autoCompact.ts:70, 332-349`）；触发时明确停止并提示用户，而非静默。
- 约束：**不要和 B1-B3 的状态生命周期修复混在同一个 PR**；不紧急，可放到 Phase 1B 收尾后做。

### B4【P1】state.json / summary.md 原子写
- 位置：`sidechain.rs:306/349`、`session_memory.rs:160/184`（裸 `tokio::fs::write`）；参照 CC tempfile+rename（`utils/file.ts:84-98, 423-438`）
- 修法：tempfile + rename；state.json 读坏时的 warn 路径保留。
- 验收：单测覆盖写入中断不留半截文件（用注入失败模拟）。

### B5【P1】mid-turn compact 恢复 initial context 注入
- 位置：`session_memory.rs:327, 360`（丢弃 `_initial_context_injection`，违反 `compact.rs:57-66` 自家注释的不变量）
- 修法：per 注释要求走 `BeforeLastUserMessage` 注入。
- 验收：新增 mid-turn auto compact 集成测试，断言当前 turn 剩余请求含 user_instructions/environment_context。

### B6【P2，已拍板】提取阈值做成可配置，默认值不变
- 位置：`session_memory.rs:42-44`（100k/20k/10）
- **维护者已确认：这是有意的自定保守值（压低提取频率），不是抄错，不向 CC 默认（10k/5k/3）靠拢。**
- 修法：三个阈值做成 config 项，默认值保持 100k/20k/10，便于后续按真实会话数据调参。
- 验收：配置注入测试。

### B7【P1】测试缺口补齐
- 补：进程崩溃/Ctrl-C 恢复（部分 Edit、残留 started 标记）、resume/fork 交互、多进程并发提取防护（`RUNNING_EXTRACTIONS` 进程级 + shutdown 清别人标记 `session_memory.rs:526-532`）。
- 恢复被 869e63deb5 删除的"失败提取回滚 summary"集成测试。

### B8【前瞻，低优先】/compact 自定义指令
- astral 目前无指令入口；将来加时必须带 CC 的跳过逻辑：有自定义指令则跳过 SM compact 走 legacy（`commands/compact/compact.ts:52-57`）。本轮只在代码注释/文档记录此约束。

---

## Phase 1C：工具 flavor 层

### C1【P0】Grep files_with_matches 模式修 multiline
- 位置：`codex-rs/exec-server/src/search.rs:353-360`（漏 `.multi_line(request.multiline)`，对比 `:366`）
- 验收：三种 output_mode 各一个 multiline 测试。

### C2【P0】Read 补 CC 双护栏
- 位置：`codex-rs/core/src/tools/handlers/astral_file_tools.rs:381-423`
- 参照 CC：limit 未指定时整文件 >256KB 预读抛错（文案：`File content (X) exceeds maximum allowed size (Y). Use offset and limit parameters...`，`utils/readFileInRange.ts:63`）；输出 >25K tokens 抛错（env 可覆盖，`tools/FileReadTool/limits.ts:18`）。
- 注意：**无单行截断**（CC 也没有）；保留 astral 的默认 2000 行（比 CC 实现更好）。
- 验收：两道护栏各一个测试，错误文案与 CC 一致。

### C3【P0】Write/Edit 自动创建父目录
- 位置：`astral_file_tools.rs:1176-1190` → `exec-server/src/local_file_system.rs:364-372`；参照 CC `FileWriteTool.ts:254`、`FileEditTool.ts:430`
- 修法：写盘前 `create_dir_all(parent)`，走 `ExecutorFileSystem`（远程实现同步支持）。
- 验收：写不存在的嵌套路径成功。

### C4【P0】Glob 后端性能：剪枝 + 扫描上限
- 位置：`exec-server/src/search.rs:92-153, 74-89`（全量收集后排序；Override whitelist 不剪枝目录）
- 修法：pattern 静态目录前缀做目录级剪枝；加扫描条目/时间上限，超限返回引导性错误（"path 太宽，换更具体的 path/pattern"）。**语义不动**：`--no-ignore --hidden`、mtime 升序、take(100)、截断文案均与 CC 一致，保持。
- 验收：大树（含深层 node_modules）下 `Glob("*.rs", path=<root>)` 在上限内返回；语义快照测试不变。

### C5【P1】read-state key 用 resolved environment_id
- 位置：`astral_file_tools.rs:391/816/901`（用 `args.environment_id`）vs `:231`（resolved）
- 验收：多环境测试："Read 省略 id → Edit 显式传默认 id"不再报 "File has not been read yet"。

### C6【P1】CoreToolCall 补 Interrupted 终态
- 位置：`core_tool_lifecycle.rs`（只发三态）；abort 路径在 `session/handlers.rs:628` 附近
- 修法：turn abort 时为 in-flight core tool item 统一 flush `Interrupted`。
- 验收：interrupt 集成测试，item 流无永久 InProgress。

### C7【P1】ShellCommand 回退模式的 Bash 描述动态生成
- 位置：`spec_plan.rs:703-708`、`astral_bash.rs:222`（静默丢 run_in_background）、`astral_prompts.rs:21-26`（静态描述）
- 修法：按 backend 分支裁剪 bash_description；不支持时 `run_in_background` 报参数错误而非静默忽略。
- 验收：两种 backend 的描述快照测试。

### C8【P1】小项打包
- Read `limit=0` 拒绝（CC schema `positive()`；`astral_file_tools.rs:388-389`）。
- Read CRLF→LF 归一 + BOM 剥离（CC `readFileInRange.ts:29,138,164-167`）。
- Read 文件不存在补 "Did you mean X?"（CC `FileReadTool.ts:639-647`，可选）。
- 非 multiline pattern 含 `\n`：保留报错，但文案加 "set multiline: true" 提示（`search.rs:186-195`）。**[DECISION]** 若要 1:1 复刻 CC 则改静默空结果——默认保留报错。
- `FileReadStateStore` 改存 hash+mtime（`astral_file_tools.rs:119-136`，防长会话内存增长）。
- `resolve_path` 不 trim（`:1301-1303`）；`~` 展开移到 executor 侧（远程环境语义，`utils/absolute-path/src/lib.rs:45-56`）。

### C9【契约】Claude Code 行为 golden 对照测试
- 新建一个集中测试文件，固化本次对照确认的忠实点：行号 `{n}\t` 格式、Glob mtime 升序+take(100)+截断文案、`--no-ignore --hidden` 默认、相对路径规则、`[Omitted long matching line]`、空文件/offset 超界 reminder 文案、tail 10k/5/40k 参数。目的：防后续重构无意破坏训练分布契约。

### C10【P2，已拍板】Bash 前台 yield 保留 10s，只做文案对齐 + 配置通道
- 位置：`unified_exec.rs:64-66`（10s）
- **维护者已确认：有意保留 Codex UnifiedExec 语义（terminal 持续观察是项目核心优势），不向 CC 的 120s 阻塞靠拢。禁止"顺手对齐 CC"改默认。**
- 修法：yield 时长加配置通道（默认 10s 不变）；后台任务返回文案对齐 CC（`Command running in background with ID: ...`）。

---

## Phase 1D：控制面扫尾（半天到一天的量）

### D1 doctor 去掉 api.openai.com fallback
- 位置：`codex-rs/cli/src/doctor.rs:2339, 2349`（另见 `:3370`）
- 修法：provider 无 base_url 时报"未配置 ASTRAL_BASE_URL"，不探测任何默认 endpoint。

### D2 announcement fetch 加门禁
- 位置：`codex-rs/tui/src/tooltips.rs:6, 151-163`、`tui/src/lib.rs:1319`
- 修法：挂到 `check_for_update_on_startup` 同一 gate。**[DECISION]** 该 gate 默认值（现为 true）是否改 false 属产品决策，本任务不改默认。

### D3 死代码删除
- `codex-rs/chatgpt/` 空目录；`app-server-transport` 的 remote_control 目录（含 `wham/remote/control/*` 路径拼接）；`connectors` crate 的 directory HTTP 助手；`backend-client` 的 wham/HostedApi 路径风格（若 cloud-tasks-client 仍需则注明留作自建 backend 兼容）；`feedback` 的 sentry 依赖（`feedback/Cargo.toml:14`）；`has_chatgpt_account`（`tui/src/app_server_session.rs:154`）、`Product::Chatgpt`/`SessionSource "chatgpt"`（`protocol/src/protocol.rs:3390-3409`）、`ChatGptAccountEntry`、`RemoteSkillProductSurface::Chatgpt` 等死枚举。
- 注意：`thread_resume_redaction.rs:11` 的 `codex_chatgpt_*` redaction 名单是旧 rollout 数据兼容，**保留**。

### D4 `otel.exporter = "statsig"` 显式告警
- 位置：`otel/src/config.rs:9-17`（静默映射 None）、`core/config.schema.json:2177`
- 修法：config 加载层 warn；从 schema/枚举移除该值（配 deprecation 提示）。

---

## Phase 2：上游安全收割（必须在 Phase 3 之前）

上游 `~/project/codex` 已更新到 `da4c8ca57d`（2026-07-03 fetch）。fork 点 `08cb633c06` 之后，安全骨架路径上共 **78 个 commit**，范围：
`codex-rs/linux-sandbox`、`codex-rs/windows-sandbox-rs`、`codex-rs/exec-server`、`codex-rs/execpolicy`、`codex-rs/core/src/{seatbelt,landlock,spawn,exec*,unified_exec}`、`codex-rs/utils/pty`。

### E1 分类
- 逐个把 78 个 commit 分为：(a) 安全修复（必收）、(b) 正确性修复（择收）、(c) 功能/重构/遥测（不收）。产出带优先级的 cherry-pick 清单文档。
- 已知必看项：`27f22b54ae`（PowerShell AST execpolicy 加固）、`0ccb676dd0`（quick-xml 安全公告）、`afbb69a2fb`/`c0b36d234a`/`4907f0c2c3`/`3ccef20ef4`（Windows sandbox ACL/凭证系列）、`bd2968a4db`/`9c3b10e5d4`（remote exec-server sandbox intent 传递）、`a781761eda`（Windows ConPTY 输入）、`96d8e34712`（文件系统遍历跟随目录 symlink——注意与 C4 的 Glob 改动协调）、`964b138c3d`（exec-server 掉线恢复）。

### E2 逐个 cherry-pick
- 按清单顺序 pick，冲突处小心保留 Astral 侧的命名/协议差异；每个 pick 跑对应 crate 的 focused tests。
- 遥测/tracing 相关 hunk 若混在安全修复 commit 里，剥离遥测部分（Astral 的 OTEL 默认 none 语义不变）。
- **分类拿不准的 commit（一口气模式）**：保守跳过，列入执行报告的"待复核清单"（带一句跳过理由），不停车询问。

### E3 同步政策（已预决策，2026-07-03）
- **短期政策已定：继续收割上游安全修复**（因此 F2 条件不触发，本次跳过）。把该政策与"F2 三个触发条件"（上游骨架修复节奏放缓 / 决定自有 sandbox 层 / v1 定版复评）写入 `ASTRAL_CODE_PROGRESS.md` 存档即可，无需再等用户。

---

## Phase 3：Rename（依赖 E3 的政策决定）

### F1 用户可见面 rename（无论政策如何都做）
- `CODEX_SANDBOX` / `CODEX_SANDBOX_NETWORK_DISABLED` → `ASTRAL_*`（`core/src/spawn.rs:20,25`、`login/src/auth/default_client.rs:250`）：**双写一个版本**（新旧同时注入/读取），并在文档标注旧名 deprecated。
  - **AGENTS.md 冲突豁免（维护者授权）**：`AGENTS.md` 中 "Never add or modify any code related to `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` or `CODEX_SANDBOX_ENV_VAR`" 一条在本任务范围内豁免。该规则的本意（不破坏测试的 sandbox 检测 early-exit 逻辑）仍然有效：双写期间旧名必须继续被设置和识别，所有依赖这两个 var 的测试 early-exit 行为不得改变。本任务之外该规则照常生效。
- `codex-feedback-*.log` 文件名（`feedback/src/lib.rs:407`）。
- `find_codex_home` 兼容别名清理（`utils/home-dir/src/lib.rs:21`）——确认无调用后删除。
- 注意：app-server 读 repo 内 `.codex/config.toml` 是 external-agent 互操作，**有意行为，保留**。

### F2 内部 crate/模块 rename（仅当 E3 决定与上游分道扬镳）
- `codex-rs/` 目录与 `codex-*` crate 名 → `astral-*`。一次性大 diff，单独 PR，期间冻结其它合并。
- 若 E3 决定继续跟上游：**跳过本任务**，只做 F1。

### F3 不做的 rename（显式记录，防止顺手改掉）
- JWT claim 里的 `chatgpt_user_id` 等历史 token payload 形状（progress 文档已有此约束）。
- serde tag 值与字段名（wire/磁盘格式）：Z1 只改类型标识符，格式面的改名（若有）推迟到 "astral v1 协议" 定版。
- 内部 crate 名（F2）：维持"挂触发条件"策略——上游骨架修复节奏放缓、或决定自有 sandbox 层、或 v1 定版复评，三者满足其一再评估；短期政策为继续收割上游，不做 crate rename。

---

## 独立任务：prompt 改动跟进（改动已合入：`fa4d5fece3`，PR #29）

### G1 models-manager 系统提示词回归与补强
- 背景：分节式 → 行为准则式短 prompt 的改动已合入 main。以下为跟进项：
  1. 用 `.cache/harness-bench-runs` 的 fair bench 基建跑一轮新旧 prompt 对比，重点确认 `20-heartbeat-escalation` 类长任务的 persistence 不回退。**方法约束：新旧 prompt 两个变体必须跑在同一个 build 上**（旧 prompt 用配置覆盖注入），不得拿 6-14 的历史数据当基线——否则 harness 修复的影响会混进 prompt 对比。环境依赖：需要 DeepSeek API key，需用户配合；
  2. 若回退（或不跑 bench 直接补强）：补一句明确的 persistence 条款（原 "keep working until the user's task is genuinely handled" 被删，对 DeepSeek 类模型的 agentic 持续性有实际作用）；
  3. C7 完成后复查：工具描述动态生成是否接住了被删的 Native Tool Flavor 段职责，不足则在 prompt 补最小兜底。

---

## 决策项状态（[DECISION]，Codex 不得自行决定默认值）

| # | 决策 | 现状 | CC 参照 | 状态 | 涉及任务 |
| --- | --- | --- | --- | --- | --- |
| 1 | Bash 前台 yield 时长默认值 | 10s | 120s 阻塞+超时 auto-background | **已拍板：保留 10s（有意分叉）** | C10 |
| 2 | SM 提取阈值默认值 | 100k/20k/10 | 源码默认 10k/5k/3 | **已拍板：保留（有意的自定保守值）** | B6 |
| 3 | 非 multiline pattern 含 `\n` | 报错（更友好） | 静默空结果（更忠实） | 开放，默认保留报错+加提示 | C8 |
| 4 | announcement/update check 默认开关 | 默认开 | — | 开放，属产品决策 | D2 |
| 5 | 上游同步政策（决定 rename 范围） | 未定 | — | **已预决策：短期继续收割，F2 挂触发条件不做** | E3/F2 |

## 与 Claude Code 的分叉意图登记（执行时的护栏）

完整登记见 `ASTRAL_CODE_REVIEW.md` 第九节。要点：**下列分叉是受保护的有意设计，任何任务执行中不得"顺手对齐 CC"**——Bash 10s yield 与 UnifiedExec 后台任务模型、后台任务四件套及 `task_id` 错误语义、subagent 工具保持 Codex 原版、`AskUserQuestion` 原生 UI、SM 提取阈值 100k/20k/10、SM state.json 持久化（但其原子写/多进程防护欠账要补，见 B4/B7）、Glob/Grep 的 `--no-ignore --hidden` 语义。反之，"不小心没抄对"清单（Grep multiline、父目录、Read 护栏、B1/B2 等）修复无争议，放手改。

## 建议的执行批次

0. **批次 0（单独先行，时间盒）**：Z1 rename（定名 → 机械改名 → 验收），期间不并行其它任务。**时间盒：冲突扫描完成后若定名决策悬置超过一个工作日，先开批次 1，Z1 改为批次 1 落地后的独立窗口执行，接受 rebase 成本——改名不得无限期阻塞 P0 修复。**
1. **批次 1（并行）**：Phase 1A（顺序：A1-A4 → A11 → A5-A10、A13）、Phase 1B（B1-B3 先行）、Phase 1C（C1-C4 先行）、Phase 1D 全部。
2. **批次 2**：Phase 1 收尾（契约测试 A12/C9 必须在批次 3 前完成；A11 已提前到批次 1）+ Phase 2（E1 分类可与批次 1 并行做）。
3. **批次 3**：Phase 3 rename（F1 必做；F2 视 E3 决定）。
4. **随时**：G1 的 bench 回归可独立先跑；第 3 小项在 C7 完成后做。
