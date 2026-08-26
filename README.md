# Astral Code

A **provider-neutral** coding agent harness forked from [OpenAI Codex CLI](https://github.com/openai/codex). The CLI command is `astral`.

Astral isolates provider-specific authentication, protocols, and hosted capabilities so the same runtime can work with local models, API-key providers, AWS Bedrock, and the optional built-in Codex OAuth provider while keeping Codex's proven Rust core, cross-platform sandbox, and TUI intact.

---

## Astral vs. Codex

Astral diverges from upstream Codex in the following ways:

| Dimension | Codex (upstream) | Astral |
|---|---|---|
| **CLI binary** | `codex` | `astral` |
| **State directory** | `~/.codex` (`CODEX_HOME`) | `~/.astral-code` (`ASTRAL_HOME`) |
| **Authentication** | ChatGPT sign-in, PKCE, device-code OAuth, `OPENAI_API_KEY` | Provider-scoped API keys, AWS/local credentials, plus persistent ChatGPT/Codex OAuth |
| **Hosted services** | Responses API proxy, remote compaction, hosted auth | Isolated by provider; Codex OAuth can use its account models, quota, Web, and image backends without affecting offline or API-key providers |
| **Tool flavor** | OpenAI Codex tool schemas | Claude Code–compatible tool schemas (`Bash`, `Read`, `Edit`, `Write`, `Glob`, `Grep`, `TodoWrite`, `Skill`, etc.) |
| **Provider support** | OpenAI API (primary) | Built-in Codex OAuth, OpenAI-compatible APIs, Anthropic, Bedrock, Ollama, LM Studio, LiteLLM, and custom endpoints |
| **Agent identity crate** | `codex-agent-identity` | `codex-agent-protocol` — generic agent protocol |
| **Capability sync** | N/A | `astral sync-caps` — pull model capabilities from LiteLLM |
| **Glob / Grep** | Codex-native search | Aligned with Claude Code behavior and output format |
| **Upstream CI** | Full GitHub Actions matrix | Disabled; Astral CI runs independently |

In short: **same Rust engine, different soul.** Astral is designed for developers who want the Codex runtime power without vendor lock-in.

---

## Architecture

```
astral-code/
├── codex-rs/                # Rust workspace — the core engine (90+ crates)
│   ├── cli/                 #   `astral` binary entry point
│   ├── core/                #   LLM conversation loop, tool orchestration, sandbox policy
│   ├── tui/                 #   Terminal UI (Ratatui-based)
│   ├── tools/               #   Astral-flavored tool definitions (Bash, Read, Edit, Write, Glob, Grep …)
│   ├── app-server/          #   JSON-RPC server for IDE / desktop integration
│   ├── app-server-protocol/ #   Wire protocol + TypeScript codegen
│   ├── exec/                #   Non-interactive exec mode (`astral exec`)
│   ├── exec-server/         #   Headless exec server
│   ├── mcp-server/          #   MCP (Model Context Protocol) server
│   ├── config/              #   TOML config loading, layered merging, schema
│   ├── sandboxing/          #   Cross-platform sandbox (Seatbelt / Landlock / bwrap)
│   ├── model-provider/      #   Provider runtime (Codex OAuth, APIs, Bedrock, local models …)
│   ├── network-proxy/       #   HTTP/SOCKS5 proxy with policy-based connect control
│   ├── login/               #   Provider-scoped authentication, Codex OAuth refresh and revoke
│   ├── state/               #   SQLite-backed session state and logs
│   ├── hooks/               #   Pre/post command hook system
│   ├── skills/              #   Skill system
│   ├── plugin/              #   Plugin system
│   ├── memories/            #   Persistent memory read/write
│   └── utils/               #   Shared utilities (path, cache, PTY, etc.)
├── sdk/
│   ├── python/              # Python SDK
│   └── typescript/          # TypeScript SDK
├── docs/                    # Project documentation
├── scripts/                 # Build, CI, and formatting scripts
├── tools/                   # Development tools (argument-comment lint, etc.)
├── patches/                 # Bazel dependency patches
└── justfile                 # Task runner recipes
```

### Key Design Decisions

- **Rust-first**: The entire runtime is written in Rust for performance, memory safety, and cross-platform sandboxing.
- **Crate-per-concern**: Each crate in `codex-rs/` owns a single responsibility. Crate names retain the `codex-` prefix for workspace compatibility.
- **Multi-platform sandboxing**: macOS uses Seatbelt (`sandbox-exec`), Linux uses Landlock + bubblewrap, Windows uses restricted tokens.
- **Provider-neutral core**: stable request and session behavior is shared; authentication, wire quirks, and hosted capabilities remain inside each provider's adapter.
- **Provider isolation**: Codex OAuth credentials and tools are restricted to the reserved `codex` provider. Custom Responses endpoints do not inherit Codex hosted capabilities.
- **Claude Code–compatible tools**: Tool schemas and prompts (`astral_flavor.rs`) are aligned with Claude Code's tool definitions, enabling a familiar agentic coding experience across providers.

---

## Quickstart

### System Requirements

| Requirement | Details |
|---|---|
| OS | macOS 12+, Ubuntu 20.04+/Debian 10+, or Windows 11 via WSL2 |
| Git | 2.23+ (optional, recommended) |
| RAM | 4 GB minimum, 8 GB recommended |

### Build from Source

```bash
git clone https://github.com/oines/astral-code.git
cd astral-code/codex-rs

# Install Rust toolchain (if needed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustup component add rustfmt clippy

# Install helper tools
cargo install --locked just
cargo install --locked cargo-nextest

# Build & run
cargo build
cargo run --bin astral -- "explain this codebase to me"
```

### Non-Interactive Mode

```bash
cargo run --bin astral -- exec "list files in this directory"
```

### Optional: Sign in to Codex

The built-in `codex` provider can use the models and quota attached to a ChatGPT/Codex account:

```bash
# Browser login with a localhost callback
astral login codex

# Device-code login for a remote or headless machine
astral login codex --device-auth

# Inspect or remove only the Codex login
astral login status
astral logout codex
```

By default, the login persists under `$ASTRAL_HOME/auth/codex.json` (normally `~/.astral-code/auth/codex.json`) and is kept separate from every API-key provider. Signing in does not silently change the active provider: start Astral and use `/model` to select a complete `provider/model` pair. The selection applies to the next turn without restarting the session.

---

## Configuration

Astral uses its own TOML config at `~/.astral-code/config.toml`. It does not read `~/.codex`.

```bash
# Set an API key (example for OpenAI-compatible providers)
export ASTRAL_API_KEY="sk-..."

# Or configure in config.toml
# See docs/config.md for the full reference
```

Generate the config JSON schema:

```bash
just write-config-schema
just write-app-server-schema
just write-hooks-schema
```

### Codex Web and image tools

Codex capabilities are selected per model and account; they are not inferred merely because a provider uses the Responses wire format.

| Codex request mode | Web | Image generation |
|---|---|---|
| Full Responses | Hosted Web by default; `web.run` when the `standalone_web_search` feature is enabled | Local Agent Loop tool `image_gen.imagegen` |
| Responses Lite | Local Agent Loop tool `web.run`; no Hosted ToolSpec | Local Agent Loop tool `image_gen.imagegen` |

`web.run` and `image_gen.imagegen` are visible model tools executed by Astral's local Agent Loop. Their backend requests use the signed-in Codex account; they never read an OpenAI API key. Astral's generic `web.search` and `web.fetch` are hidden while the Codex adapter owns Web routing, preventing duplicate Web tools.

Image availability still depends on the selected model's image capabilities, the account plan, and the `image_generation` feature. A custom Responses URL or another Provider never gains Codex OAuth, `web.run`, or `image_gen.imagegen` automatically; it continues to use only its explicitly configured capabilities and Astral's generic tools.

---

## Development

### Task Runner

Astral uses [just](https://github.com/casey/just). Default working directory is `codex-rs/`:

```bash
just fmt              # Format code (Rust, Python, JS, Markdown)
just fmt-check        # Check formatting without modifying files
just clippy           # Run Clippy lints
just fix -p <crate>   # Auto-fix Clippy warnings for a crate
just test             # Run all tests via nextest
just test -p <crate>  # Run tests for a specific crate
just bench            # Run workspace benchmarks
just mcp-server-run   # Start the MCP server
```

### Bazel

```bash
just bazel-codex         # Build and run via Bazel
just bazel-test          # Run all Bazel tests
just bazel-clippy        # Run Clippy via Bazel
just bazel-lock-update   # Refresh MODULE.bazel.lock
just build-for-release   # Build release binaries
```

### Verbose Logging

```bash
# TUI mode
astral -c log_dir=./.astral-log
tail -F ./.astral-log/astral-tui.log

# Non-interactive mode
RUST_LOG=debug astral exec "hello"
```

---

## SDKs

**Python** — `sdk/python/` (see [sdk/python/README.md](sdk/python/README.md))

**TypeScript** — `sdk/typescript/` (see [sdk/typescript/README.md](sdk/typescript/README.md))

---

## Documentation

| Topic | Link |
|---|---|
| Getting Started | [docs/getting-started.md](docs/getting-started.md) |
| Installation | [docs/install.md](docs/install.md) |
| Configuration | [docs/config.md](docs/config.md) |
| Sandbox & Security | [docs/sandbox.md](docs/sandbox.md) |
| Exec Mode | [docs/exec.md](docs/exec.md) |
| Exec Policy | [docs/execpolicy.md](docs/execpolicy.md) |
| Slash Commands | [docs/slash_commands.md](docs/slash_commands.md) |
| Skills | [docs/skills.md](docs/skills.md) |
| Authentication | [docs/authentication.md](docs/authentication.md) |
| Agents.md | [docs/agents_md.md](docs/agents_md.md) |
| Contributing | [docs/contributing.md](docs/contributing.md) |

---

## Contributing

See [docs/contributing.md](docs/contributing.md) for the development workflow, PR guidelines, and community values.

---

## Security

See [SECURITY.md](SECURITY.md) for vulnerability reporting guidelines.

---

## License

Licensed under the [Apache-2.0 License](LICENSE).

Astral Code includes code derived from [Ratatui](https://github.com/ratatui/ratatui) (MIT License). See [NOTICE](NOTICE) for details.
