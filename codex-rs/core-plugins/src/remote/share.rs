use super::*;
use codex_login::CodexAuth;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

mod checkout;
mod local_paths;

pub use checkout::checkout_remote_plugin_share;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePluginShareSaveResult {
    pub remote_plugin_id: String,
    pub share_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemotePluginShareAccessPolicy {
    pub discoverability: Option<RemotePluginShareDiscoverability>,
    pub share_targets: Option<Vec<RemotePluginShareTarget>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RemotePluginShareDiscoverability {
    Listed,
    Unlisted,
    Private,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RemotePluginShareUpdateDiscoverability {
    Unlisted,
    Private,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemotePluginSharePrincipalType {
    User,
    Group,
    Workspace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemotePluginShareTarget {
    pub principal_type: RemotePluginSharePrincipalType,
    pub principal_id: String,
    pub role: RemotePluginShareTargetRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RemotePluginSharePrincipal {
    pub principal_type: RemotePluginSharePrincipalType,
    pub principal_id: String,
    pub role: RemotePluginSharePrincipalRole,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemotePluginShareTargetRole {
    Reader,
    Editor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemotePluginSharePrincipalRole {
    Reader,
    Editor,
    Owner,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePluginShareUpdateTargetsResult {
    pub principals: Vec<RemotePluginSharePrincipal>,
    pub discoverability: RemotePluginShareDiscoverability,
}

pub async fn save_remote_plugin_share(
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
    _codex_home: &Path,
    _plugin_path: &AbsolutePathBuf,
    _remote_plugin_id: Option<&str>,
    _access_policy: RemotePluginShareAccessPolicy,
) -> Result<RemotePluginShareSaveResult, RemotePluginCatalogError> {
    Err(super::remote_plugin_control_plane_disabled_error())
}

pub async fn list_remote_plugin_shares(
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
    _codex_home: &Path,
) -> Result<Vec<RemotePluginShareSummary>, RemotePluginCatalogError> {
    Err(super::remote_plugin_control_plane_disabled_error())
}

pub fn load_plugin_share_remote_ids_by_local_path(
    codex_home: &Path,
) -> io::Result<BTreeMap<AbsolutePathBuf, String>> {
    let local_paths = local_paths::load_plugin_share_local_paths(codex_home)?;
    local_paths
        .into_iter()
        .map(|(remote_plugin_id, local_plugin_path)| {
            if !is_valid_remote_plugin_id(&remote_plugin_id) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "invalid remote plugin id in share local path mapping: {remote_plugin_id}"
                    ),
                ));
            }
            Ok((local_plugin_path, remote_plugin_id))
        })
        .collect()
}

pub async fn delete_remote_plugin_share(
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
    _codex_home: &Path,
    _remote_plugin_id: &str,
) -> Result<(), RemotePluginCatalogError> {
    Err(super::remote_plugin_control_plane_disabled_error())
}

pub async fn update_remote_plugin_share_targets(
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
    _remote_plugin_id: &str,
    _targets: Vec<RemotePluginShareTarget>,
    _discoverability: RemotePluginShareUpdateDiscoverability,
) -> Result<RemotePluginShareUpdateTargetsResult, RemotePluginCatalogError> {
    Err(super::remote_plugin_control_plane_disabled_error())
}
