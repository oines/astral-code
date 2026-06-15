# Astral Code

A provider-neutral coding agent harness built on the Codex runtime architecture. The CLI command is `astral`.

Astral is a standalone project with its own configuration and state namespace (`ASTRAL_HOME`, defaulting to `~/.astral-code`). It does not read or migrate `~/.codex` data.

---

## Architecture

Astral is organized as a Rust monorepo with Python and TypeScript SDKs:

```
astral-code/
├── codex-rs/           # Rust workspace — the core engine (90+ crates)
│   ├── cli/            #   `astral` binary entry point
│   ├── core/           #   Business logic, LLM orchestration, sandboxing
│   ├── tui/            #   Terminal UI (Ratatui-based)
│   ├── tools/          #   Tool definitions, discovery, and execution
│   ├── app-server/     #   JSON-RPC server for IDE/desktop integration
│   ├── app-server-protocol/  # Wire protocol + TypeScript codegen
│   ├── exec/           #   Non-interactive exec mode (`astral exec`)
│   ├── exec-server/    #   Headless exec server
│   ├── mcp-server/     #   MCP (Model Context Protocol) server
│   ├── config/         #   TOML config loading, schema, merging
│   ├── sandboxing/     #   Cross-platform sandbox (Seatbelt, Landlock, bwrap)
│   ├── linux-sandbox/  #   Linux bubblewrap/landlock sandbox helper
│   ├── model-provider/ #   LLM provider abstraction (OpenAI, Bedrock, etc.)
│   ├── network-proxy/  #   HTTP/SOCKS5 network proxy with MITM support
│   ├── state/          #   SQLite-backed session state and logs
│   ├── hooks/          #   Pre/post command hooks
│   ├── file-search/    #   Ripgrep-based file search
│   ├── file-system/    #   Filesystem operations
│   ├── skills/         #   Skill system
│   ├── plugin/         #   Plugin system
│   ├── memories/       #   Persistent memory read/write
│   ├── login/          #   Authentication flows
│   └── utils/          #   Shared utilities (path, cache, PTY, etc.)
├── codex-cli/          # Legacy Node.js CLI wrapper
├── sdk/
│   ├── python/         # Python SDK
│   └── typescript/     # TypeScript SDK
├── docs/               # Project documentation
├── scripts/            # Build, CI, and formatting scripts
├── tools/              # Development tools (lint, etc.)
├── patches/            # Bazel dependency patches
└── justfile            # Task runner recipes
```

### Key Design Decisions

- **Rust-first**: The entire runtime is written in Rust for performance, memory safety, and cross-platform sandboxing.
- **Crate-per-concern**: Each crate in `codex-rs/` owns a single responsibility. Crate names are prefixed with `codex-` (e.g., `codex-core`, `codex-tui`).
- **Multi-platform sandboxing**: macOS uses Seatbelt (`sandbox-exec`), Linux uses Landlock + bubblewrap, and Windows uses restricted tokens.
- **Provider-neutral**: LLM provider abstraction in `codex-model-provider` supports OpenAI, Amazon Bedrock, Ollama, LM Studio, and custom endpoints.
- **App-server protocol**: JSON-RPC protocol over Unix domain sockets for IDE and desktop app integration, with TypeScript type generation.

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
# Clone the repository
git clone https://github.com/oines/astral-code.git
cd astral-code/codex-rs

# Install Rust toolchain (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustup component add rustfmt clippy

# Install helper tools
cargo install --locked just
cargo install --locked cargo-nextest

# Build
cargo build

# Run
cargo run --bin astral -- "explain this codebase to me"
```

### Non-Interactive Mode

```bash
# Run a single prompt without the TUI
cargo run --bin astral -- exec "list files in this directory"
```

---

## Development

### Task Runner

Astral uses [just](https://github.com/casey/just) for common development tasks. The default working directory is `codex-rs/`:

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

The project also supports Bazel for reproducible builds and CI:

```bash
just bazel-codex                      # Build and run via Bazel
just bazel-test                       # Run all Bazel tests
just bazel-clippy                     # Run Clippy via Bazel
just bazel-lock-update                # Refresh MODULE.bazel.lock
just build-for-release                # Build release binaries
```

### Configuration

Astral uses TOML configuration. Generate the JSON schema:

```bash
just write-config-schema              # Regenerate config.schema.json
just write-app-server-schema          # Regenerate app-server protocol schemas
just write-hooks-schema               # Regenerate hooks schema
```

### Verbose Logging

```bash
# TUI logging
astral -c log_dir=./.astral-log
tail -F ./.astral-log/astral-tui.log

# Non-interactive mode uses RUST_LOG
RUST_LOG=debug astral exec "hello"
```

---

## SDKs

### Python

```bash
cd sdk/python
pip install -e .
```

See `sdk/python/README.md` for usage.

### TypeScript

```bash
cd sdk/typescript
pnpm install
pnpm build
```

See `sdk/typescript/README.md` for usage.

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

## Project Layout

### Core Crates

- **`codex-core`** — Central business logic: LLM conversation loop, tool execution, sandbox policy resolution, and session management.
- **`codex-tui`** — Full-featured terminal UI built on Ratatui with markdown rendering, diff display, file search, and multi-agent orchestration.
- **`codex-tools`** — Tool definitions (shell, apply_patch, etc.), dynamic tool discovery, MCP tool integration, and tool search.
- **`codex-config`** — TOML-based configuration with layered merging (defaults → global → project → thread), schema validation, and cloud config support.
- **`codex-sandboxing`** — Cross-platform sandbox enforcement using Seatbelt (macOS), Landlock + bubblewrap (Linux), and restricted tokens (Windows).

### Server Crates

- **`codex-app-server`** — JSON-RPC server exposing thread, config, and session management over Unix domain sockets.
- **`codex-app-server-protocol`** — Wire protocol definitions with `ts-rs` TypeScript codegen for client integration.
- **`codex-mcp-server`** — MCP (Model Context Protocol) server for tool exposure to external clients.
- **`codex-exec-server`** — Headless execution server for programmatic access.

### Infrastructure Crates

- **`codex-model-provider`** — Provider abstraction supporting OpenAI, Amazon Bedrock, Ollama, LM Studio, and custom backends.
- **`codex-network-proxy`** — HTTP/SOCKS5 proxy with policy-based connect control and optional MITM.
- **`codex-state`** — SQLite-backed persistence for session history, logs, and analytics.
- **`codex-login`** — Authentication and credential management.
- **`codex-hooks`** — Pre/post command hook system with JSON schema.

---

## CI

The project runs extensive CI via GitHub Actions:

- **`ci.yml`** — Main CI pipeline
- **`rust-ci.yml`** / **`rust-ci-full.yml`** — Rust compilation and test matrix
- **`bazel.yml`** — Bazel build and test
- **`cargo-deny.yml`** — Dependency auditing
- **`codespell.yml`** — Spell checking
- **`sdk.yml`** — SDK tests (Python + TypeScript)
- **Release workflows** — Multi-platform release builds (macOS, Linux, Windows)

---

## Contributing

See [docs/contributing.md](docs/contributing.md) for the development workflow, PR guidelines, and community values.

---

## Security

See [SECURITY.md](SECURITY.md) for vulnerability reporting and safe operation guidelines.

---

## License

Licensed under the [Apache-2.0 License](LICENSE).
