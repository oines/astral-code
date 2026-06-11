# Astral-Code Project State

Last updated: 2026-06-11

This document is the continuity anchor for long-running Astral-Code work. Keep it current before
pausing, after major commits, and before any context compaction risk.

## Core Objective

Build `astral-code` as a deep fork, not a Codex compatibility mode.

The product shape is:

- Public repository: `oines/astral-code`
- CLI command: `astral`
- User-facing project name: `astral-code`
- State/config namespace: `ASTRAL_HOME`, `~/.astral-code`, `ASTRAL_API_KEY`,
  `ASTRAL_BASE_URL`, `ASTRAL_EXEC_SERVER_URL`

The architectural intent is to inherit Codex's strong runtime body while replacing the model-facing
protocol and tool flavor:

- Keep Codex's C/S architecture, daemon/app-server, exec-server, PTY buffering, UnifiedExec,
  sandbox, approval, environments, MCP, skills/plugins, Plan Mode, Goal Mode, local compact, and
  subagent runtime.
- Replace OpenAI/Responses-first assumptions with provider-neutral model plumbing.
- Expose Claude Code-like core tools to the model so domestic or OpenAI-compatible coding models
  see a familiar agentic trajectory.
- Remove or isolate OpenAI proprietary control plane: ChatGPT login, OpenAI hosted auth/routing,
  add-credits/rate-limit nudges, feedback upload, remote compact, OpenAI-only web/search gates, and
  ChatGPT backend defaults.

## Non-Negotiable Decisions

- No Codex user-data compatibility. Do not read, migrate, or fallback to `~/.codex`, `CODEX_HOME`,
  old sessions, old auth, old plugins, or old config.
- Astral naming only for user-visible new surfaces. Internal crate names may still be `codex-*`
  until a later mechanical rename.
- Do not degrade sandbox behavior. Seatbelt/Bwrap/Windows sandbox, approval, and denied-action retry
  paths are part of the inherited runtime boundary.
- Do not replace app-server, exec-server, UnifiedExec, PTY, or Environment/ExecBackend as part of
  the tool flavor work.
- Keep MCP and local skills/plugins. Remote OpenAI/ChatGPT plugin distribution is suspect and should
  be audited separately.
- Keep Codex local compact unless there is clear evidence it damages model-native tool streaming.
  OpenAI remote compact should not remain a dependency.
- Do not store real provider secrets in the repository. A user mentioned DeepSeek model names for
  future manual testing; never persist API keys in docs, fixtures, or commits.

## Current Implementation Strategy

Astral should not be a thin reverse proxy or endpoint hook.

The preferred architecture is:

1. A provider-neutral internal agent IR for messages, tool use, tool results, stream deltas, usage,
   stop reasons, and errors.
2. Provider adapters for Anthropic Messages and OpenAI-compatible `/v1/chat/completions`.
3. A legacy/optional Responses adapter only where needed during transition.
4. Astral-native tool schemas and handlers that are model-facing Claude-ish, while reusing Codex
   runtime primitives internally.

The tool layer should be "native at the planning boundary": model-visible names and schemas are
selected in the core tool plan, not only renamed at the final HTTP edge.

## Inherited Runtime Boundaries

Keep these Codex systems unless there is a strong, explicit reason:

- `app-server` and `app-server-protocol`
- `exec-server`
- `Environment` / `ExecBackend`
- `UnifiedExecProcessManager`
- PTY/stdin/terminate/output streaming behavior
- sandbox and approval engine
- permission request lifecycle
- Plan Mode and Goal Mode host behavior
- local compact/history reconstruction
- MCP runtime and MCP resources
- skills/plugins runtime
- multi-agent v2 runtime

## Claude-Ish Tool Flavor Status

Core model-facing tools currently present in `codex-rs/tools/src/astral_flavor.rs`:

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

Runtime mappings already in tree:

- `Bash` maps to Codex `exec_command` / `shell_command` via `AstralBashHandler`.
- `Monitor` maps to `write_stdin` via `AstralMonitorHandler`.
- `TaskStop` can terminate UnifiedExec shell tasks or interrupt multi-agent tasks.
- `Read/Write/Edit/Glob/Grep` have Astral-native file handlers using Codex filesystem and sandbox
  context. They support `environment_id`.
- `TodoWrite` maps to Codex `update_plan`.
- `Agent` maps to `spawn_agent`.
- `SendMessage` maps to multi-agent v2 `send_message`.
- `RequestPermissions` maps to the existing approval/permission channel.
- MCP resource tools, `ToolSearch`, and `Skill` reuse the existing extension/runtime machinery.

Important implementation files:

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

Deferred tools by decision:

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

These can be added later only if Codex already has a strong runtime primitive or if the feature is
clearly worth implementing natively.

## Completed Progress

Recent commits that matter:

- `7099420969 Remove ChatGPT login entrypoints from CLI`
  - Removed hidden CLI flags for `--with-access-token`, `--device-auth`,
    `--experimental_issuer`, and `--experimental_client-id`.
  - Removed disabled OAuth/access-token login stubs and exports from the CLI crate.
  - Kept `astral login --with-api-key`, `astral login status`, and `astral logout`.
  - Verified that removed ChatGPT login flags now fail at CLI parsing instead of entering a hidden
    disabled flow.

- `7f1e959c0f Remove OpenAI backend routing from providers`
  - Removed OpenAI/ChatGPT default base URL routing from provider conversion.
  - Removed OpenAI org/project env headers from the legacy Responses provider.
  - Disabled legacy Responses provider Astral-managed auth and websocket special casing.
  - Made default provider capabilities provider-neutral: no hosted image generation or web search
    unless explicitly implemented.
  - Fixed provider-local bearer token model refresh so `/models` can work without ChatGPT backend
    auth.

- `55c446e0c9 Switch Windows installer to Astral`
  - Switched Windows installer user-facing envs/paths/package names to Astral.

- `5d511f6440 Let Astral file tools target environments`
  - Added `environment_id` support to `Read/Write/Edit/Glob/Grep`.
  - Routed file tools through Codex environment resolution and sandbox context.

- `28382b9678 Switch package entrypoint and installer to Astral`
  - Switched package entrypoint and Unix installer to `astral`, `ASTRAL_*`, and `~/.astral-code`.

- `f8bd6937b1 Guard Astral tool names in provider-neutral plans`
  - Ensured Astral tool names are preserved in provider-neutral tool planning.

- `bf5b06b874 Add Anthropic prompt cache markers`
  - Added Anthropic prompt-cache support scaffolding.

- `c82843b174 Prune noisy directories in Astral file search`
  - Pruned noisy directories for Astral file search behavior.

- `e53f7253e6 Disable TUI feedback upload flow`
  - Disabled feedback upload from TUI surface.

- `42e5f7f83e Remove OpenAI rate-limit model nudge`
- `0fd4f2e703 Remove add-credits nudge app-server API`
- `5a9ddcf8d4 Remove add-credits nudge from TUI`
- `b756262d8e Remove add-credits nudge backend client`
  - Removed OpenAI add-credits/rate-limit upsell paths.

## Latest Pause-Point Work

At the time this document was created, the latest completed slice is:

- `7099420969 Remove ChatGPT login entrypoints from CLI`

Intent:

- Remove hidden CLI entrypoints for `--with-access-token`, `--device-auth`,
  `--experimental_issuer`, and `--experimental_client-id`.
- Remove disabled OAuth/access-token login stubs and exports.
- Keep `astral login --with-api-key`, `astral login status`, `astral logout`, and the explicit
  "no credentials" guidance.

Validation already run for this latest slice:

- `just fmt`
- `just test -p codex-cli login`
  - Result: 8 tests passed, 273 skipped.

## Validation Already Performed

Recent focused checks:

- `just fmt`
- `just test -p codex-tools astral_flavor`
- `just test -p codex-core astral_file_tools`
- `just test -p codex-model-provider-info`
- `just test -p codex-model-provider configured_provider_uses_default_capabilities`
- `just test -p codex-model-provider configured_provider_models_manager_uses_provider_bearer_token`
- `just test -p codex-models-manager refresh_available_models_fetches_with_provider_auth`
- `just test -p codex-cli login`
- `git diff --check`

Known unrelated or pre-existing issue observed:

- Full `just test -p codex-model-provider` still has Bedrock catalog failures around bundled
  `models.json` missing `gpt-5.5`. Treat this as unrelated to the provider-neutral cleanup unless
  actively working on Bedrock catalog.

## Remaining High-Priority Work

1. Audit and remove remaining OpenAI/ChatGPT auth/config surfaces:
   - `chatgpt_base_url` in core config.
   - app-server `account/login/start` behavior.
   - `codex-rs/login/src/server.rs` OAuth callback server.
   - revoke/token code paths that only exist for ChatGPT OAuth.
   - doctor output that reports ChatGPT login details.

2. Audit cloud/remote control-plane crates:
   - `codex-rs/backend-client`
   - `codex-rs/cloud-config`
   - `codex-rs/cloud-tasks`
   - `codex-rs/core-plugins/src/remote*`
   - `codex-rs/memories/write`

   Decide whether to remove, compile-disable, or isolate these behind explicit non-default
   features. Do not let them silently talk to `chatgpt.com/backend-api`.

3. Continue provider-neutral protocol work:
   - Make Anthropic Messages stream/tool_use/tool_result path first-class.
   - Make OpenAI-compatible chat-completions stream/tool_calls path first-class.
   - Keep Responses as legacy, not the core truth.
   - Ensure usage and stop-reason mappings are stable.

4. Harden tool result shapes:
   - Compare Astral `tool_result` payloads against Claude Code fixtures where useful.
   - Especially verify `Bash`, `Monitor`, `TaskStop`, `Read`, `Edit`, `TodoWrite`, `Agent`, and
     permission-denied/retry flows.
   - Avoid changing the Codex runtime behavior just to imitate names.

5. Verify long-running terminal experience:
   - `Bash(run_in_background=true)` returns a monitorable id.
   - `Monitor` can poll output and write stdin for y/n prompts.
   - `TaskStop` terminates shell tasks.
   - Existing PTY progress streaming remains intact.

6. Keep local compact unless later evidence contradicts it:
   - Do not rewrite compact just for aesthetic parity.
   - Remove or disable any OpenAI remote compact dependency if found.
   - Preserve tool streaming/history shape.

7. Rename user-facing leftovers:
   - Docs/comments/tests can still mention OpenAI-compatible protocols where technically accurate.
   - User-facing product strings should say Astral/Astral-Code.
   - Internal crate names can remain `codex-*` until a later mechanical stage.

## Testing Policy During This Fork

The current working policy is speed-first:

- Run `just fmt` after Rust edits.
- Run focused tests for changed crates or changed behavior.
- Do not burn time or disk on broad workspace tests after every slice.
- Save full-suite testing and broad CI repair for a later stabilization pass.
- If a focused test triggers heavy compilation, let it finish; do not kill Rust processes.

## Important Safety Notes

- Do not edit code related to `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` or `CODEX_SANDBOX_ENV_VAR`.
- Do not weaken sandbox, approval, or exec-server behavior.
- Do not introduce proxy-only hacks as the main architecture.
- Do not write real API keys into files.
- Do not re-enable OpenAI/ChatGPT login to make old tests pass.
- Do not mark the goal complete until provider-neutral protocols, Claude-ish tools, and OpenAI
  control-plane removal are actually verified against the current tree.
