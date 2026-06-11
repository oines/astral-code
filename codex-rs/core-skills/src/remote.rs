use anyhow::Result;
use std::path::PathBuf;

use codex_login::CodexAuth;

fn remote_skill_control_plane_disabled_error() -> anyhow::Error {
    anyhow::anyhow!("legacy hosted remote skill control-plane is disabled in Astral")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteSkillScope {
    WorkspaceShared,
    AllShared,
    Personal,
    Example,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteSkillProductSurface {
    Chatgpt,
    Codex,
    Api,
    Atlas,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSkillSummary {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSkillDownloadResult {
    pub id: String,
    pub path: PathBuf,
}

pub async fn list_remote_skills(
    _hosted_base_url: String,
    _auth: Option<&CodexAuth>,
    _scope: RemoteSkillScope,
    _product_surface: RemoteSkillProductSurface,
    _enabled: Option<bool>,
) -> Result<Vec<RemoteSkillSummary>> {
    Err(remote_skill_control_plane_disabled_error())
}

pub async fn export_remote_skill(
    _hosted_base_url: String,
    _codex_home: PathBuf,
    _auth: Option<&CodexAuth>,
    _skill_id: &str,
) -> Result<RemoteSkillDownloadResult> {
    Err(remote_skill_control_plane_disabled_error())
}

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;
