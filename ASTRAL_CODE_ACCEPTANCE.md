# ASTRAL_CODE_TASKS 一口气执行 · 验收报告与返工清单

> **二轮验收结论(2026-07-03):通过,可提交。** 详见文末「六、二轮验收记录」。一轮清单(下文一至五节)保留作过程记录。

验收对象:工作区未提交改动(baseline = `main @ b5e3d35bf8`,218 文件,+8365/−5902)。
验收方式:四路独立审计(Z1 serde 兼容 / 协议层 A 系列 / compact B 系列 / 工具面 C+D+F1),逐项对照 `ASTRAL_CODE_TASKS.md` 验收标准与 `~/project/claude-code` ground truth,并跑全量 crate 测试交叉验证。

## 总判定

**有条件通过:阻塞项修完、静默跳过项补申报或补做之前,不得提交。**

- 已落地部分实现质量普遍很好(Z1 教科书级、F1 红线零违规、B3 删得干净)。
- 问题集中在两类:**验证虚报**(只跑自己新写测试的窄过滤器,全量跑实际挂 5 个既有测试)和**静默跳过**(9 个任务项既不在完成清单也不在跳过清单)。
- 两条硬红线经查无恙:ExecutorFileSystem 抽象未破坏;sandbox / approval / PTY 安全语义未变。

---

## 一、阻塞项(必须修复,修完全量测试须绿)

### R1. C4 Glob 剪枝丢失锚定语义(实测回归)
`exec-server/src/search.rs:667-680` 的 `relative_glob_base` 把 `src/*.rs` 改写成 root=`src` + pattern=`*.rs`,不带 `/` 的 override pattern 匹配任意深度,导致 `src/*.rs` 错误匹配 `src/nested/mod.rs`。
全量 `just test -p codex-exec-server` 挂既有测试 `glob_with_slash_matches_relative_path`(search_tests.rs:59)。
**修法**:剥离 literal prefix 后,若原 pattern 含 `/` 且剩余 pattern 不含 `**`,必须保持单层锚定(如以 `/` 前缀形式重写剩余 pattern)。修复后 `just test -p codex-exec-server` 全量必须 208/208 绿,mtime 排序 / take(100) / 截断文案 / hidden 语义不动。

### R2. A3 连带:usage-limit 型 429 被重试风暴
`retry_429: true` 后,`suite::client::usage_limit_error_emits_rate_limit_event` 挂(wiremock 期望 1 次实际打 5 次)。这不只是测试没更新——配额耗尽型 429 被无差别重试是语义错误。
**修法(已裁决,按此执行)**:保留瞬时限流 429 的重试;命中既有 usage-limit 检测路径(配额耗尽 body)的 429 不重试,立即上抛 rate-limit 事件。测试恢复原断言。

### R3. A7 连带:3 个存量测试/快照未按新行为更新
- `suite::client::azure_chat_completions_request_serializes_model_context`(core/tests/suite/client.rs:2397):断言 generic(azure)回传 `reasoning_content`,与 A7 flavor 门控冲突。
- `suite::pending_input` 两个 insta 快照(`queued_inter_agent_mail_triggers_follow_up_after_reasoning_item`、`user_input_does_not_preempt_after_reasoning_item`):仍期望 generic flavor 回传 reasoning 项。
**修法**:按 A7 新行为改写断言/快照(generic 默认不回传);如某测试本意是覆盖"回传开启"路径,则给该 provider 显式设置 `astral_chat_reasoning_content` 覆盖开关,顺带补上覆盖开关本身的专门测试(A7 目前缺)。

### R4. Grep `respect_ignore_files` true→false 属未授权夹带,回退
`search.rs:591-594`。无任何任务条目授权;对照 ground truth,CC GrepTool 只传 `--hidden` 不传 `--no-ignore`(claude-code/tools/GrepTool/GrepTool.ts:330),baseline(true)才是忠实行为。
**修法(已裁决)**:回退为 true;golden test `claude_code_glob_grep_golden_hidden_and_no_ignore_defaults` 相应改写,并把测试目录初始化为 git repo(ignore crate `require_git` 默认 true,非 git 目录下该断言区分不出两种行为,等于没测)。同步更正 `ASTRAL_CODE_REVIEW.md` §九登记表:`--no-ignore` 保护项仅适用于 Glob(证据 glob.ts),Grep 不在保护范围。

### R5. 验证收口
上述修完后,以下必须全量绿并把结果写进执行报告(禁止只跑窄过滤器):
`just test -p codex-core`(当前 2674/2678)、`just test -p codex-exec-server`(当前 207/208)、`just test -p codex-api`、`just test -p codex-protocol`、`just test -p codex-app-server-protocol`、`just fmt`。

---

## 二、静默跳过项(违反"跳过必留档"规则,逐项处置)

以下 9 项既不在报告完成清单、也不在"跳过/保守处理"清单。处置方式:**能低成本补做的补做,其余在执行报告补申报跳过理由**。

| 项 | 处置 |
|---|---|
| B4 state.json/summary.md 原子写(tempfile+rename) | **补做**,成本低、防半写损坏,直接服务稳定性目标 |
| B5 mid-turn compact 恢复 initial context 注入 | **补做**,这是真 bug 留置:`session_memory.rs:316` 丢弃 `_initial_context_injection`,SM 路径违反 compact.rs 自述不变量 |
| B7 测试补缺(崩溃恢复 / resume-fork / 并发防护 / 恢复被删的 `failed_session_memory_sidechain_restores_previous_summary`) | **补做**,至少恢复被 869e63deb5 删除的那个集成测试 |
| B8 /compact 自定义指令约束注释 | **补做**,一条注释的事 |
| C5 environment_id resolved key(astral_file_tools.rs:403,845,930) | **补做**,小改动 |
| C6 Interrupted 终态 | 补申报或补做,允许保守跳过但必须留档 |
| C7 动态 Bash description(含 `astral_bash.rs:222` 静默丢 `run_in_background`) | 补申报;`run_in_background` 静默丢弃至少要改成显式报错或支持 |
| C8 小项包(limit=0 / CRLF-BOM / did-you-mean / `\n` 提示 / state store hash+mtime / 不 trim / `~` 展开) | 补申报,可整包后置,但 state store 存全文 `content: String` 建议尽早换 hash+mtime(内存占用) |
| C10 yield 配置通道 + 后台文案对齐 | 补申报(默认 10s 保护项确认未动,无恙) |

## 三、补作业项(非阻塞,本轮或下轮完成)

- **B1**:修复本体正确,补整链集成测试(SM 失败 → legacy fallback → baseline 重置 → 后续提取正常)。
- **A11**:补每个 wire 的 adapter→item→adapter roundtrip property test(验收标准明确要求;workspace 尚无 property test 基建,引入 proptest 即可)。
- **A6**:thinking budget 补 Anthropic 官方 1024 下限校验(max_tokens 很小时可能产出非法 budget)。
- **C2**:env var 改名——`CLAUDE_CODE_MAX_OUTPUT_TOKENS` 撞了 CC 另一语义的变量名,改为对齐 CC 的 `CLAUDE_CODE_FILE_READ_MAX_OUTPUT_TOKENS` 或干脆 `ASTRAL_FILE_READ_MAX_OUTPUT_TOKENS`;报错文案补 CC 尾句(", or search for specific content instead of reading the whole file.")、字节数格式化。登记 follow-up:给 `FileMetadata` 加 size 字段,把 256KB 判定挪到读前(当前 500MB 文件会整个读进内存/过 base64 wire 才报错)。
- **debug_models 测试**:放宽未申报。改为断言明确的 `{"models":[]}` 空数组,或注入配置后断言非空——当前形态区分不出"优雅空结果"和"目录加载回归"。
- **D1**:doctor 英文输出里嵌了中文字面量"未配置 ASTRAL_BASE_URL",改为英文(照抄任务文档原文抄进了产品文案)。
- **E1 交付物**:补 78 个上游 commit 的分类清单(哪些已收割、哪些明确不适用、哪些保守跳过+复核条件),追加到 PROGRESS.md。当前只收割 2 项且无清单,76 个 commit 处置去向无记录,下次收割等于重新分类一遍。

## 四、收货清单(验收通过,无需改动)

- **Z1**:wire-safe。RolloutItem tag 已钉 `response_item`,legacy roundtrip 测试真实断言旧 tag 字节;全仓无第二处 serde 面变化;fixture 零改动;schema 差异归一化后仅类型名 + additive 可选字段。TS 导出类型名变化属预期 codegen 面。
- **A1/A2/A4/A5/A8/A9/A10/A12a/A13**:实现与测试均达标。A12a fixtures 全部标注 synthetic,红线未踩。
- **B2**(100ms 轮询、timeout 清 marker 继续)、**B3**(熔断删净、旧 state.json 兼容有测试、失败 warn 不吞)。B6 系 baseline 已有,阈值默认 100k/20k/10 保护项无恙。
- **C1**(multiline 修复+三模式测试)、**C3**(mkdir 全程走 ExecutorFileSystem trait,本地远程双实现,调用序列有断言)、**C9 测试本体**(golden 断言质量高;夹带问题见 R4)。
- **D1-D4** 主体、**F1**(设置/读取全仓双写双读无遗漏、旧名可用、user shell 不暴露、early-exit 不变)、**Phase 2 两项收割**(quick-xml 0.41 + RUSTSEC 例外标注规范;PowerShell AST +113 行含测试)。
- render_human_report fixture 修复合理(fixture 与既有断言自洽,非弱化)。

## 五、流程改进(写给下一轮 runbook)

1. **验证口径收紧**:任务文档"focused tests"被解释成了"只跑自己新写的测试"。改为:**每个被改动 crate 的全量测试必须绿**才算该批次收口;窄过滤器只用于开发中迭代。
2. **跳过必留档是硬规则**:任何任务项,要么进完成清单,要么进跳过清单+理由,不允许有第三种状态。本轮 9 项静默失踪。
3. **行为变更必须有任务编号**:R4 那种"顺手改语义"藏在纯测试任务名下,是本轮唯一一次越权。生产语义改动没有对应任务条目时,一律先登记再改。

---

## 六、二轮验收记录(2026-07-03)

验收方式:主会话独立全量复跑 codex-core / codex-exec-server / codex-api / codex-client(**3064/3064 通过**,25 skipped,2 flaky 重试即过)+ 两路独立代码级复核(R1-R4 / 补做项),含一次突变验证。

### 判定:通过

- **R1 PASS**:剩余 pattern 不含 `/` 时改写为 `/{pattern}` 锚定 override root;`src/**/*.rs` 仍递归、裸 `*.rs` 仍任意深度、多层 prefix 正常;剪枝收益与 100k/5s 上限均在;`absolute_glob_base` 同步修正。
- **R2 PASS**:usage_limit_reached 429 不重试立即上抛,瞬时 429 仍重试+尊重 Retry-After;`usage_limit_error_emits_rate_limit_event` 恢复原断言(diff 零改动)。
- **R3 PASS**:azure 测试按新行为改断言,两个 insta 快照更新,覆盖开关专门测试补齐(generic 默认省略 + metadata 显式开启回传),控制键不泄漏进请求体。
- **R4 PASS**:grep `respect_ignore_files` 回到 true;golden test 补了 `.git` 目录满足 require_git,**突变验证证实断言真实区分行为**;Glob no-ignore 保护项未动;REVIEW §九登记表已更正(--no-ignore 仅保护 Glob)。
- **补做项 B4/B7/B8/C5/C7/D1 PASS**:B4 原子写同目录 temp+sync_all+persist、五处裸写全改;B7 恢复的集成测试断言等价且更强;C5 类型收紧到编译层面消除串扰;C7 显式报错;D1 无中文残留。
- **夹带扫描两路均干净**:一轮抓到的越权改动模式没有复发。
- **申报纪律显著改善**:跳过项(C6/C8/C10/A12b/E1/F2/G1)全部留档;全量测试声称与独立复跑一致。

### 遗留 follow-up(全部非阻塞,滚入下轮)

1. B1 整链集成测试(SM 失败→legacy fallback→baseline 重置→后续提取)——未做且二轮报告未申报(本轮唯一申报瑕疵)。
2. B5 mid-turn 场景测试(实现正确,靠与 legacy 共享函数背书,但无 `BeforeLastUserMessage` 驱动 SM 路径的测试)。
3. R2 检测从字符串包含换成 serde 类型化解析(现为 fail-open,异形 body 退回重试但最终错误正确)。
4. A11 roundtrip property test(需引入 proptest)。
5. A6 thinking budget 1024 下限校验。
6. C2:env var 改名(现仍是撞名的 `CLAUDE_CODE_MAX_OUTPUT_TOKENS`)、报错文案对齐 CC、FileMetadata 加 size 做 pre-read 判定。
7. debug_models default 测试改为断言明确空数组。
8. E1 78 个上游 commit 分类清单。
9. C6(Interrupted 终态)、C8(小项包)、C10(yield 配置通道)——已留档的后置专项。
10. 备注:grep VCS 测试改排序比较修 flakiness 后,mtime-desc 排序契约的直接覆盖略弱,可在 C9 补一个平局无关的排序断言。

### 建议提交方式

工作区是两轮改动的总和(230+ 文件),建议按批次拆 PR:Z1 rename 单独一个(纯机械、diff 大)、1A 协议层、1B compact、1C+C9 工具面、1D+F1+Phase2 杂项。每个 PR 引用 PROGRESS.md 对应报告节。
