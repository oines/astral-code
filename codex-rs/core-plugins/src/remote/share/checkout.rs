use super::super::RemotePluginCatalogError;
use super::super::RemotePluginServiceConfig;
use codex_login::CodexAuth;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePluginShareCheckoutResult {
    pub remote_plugin_id: String,
    pub plugin_id: String,
    pub plugin_name: String,
    pub plugin_path: AbsolutePathBuf,
    pub marketplace_name: String,
    pub marketplace_path: AbsolutePathBuf,
    pub remote_version: Option<String>,
}

pub async fn checkout_remote_plugin_share(
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
    _codex_home: &Path,
    _remote_plugin_id: &str,
) -> Result<RemotePluginShareCheckoutResult, RemotePluginCatalogError> {
    Err(super::super::remote_plugin_control_plane_disabled_error())
}
