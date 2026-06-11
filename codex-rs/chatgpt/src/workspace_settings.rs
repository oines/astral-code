use codex_core::config::Config;
use codex_login::CodexAuth;

#[derive(Debug, Default)]
pub struct WorkspaceSettingsCache;

pub async fn codex_plugins_enabled_for_workspace(
    _config: &Config,
    _auth: Option<&CodexAuth>,
    _cache: Option<&WorkspaceSettingsCache>,
) -> anyhow::Result<bool> {
    // Astral does not consult ChatGPT workspace settings. Local plugins, skills, and MCP
    // integrations stay governed by Astral's local config and feature flags.
    Ok(true)
}
