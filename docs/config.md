# Configuration

Astral configuration lives under `ASTRAL_HOME`, which defaults to
`~/.astral-code`. The project does not read, migrate, or fall back to old Codex
configuration.

Provider credentials are configured with provider-neutral settings such as
`ASTRAL_API_KEY` and `ASTRAL_BASE_URL`, plus the corresponding keys in
`config.toml`.

## Lifecycle hooks

Admins can set top-level `allow_managed_hooks_only = true` in
`requirements.toml` to ignore user, project, and session hook configs while
still allowing managed hooks from requirements and managed config layers. This
setting is only supported in `requirements.toml`; putting it in `config.toml`
does not enable managed-hooks-only mode.
