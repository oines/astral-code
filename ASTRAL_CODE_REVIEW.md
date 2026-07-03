# Astral-Code 实现审查报告（v2，含 Claude Code 源码对照）

审查日期：2026-07-03
审查基线：`main` @ `b5e3d35bf8`（审查开展时为 `2590a75ee3` + 工作区未提交的 models-manager prompt 改动；该改动其后已合入为 `fa4d5fece3`，PR #29，见第六节）
对照上游：`openai/codex`（fork 点 `08cb633c06`，2026-06-08；此后本仓 315 个 commit、1313 个文件变更，+57.8k/-76.6k 行）
对照参考：`~/project/claude-code`（Claude Code 还原 TS 源码）——工具层与 session memory compact 的每条发现都已逐项对照，标注判定：**【真 bug】**（CC 行为不同）、**【忠实模仿】**（与 CC 一致，是 feature 不是 bug）、**【自有设计】**（CC 无对应机制）。

审查方法：以 `ASTRAL_CODE_PROGRESS.md` 声明的项目方向为验收标准，对四个核心区域做并行深度审查（协议适配层、工具 flavor 层、控制面拆除完整性、session memory compact），随后用 Claude Code 源码逐项复核"疑似偏差"类发现。v1 中有多条被复核推翻，见第八节撤回清单。

---

## 一、方向确认与 diff 概览

项目方向（与 handoff 文档一致，代码实际状态吻合）：

- **继承** Codex 的执行骨架：PTY/UnifiedExec/exec-server、sandbox、approval、app-server、MCP、multi-agent、Plan/Goal Mode。
- **重做** 模型协议层：provider-neutral Agent IR（`codex-rs/agent-protocol`）+ Anthropic Messages / OpenAI-compatible chat completions 双 adapter，OpenAI Responses 降级为非核心。
- **重做** 模型可见工具面：Claude-ish flavor（`Read/Write/Edit/Glob/Grep/TodoWrite/Bash/后台任务四件套/AskUserQuestion`），重点服务 DeepSeek 等国产模型的 SFT 轨迹。
- **拆除** OpenAI/ChatGPT 控制面：登录、遥测、feedback、cloud-tasks、remote plugins/skills、connector directory，命名空间收敛为 `~/.astral-code` / `ASTRAL_*` / `hosted_base_url`。

## 二、总体评价

**架构方向执行得很好，四个区域都已过"能跑通"线。经 Claude Code 源码对照后，工具层的复刻忠实度比 v1 判断的高得多**（行号格式、Glob 排序/hidden 语义、相对路径、freshness 检查、超长行占位、count 语义、tail 选取参数乃至错误文案都是逐字对齐的）；**当前真正的风险集中在协议层对杂牌网关的容错性，以及 session memory compact 少数几个"没抄全"的状态生命周期点**。

| 区域 | 状态 | 核心问题 |
| --- | --- | --- |
| 协议适配层 | IR 设计干净，happy path 可用 | 流终止条件过激、未知输入零容错、429 不重试——恰好打在"国产网关兼容"主目标上 |
| 工具 flavor 层 | 复刻忠实度高，全部走 `ExecutorFileSystem` 抽象 | 少数真 bug：Grep multiline 在默认模式失效、Write/Edit 不建父目录、Read 缺 CC 的 token/字节护栏、Glob 后端性能（非语义）问题 |
| Session memory compact | 设计与参数大量 1:1 复刻 CC | 三个"没抄全"点：legacy compact 后不重置 boundary（CC 有三处显式重置）、等待策略走样、熔断移植错层 |
| 控制面拆除 | **约 95% 完成，质量高** | 剩 doctor 的 api.openai.com fallback、TUI 无门禁外联 GitHub、一批死代码 |

---

## 三、P0：会丢数据、截断响应或让会话退化的问题

### 3.1 协议层（与 CC 对照无关，独立成立）

1. **任意带非 null `usage` 的 chunk 会提前终止 chat-completions 流。**
   `codex-rs/codex-api/src/agent_adapters/chat_completions.rs:208-213` 中 `finish_reason.is_some() || usage.is_some()` 即 push `MessageStop`；`codex-rs/codex-api/src/sse/agent.rs:549-556, 580-588` 收到后立即 Completed 并退出。SiliconFlow、Fireworks 等每 chunk 带累积 usage 的网关，响应会在第一个 chunk 后被**静默截断成"完整"结果**。
   修法：usage 只做旁路累积；`MessageStop` 仅由 `finish_reason` / `[DONE]` / 空 choices+usage（且已见 finish_reason）驱动。补"每 chunk 带 usage"的流测试。

2. **Anthropic 未知 SSE 事件 / content block / delta 一律 hard error 杀流。**
   `codex-rs/codex-api/src/agent_adapters/anthropic.rs:181, 613, 714`。Anthropic 规范明确要求忽略未知事件；现实触发点：开 thinking + tools 会收到 `redacted_thinking` → 整个 turn 报错；Kimi/GLM/MiniMax 的 anthropic 兼容端点容错为零。
   修法：未知 event → warn + skip；未知 block/delta → 跳过该 index。确立"未知输入不致命"的解析原则。

3. **429 在任何层都不重试，`retry-after` 被忽略。**
   `codex-rs/model-provider-info/src/lib.rs:359-365`（`retry_429: false`）+ `codex-rs/codex-api/src/api_bridge.rs:80-108`（429 body 不匹配 OpenAI 专属形状时映射为不可重试的 `RetryLimit`）。Anthropic 短时限流非常常见，现在会直接让 turn 用户可见地失败。
   修法：解析 `retry-after` 做带退避重试，或映射为 `ApiError::Retryable{delay}`。

4. **Anthropic `max_tokens` 硬编码 4096，且截断被当正常结束。**
   `codex-rs/core/src/client.rs:101, 524-531`；`codex-rs/codex-api/src/sse/agent.rs:404-413` 把 `StopReason::MaxTokens` 当 `end_turn` 无告警。coding agent 写大 patch 时输出被静默砍断。
   修法：`max_tokens` 来自模型目录/provider 配置；`MaxTokens` 停止至少发可观测信号。

### 3.2 Session memory compact

5. **【真 bug】legacy compact 后不重置 session memory 状态，单向退化 + 死锁。**
   CC 在**全部三个** legacy compact 成功点都显式 `setLastSummarizedMessageId(undefined)`（`commands/compact/compact.ts:110-112, :200`、`services/compact/autoCompact.ts:294-296, 323-325`），之后 SM compact 走 resumed-session 分支照常工作。astral 的 legacy 回退路径（`codex-rs/core/src/compact.rs:252-258 → 361-380`）不碰 state.json → 陈旧 `last_summary_index/fingerprint` 必然 mismatch（`tail.rs:83-94`）→ 每次 auto 失败给熔断 +1（`session_memory.rs:310-313`）→ 熔断打开后只有 compact 成功才清零，而成功又依赖 boundary 先修复——死锁。
   修法：legacy compact 完成后同样调用 `record_post_compact_baseline`（对齐 CC）；提取成功也重置熔断计数。

6. **【真 bug】compact 等待运行中提取的策略走样，且失败语义相反。**
   CC 是 **1 秒间隔轮询、最多 15s**，且超时/过期（>60s stale）后**照样继续 SM compact**（`services/SessionMemory/sessionMemoryUtils.ts:12, 89-105`）。astral 是一次性 `sleep(15s)` 再检查一次，仍在跑就返回 Err → 回退 legacy 并计入熔断（`session_memory.rs:482-506`）。两个后果：提取 2s 完成也白等满 15s（compact 卡顿 13s）；提取 16s 完成则白白 fallback 且喂熔断。
   修法：改 1s 轮询（复用 `wait_for_extraction_completion` 的实现，`:508-538`），超时容忍继续而非判失败。

7. **【未经决策，已确认需修】熔断移植错了层。**
   CC 的 SM compact 失败是**零成本静默回退**（catch → return null，`sessionMemoryCompact.ts:621-629`），不计数无熔断；CC 的 3 次熔断在 **autocompact 整体层**（`autoCompact.ts:70`），内存态、只统计 legacy compact 失败、成功即清零。astral 把熔断装在 SM compact 失败上并**持久化到 state.json**（`session_memory.rs:48, 286-320`）——SM 失败本是零成本事件，却会永久性关闭 SM compact；叠加 #5 的 stale boundary 才形成死锁倾向。**维护者确认这块由 Codex agent 实现、未经专门决策**（见第九节登记）。
   修法：SM compact 失败不进熔断（回退 legacy 本身就是兜底）；防 compact thrashing 的保护（progress 文档中 `rapid_refill_breaker` TODO）应在 autocompact 整体层实现，与 CC 同层。

### 3.3 工具层

8. **【真 bug】Grep 默认输出模式下 `multiline: true` 完全失效。**
   CC 底层是 ripgrep 二进制，`-U --multiline-dotall` 无条件先于 output-mode flags 加入（`tools/GrepTool/GrepTool.ts:340-343`），三种模式都生效。astral 自实现中 `files_with_matches_searcher()` 漏掉 `.multi_line(request.multiline)`（`codex-rs/exec-server/src/search.rs:353-360`，对比 `:366` 的 count/content 路径）。跨行 pattern 在默认模式返回 "No files found"，换 content 模式又能找到。
   修法：给 files_with_matches 的 Searcher 也设 multi_line，补测试。

9. **【实现性能问题，语义忠实】Glob 无扫描上限、不剪枝，大目录会拖死会话。**
   语义上 astral 与 CC 一致（都 `--no-ignore --hidden`、含 `.git`，连 env 开关名 `CLAUDE_CODE_GLOB_NO_IGNORE`/`CLAUDE_CODE_GLOB_HIDDEN` 都相同：`utils/glob.ts:95-107` vs `search.rs:542-547`）——v1 判的"hidden=true 是偏离"撤回。真正的差异是**实现**：CC 靠 rg 二进制流式扫描，快；astral 用 `ignore` crate 的 Override whitelist（对目录不剪枝）把**所有** candidate 收进内存排序后才 take(100)（`search.rs:92-153, 74-89`），`Glob("*.rs", path=~)` 依然全量遍历 home。早期 commit c82843b174 的噪声目录剪枝在 e083d7cc12 重构时被删。
   修法：pattern 静态目录前缀剪枝 + 扫描条目/时间上限（超限返回引导性提示）。语义保持 CC 对齐不动。

10. **【真 bug】Read 缺 CC 的两道体量护栏。**
    CC 的护栏（v1 描述有误，已修正）：**无单行字符截断**；limit 未指定时整文件 >256KB 预读即抛错（`utils/file.ts:48`、`utils/readFileInRange.ts:63, 95-101`，文案引导 offset/limit）；输出 >25K tokens 抛错（`tools/FileReadTool/limits.ts:18`，env 可覆盖）。astral 只有 fs 层 512MB 硬顶（`local_file_system.rs:26`），一个大文件/超长行会直接冲进 tool result，只靠 history 层通用截断兜底（不可解释、不引导重试）。顺带：astral 真正实现了"默认 2000 行"（`astral_file_tools.rs:67,388`），CC 只在 prompt 里这么写、实现是读整文件——这一点 astral 反而更好，保留。
    修法：补 256KB 文件级 + 25K token 输出级护栏和 CC 同款错误文案。

11. **【真 bug】Write/Edit 不自动创建父目录。**
    CC 两个工具写盘前都 `mkdir` 递归建目录（`FileWriteTool.ts:254`、`FileEditTool.ts:430`）。astral 直接 `tokio::fs::write`（`astral_file_tools.rs:1176-1190` → `local_file_system.rs:364-372`），父目录不存在报裸 NotFound。模型按 CC 习惯直接写新路径会失败。
    修法：写盘前 `create_dir_all(parent)`。

---

## 四、P1：兼容性与正确性问题

### 协议层

- **Anthropic `message_start` 的 usage 被丢弃**（`anthropic.rs:139-148`），input token 统计全靠 `message_delta`。旧规范实现的 anthropic 兼容端点（DeepSeek `/anthropic`、GLM、Kimi）只在 `message_start` 报 input → `input_tokens≈0` → **自动 compact 永不触发**，最终撞 context 硬错误。`anthropic_tests.rs:789-803` 把这个缺口固化成了断言。
- **Anthropic adapter 完全忽略 `reasoning` 配置**（`anthropic.rs:64-130`），thinking 只能靠 `request_body` 手工注入；同时历史 `Reasoning` 块无条件投影为 `thinking` block（`:536-542`），跨 provider 切换会产生无 signature 的 thinking block → 必 400。
- **`reasoning_content` 回传不按 flavor 门控**（`chat_completions.rs:496-512, 369-370`），对 generic_openai 也塞该非标字段，部分 provider/严格网关会 400。应由 flavor 能力表决定。
- **tool_calls 缺 `index` 时全部塌缩到 index 0 互相覆盖**（`chat_completions.rs:929-937` + `sse/agent.rs:210-227`），Mistral 风格 provider 的并行调用会丢失/串参。无 index 时按 id 出现顺序分配。
- **图片 tool result 拆出的 user 消息插在 tool 消息序列中间**（`chat_completions.rs:524-561`），产出 `tool, user, tool` 序列，vLLM 等严格实现会 400。应统一 append 到该轮全部 tool 消息之后。
- **`anthropic-version` 用户覆盖失效**：`endpoint/agent.rs:72-75` 只查 `options.extra_headers`，provider `http_headers` 被默认值顶掉。

### 工具层

- **【自有 bug】read-state key 用原始 `args.environment_id` 而非 resolved id**（`astral_file_tools.rs:391/816/901` vs `:231`），多环境下"Read 省略 id → Edit 显式传默认 id"会报 "File has not been read yet"。这是 astral 的多环境机制，与 CC 无关，统一用 resolved id 即可。（注：freshness 检查本体经对照是忠实模仿——CC 同样以 mtime 为主、内容 fallback 同样只在 full-read state 生效、`touch` 后同样会误报，见第八节撤回清单；astral 的 canonicalize key 和"mtime 相同内容不同也报错"属于合理增强。）
- **【自有 bug】`CoreToolCallStatus::Interrupted` 从不发出**：`core_tool_lifecycle.rs` 只发三态，turn abort 时 in-flight item 永久 InProgress（协议和 TUI 都已支持 Interrupted，唯独 core 不产生）。
- **【有意分叉，已拍板保留】Bash 前台 10 秒即 yield。**
  CC 前台**阻塞直到命令完成**，默认 timeout 120s / 上限 600s（`utils/timeouts.ts:2-3`），超时才 auto-background 或 SIGTERM（`BashTool.tsx:965-983`）。astral 前台 10s 即 yield 成后台 task（`unified_exec.rs:64-66`）。**维护者确认有意保留 UnifiedExec 语义**（见第九节登记），不向 CC 靠拢。剩余小项：后台返回文案可对齐 CC 的 `Command running in background with ID: ...`；yield 时长可开配置通道（默认不变）。
- **【真 bug（小）】Read `limit=0` 被接受并返回空串**：CC schema 用 `positive()` 直接拒绝（`FileReadTool.ts:233`）；astral 接受 0（`astral_file_tools.rs:388-389`）。按输入校验错误处理。
- **【设计选择，需拍板】非 multiline pattern 含 `\n`**：CC 把 rg 的 usage error 静默吞成 "No matches found"（`utils/ripgrep.ts:376-380, 437-442`）；astral 返回构建错误（`search.rs:186-195`）。astral 更友好但不忠实。建议：保留报错但在文案中提示 "set multiline: true"。
- **【真 bug】ShellCommand 回退模式下 Bash 静态描述承诺兑现不了**：`spec_plan.rs:703-708` 不注册后台任务四件套、`astral_bash.rs:222` 静默丢弃 `run_in_background`，但 `bash_description()` 仍在教模型用它们。描述应随 backend 动态生成。
- **【小偏差】Read 不做 CRLF→LF 归一 / BOM 剥离**：CC `readFileInRange.ts:29,138,164-167` 会做；astral `split_inclusive('\n')` 保留 `\r`，CRLF 文件的行号输出带 `\r`，可能影响模型 Edit 匹配。
- **【可选补齐】Read 文件不存在时 CC 有 "Did you mean X?" 相似文件建议**（`FileReadTool.ts:639-647`），astral 主句一致但无建议。

### Session memory compact

- **【有意分叉，已拍板保留】提取触发阈值 100k/20k/10**：CC 源码默认 init 10k / 增量 5k / 3 次工具调用（`sessionMemoryUtils.ts:32-36`，可被远程配置覆盖，生产值未知）；**维护者确认 astral 的 100k/20k/10 是自定的保守值**（有意压低提取频率），不是抄错。触发逻辑形状一致。代价要知情：summary 更新频率低 → compact 时 summary 更陈旧、boundary 间隙更大——B1/B2 修掉后这个代价可控。建议做成可配置（默认不变），便于后续按真实会话数据调参。
- **【真偏差】summary.md 写入非原子**：CC 的 Edit 落盘走 tempfile + `renameSync`（`utils/file.ts:84-98, 423-438`）；astral 全部裸 `tokio::fs::write`（`sidechain.rs:306/349`、`session_memory.rs:160/184`）。state.json 是 astral 自创（CC 无持久化状态，全内存），更应原子写——它一旦写坏，读失败仅 warn 跳过，等于静默丢 boundary。
- **【自有设计的连带义务】state 持久化 + 多进程**：CC 状态是进程内存（重启即清，天然无一致性问题）；astral 把 boundary/token 基线持久化到 `state.json` 且 per-thread 跨进程，就必须自己解决 CC 不存在的问题：多进程 resume 并发提取（`RUNNING_EXTRACTIONS` 是进程级，`session_memory.rs:50-51`）、崩溃残留、shutdown 清别人的标记（`:526-532`）。这不是"抄错"，是自有设计没配套做完。
- **【行为差异，非 bug】mid-turn compact 丢 initial context**：`compact.rs:57-66` 注释声明的不变量被 `try_compact_inner` 自己违反（`session_memory.rs:327, 360` 丢弃 `_initial_context_injection`），当前 turn 剩余请求缺 user_instructions/environment_context。这是 astral 对接 Codex 历史重建机制的自有问题（CC 无此机制），无测试覆盖。
- **【前瞻】/compact 自定义指令**：astral 目前根本没有指令入口（Op 层无参数，`tasks/compact.rs:37-42`），谈不上"被吞"（v1 判定修正）。但 CC 的行为是**有指令时跳过 SM compact 走 legacy** 让指令生效（`commands/compact/compact.ts:52-57`）——将来给 astral 加指令支持时必须带上这个跳过逻辑，否则就会变成真 bug。

### 控制面 / 默认外联

- **`doctor` 的 fallback 探测硬编码 `api.openai.com`**（`codex-rs/cli/src/doctor.rs:2339, 2349`）：ApiKey 模式且 provider 无 base_url 时会去打 OpenAI——而默认 `astral` provider 未设 `ASTRAL_BASE_URL` 时恰好落进这个分支。全仓最后一个运行时可触发的 OpenAI 硬编码 endpoint。应改报"未配置"。
- **TUI 每次启动无门禁外联 `raw.githubusercontent.com` 拉 announcement**（`codex-rs/tui/src/tooltips.rs:6, 151-163`，`tui/src/lib.rs:1319` 触发）：指向 fork 自有仓库，但是唯一完全无开关的默认外联。建议挂到 `check_for_update_on_startup` 同一 gate。
- **更新检查默认开启**（release 构建，`updates.rs:65-66`、`npm_registry.rs:5`，默认 true 见 `core/src/config/mod.rs:3476`）：端点已 repointed 到 fork 自有命名空间，属可接受，但若目标是"零默认外联"需明确取舍并在 README 声明这几条 egress。

---

## 五、P2：清理与架构质量

- **完成 `ResponseItem` 的中立化，并用无损往返测试固化**（v1 的"砍掉往返"判定已修正——经与维护者确认，`ResponseItem` 已被改造为中立的 canonical 内部表示，同时服务 chat completions 和 Anthropic Messages，投影层是有意设计，名字属于命名债）。剩余收尾：
  1. `"anthropic_signature:"` 字符串前缀走私（`sse/agent.rs:367` vs `core/src/agent_request.rs:37`）说明中立化未做完——signature、`redacted_thinking`、cache 标记应成为 `ResponseItem` 的结构化字段（或 `provider_metadata` 扩展位），而非编码进 `encrypted_content`；
  2. 立不变量并用 property test 固化：每个 wire 的 adapter → `ResponseItem` → adapter 必须无损往返。现有的 reasoning 多块丢 signature、跨 provider 产生非法 thinking block 都会被这条测试当场抓住；
  3. 在文档中显式定位 `agent-protocol` 的 IR 类型为 adapter 的流式中间产物（wire-local），防止它漂移成第二个中立表示；
  4. `ResponseItem` 一族改名（2026-07-03 维护者拍板：现在做，任务清单 Z1，作为执行计划的第一个 PR）——`Response*` 前缀误导读者以为耦合 OpenAI Responses API，本审查 v1 的架构误判即由此名而起。关键事实：改类型标识符不动 serde tag/字段名，纯编译期变更，rollout 兼容无损；schema 生成物的类型名变更趁 v1 未发布时付清最便宜；
  5. 顺带清理 codex-api 里残留的 realtime_websocket（约 3000 行）等 Responses 时代 sideband endpoint。
- **anthropic_cache_fold 状态机**（`core/src/anthropic_cache_fold.rs:57-66`）：pinned index 不随历史重写（compact/回滚）重置；请求发出前就 mutate 状态、失败即污染；首次撞上不支持 `cache_edits` 的端点是一次用户可见 400（`client.rs:584-590`），本可摘掉 fold 原地重试。
- **死代码删除**：`codex-rs/chatgpt/` 空目录、`app-server-transport` remote_control 整目录（双重封死但仍保留 `wham/remote/control/*` 路径拼接）、`connectors` crate 的 directory HTTP 助手、`backend-client` 的 wham/HostedApi 路径风格、`feedback` 的 sentry 依赖（`feedback/Cargo.toml:14`）、`has_chatgpt_account` / `Product::Chatgpt` 等死枚举。
- **`otel.exporter = "statsig"` 被静默映射为 None**（`otel/src/config.rs:9-17`）：应在 config 加载层显式 warn 并从 schema 移除。
- **CODEX_* env 仍是运行时契约**（`core/src/spawn.rs:20,25` 的 `CODEX_SANDBOX*` 注入子进程）：改名要配一个版本的双写兼容，建议排期。
- **两个 adapter 的 `apply_provider_body_overrides` 逐字重复**（`anthropic.rs:361-372` / `chat_completions.rs:608-619`），提公共函数。
- **flavor 推断靠 name/base_url 子串猜测**（`model_provider_info.rs:432-464`）：自建反代域名静默落到 generic，至少日志打印推断结果。
- 工具层小项（均为自有实现细节，非 CC 偏差）：`FileReadStateStore` 无界缓存全文（建议 hash+mtime）、`resolve_path` 的 `trim()` 吞合法空格文件名、`~` 在 core 本地展开（远程环境语义错）、`split_context_line` 对含 '-' 的 context 行放弃 path 前缀、`is_partial_view` 死字段。

---

## 六、models-manager 系统提示词改动（已合入：`fa4d5fece3`，PR #29）

改动内容：`models.json` 全部模型条目 + `prompt.md` + `DEFAULT_PERSONALITY_HEADER` 的 base_instructions 从分节式（Operating Style / Native Tool Flavor / Sandbox / Planning / Code Work / Communication）收敛为 8 段行为准则式短 prompt（work from evidence / stay within the request / protect the user's work / report honestly…），测试同步更新。审查时该改动尚在工作区，现已合入 main。

评价：

- **方向正确**：新 prompt 更短、更行为化，减少每请求 token 固定开销，且"证据先行/不越界/诚实汇报"是对弱模型更有效的约束形式。与 harness-bench 观察到的"Astral token/cache footprint 偏大"问题方向一致。
- **两个值得跟进的删减**（已合入，以下作为后续跟进项而非提交前提）：
  1. 删掉了 *"your job is to keep working until the user's task is genuinely handled"* 这类 persistence 条款。对 DeepSeek 等模型，agentic 持续性很大程度靠这句话撑着；新 prompt 只有 "end to end" 一处弱表述。建议跟进补一句明确的"不要中途停在提案/半成品"。
  2. 删掉了 Native Tool Flavor 段（TodoWrite 时机、后台任务 monitor、subagent 使用）。理论上这些该由工具描述自身承载——但结合第四节"Bash 静态描述在 ShellCommand 回退模式下名不副实"的发现，工具描述目前还没有完全接住这个职责。补齐工具描述动态生成（任务清单 C7）后复查是否需要 prompt 兜底。
- **验证建议**：用 harness-bench fair 对比（2026-06-14 的基建可直接复用）回归验证一轮，重点确认 `20-heartbeat-escalation` 这类长任务的 persistence 相比旧 prompt 不回退；若回退，优先补 persistence 条款。

---

## 七、建议的改进路线（按优先级）

1. **协议层生存性三连修**（P0.1/2/3/4）：流终止收敛为显式状态机（usage 永远旁路）、未知 SSE 输入 warn+skip、429 带 retry-after 退避、max_tokens 走模型目录。这四个都打在"国产网关兼容"主目标上，且都有明确修法。
2. **Session memory compact 补齐"没抄全"的三处**（P0.5/6/7）：legacy compact 后重置 boundary（对齐 CC 三处显式重置）、等待改 1s 轮询+超时容忍、熔断从 SM 失败上摘掉。这三个都有 CC 侧的明确参照实现，修法无争议。顺带把被 869e63deb5 删掉的失败回滚集成测试补回。
3. **工具层四个真 bug**（P0.8/10/11 + limit=0）：Grep multiline、Read 双护栏（256KB + 25K token，抄 CC 文案）、Write/Edit 建父目录、limit=0 校验。每个都是小改动 + 一个测试。
4. **Glob 后端性能**（P0.9）：pattern 前缀剪枝 + 扫描上限，语义保持 CC 对齐不动。这是当前唯一能拖死会话的层。
5. **建立两张"契约表"防回归**：
   - *flavor 能力表*：reasoning 回传、usage 字段位置、tool_call index 行为、stream_options 支持、默认 max_tokens 等 provider 差异集中查表，adapter 不再散落 if；
   - *Claude Code 行为 golden test*：本次对照确认的"忠实点"（行号格式、排序方向、截断文案、tail 参数、空文件 reminder 等）集中固化成对照测试，防止后续重构无意破坏；配真实 provider 抓包 SSE 回放（DeepSeek、SiliconFlow、GLM、Kimi、MiniMax 各一份 golden fixture）作为兼容矩阵回归基线；
   - *无损往返 property test*：每个 wire 的 adapter → `ResponseItem` → adapter 恒等（见 P2 第一条），这是 `ResponseItem` 作为中立 canonical 表示的核心不变量。
6. **自有机制的连带义务**：environment_id key 统一、CoreToolCall 补 Interrupted 终态、state.json/summary.md 原子写、mid-turn compact 的 initial context。
7. **控制面扫尾**（一个下午的量）：doctor 的 api.openai.com fallback、announcement fetch 加 gate、删死代码目录、statsig 显式 warn。
8. **prompt 改动跟进**（已合入 `fa4d5fece3`）：跑一轮 fair bench 回归；按第六节建议评估补 persistence 条款；C7 完成后复查工具引导是否需要 prompt 兜底。
9. **需要拍板的设计选择**（不是 bug，但要有意识地选）：Bash 前台 10s yield vs CC 的 120s 阻塞；提取阈值 100k/20k/10 vs CC 的 10k/5k/3；pattern 含 `\n` 报错 vs CC 静默空结果。三个都建议做成可配置或向 CC 靠拢，理由：模型是按 CC 分布训练的。

## 八、v1 判定撤回/修正清单（经 Claude Code 源码对照）

以下 v1 发现经对照**撤回或改判为忠实模仿**，不需要修：

| v1 发现 | 对照结论 |
| --- | --- |
| Read 行号无 padding 偏离 CC | CC 当前默认就是 `{n}\t` 紧凑格式（`utils/file.ts:304-308`，padStart(6)+`→` 是 killswitch 旧格式）。一致。 |
| Glob 截断保留"最旧"是 bug | CC 就是 `rg --sort=modified` 升序 + slice 保最旧（`utils/glob.ts:94-127`），文案也一致。忠实模仿。 |
| Glob hidden=true / 含 .git 是偏离 | CC 默认同样 `--no-ignore --hidden`，连 env 开关名都相同。忠实模仿（性能问题另算，见 P0.9）。 |
| Glob/Grep 相对路径偏离 CC | CC 同样 `toRelativePath`，出 cwd 保持绝对（`utils/path.ts:95-99`）。一致。 |
| Edit freshness mtime 误报是 bug | CC 同样 mtime 为主、内容 fallback 只在 full-read state 生效、Read 永远存 offset=1 → `touch` 后 CC 也误报。忠实模仿；astral 的 canonicalize key 与 mtime 相同内容不同报错属合理增强。 |
| Grep 超长行占位符替换偏离 | CC 就是 `--max-columns 500` + `[Omitted long matching line]`。逐字一致。 |
| Grep Count 总数是分页后窗口 | CC 同样先截断再求和。一致。 |
| Read 单行截 2000 字符缺失 | CC 当前源码**没有**单行截断（v1 描述有误）；真缺的是 256KB/25K-token 两道护栏（已列 P0.10）。 |
| SM 提取无 Edit 也推进 boundary | CC 同样无条件推进（`sessionMemory.ts:344-349`），无"无编辑=失败"防护；astral 还多了 old==new 拒绝 + follow-up 轮次。忠实模仿。注意：astral 的 state 持久化放大了同款行为的丢失窗口（CC 内存态重启即清），列为可选增强而非 bug。 |
| boundary None 静默丢弃、validate_summary 弱 | CC 同样只校验非空/非模板（`sessionMemoryCompact.ts:533-543`），resumed-session 路径行为一致。忠实模仿。 |
| 删除 tiny-rewrite 守卫是防线倒退 | CC 本来就没有该守卫，防掏空只靠 prompt 措辞（astral 逐字复刻了）。869e63deb5 的删除恰是向 CC 对齐。改判为可选增强。 |
| /compact 自定义指令被 SM 静默吞掉 | astral 目前无指令入口，谈不上"吞"；CC 的做法是有指令则跳过 SM。改为前瞻性要求（见 P1）。 |
| compact 后 summary-first 顺序、10k/5/40k tail 参数 | 与 CC 完全一致（`sessionMemoryCompact.ts:57-61`、`compact.ts:330-338`），文案逐字复刻。忠实模仿。 |

## 九、与 Claude Code 的分叉意图登记（2026-07-03 与维护者确认）

真 bug 类发现需要再分"不小心没抄对"和"故意没抄"——前者修复无争议，后者是受保护的设计决策，Codex 执行时不得"顺手对齐 CC"改掉。依据：progress 文档、commit message、维护者当面确认。

### 有意分叉（受保护，不改）

| 分叉点 | 意图依据 |
| --- | --- |
| Bash 前台 10s yield 成后台 task（CC 是阻塞到完成/120s） | **维护者确认有意保留**——terminal 持续观察体验是项目核心优势，UnifiedExec 语义不向 CC 靠拢 |
| 后台任务四件套（ReadTaskOutput 等）+ `task_id` 错误语义 | progress 文档补充 64/65，有意设计 |
| subagent/multi-agent 工具保持 Codex 原版，不 Claude-ish 化 | progress 文档明确记录 |
| `AskUserQuestion` 走 Codex 原生 `request_user_input` UI | progress 文档明确记录 |
| SM 提取阈值 100k/20k/10（CC 源码默认 10k/5k/3） | **维护者确认为自定的保守值**，有意压低提取频率，不是抄错 |
| SM 状态持久化到 `state.json`（CC 为进程内存态） | 适配 daemon/C-S 多进程架构的必要改造；但连带义务（原子写、多进程防护）未做完，属欠账 |
| Glob 的 `--no-ignore --hidden` 语义及噪声目录剪枝的删除 | commit `613c7a3188` "Align Glob and Grep with Claude Code behavior" 有意对齐；性能回归是该次对齐的无意副作用。Grep 只保护 hidden 行为；按 Claude Code ground truth 仍尊重 ignore 文件，不在 `--no-ignore` 保护范围 |
| Plan Mode / Goal Mode / local compact 骨架保留 Codex 方案 | progress 文档明确记录 |
| 非 multiline pattern 含 `\n` 时报错（CC 静默空结果） | 无意形成但结果更优，作为开放设计选择保留（任务清单 C8） |

### 不小心没抄对（修复无争议）

Grep files_with_matches 漏 `multi_line`（另两个模式都有，明显抄漏）；Write/Edit 不建父目录；Read 双护栏缺失；legacy compact 后不重置 boundary；compact 等待提取的一次性 15s sleep + 超时判失败；summary.md 非原子写；Read limit=0 / CRLF 归一。这些在文档和 commit 中均无有意迹象，且与 progress 文档"工具 flavor 仍需继续校准 schema/result shape"的自述吻合。

### 未经决策 → 已决策（2026-07-03 维护者拍板）

**SM compact 失败熔断（3 次、持久化）**：由 Codex agent 实现，维护者确认未专门决策，现已拍板按审查建议处理——**删除 SM 层熔断，失败降级为日志/metrics**（零成本事件不该有自动关闭；自锁 + 持久化违反"不留静默单向退化机制"原则）。progress 文档中"类似 `rapid_refill_breaker` 保护语义"的真实意图（防 compact thrashing）在 autocompact 整体层单独落地（与 CC 同层，内存态，触发时明确提示）。见任务清单 B3 / B3b。

## 十、测试覆盖缺口汇总（修上述问题时顺带补齐）

- 协议层：每 chunk 带 usage 的流、未知 Anthropic 事件容错、Anthropic messages 端到端 wiremock 回放（目前 `codex-api/tests/clients.rs` 只测 chat completions）、429/retry-after、tool_calls 缺 index、跨 provider 历史投影。
- 工具层：Grep multiline（三种输出模式）、Read 护栏触发、Write 建父目录、多环境 key 一致性、CRLF/非 UTF-8 Edit、interrupt 下的 CoreToolCall 生命周期、CC 行为 golden 对照测试（第七节第 5 条）。
- Compact：legacy fallback 后状态链（P0.5 整条）、compact 与提取并发（轮询路径）、mid-turn auto compact、进程崩溃恢复、resume/fork 交互、熔断打开/恢复。
