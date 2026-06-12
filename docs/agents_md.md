# AGENTS.md

`AGENTS.md` files provide workspace guidance for Astral agents. They are read
from the active workspace hierarchy and are not loaded from old Codex state.

## Hierarchical agents message

When the `child_agents_md` feature flag is enabled (via `[features]` in
`config.toml`), Astral appends additional guidance about AGENTS.md scope and
precedence to the user instructions message and emits that message even when no
AGENTS.md is present.
