use crate::remote::RemotePluginServiceConfig;
use codex_login::CodexAuth;
use codex_protocol::protocol::Product;

#[derive(Debug, thiserror::Error)]
pub enum RemotePluginMutationError {
    #[error("legacy hosted remote plugin mutation is disabled in Astral")]
    ControlPlaneDisabled,
}

#[derive(Debug, thiserror::Error)]
pub enum RemotePluginFetchError {
    #[error("legacy hosted remote featured plugin fetch is disabled in Astral")]
    ControlPlaneDisabled,
}

pub async fn fetch_remote_featured_plugin_ids(
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
    _product: Option<Product>,
) -> Result<Vec<String>, RemotePluginFetchError> {
    Err(RemotePluginFetchError::ControlPlaneDisabled)
}

pub async fn enable_remote_plugin(
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
    _plugin_id: &str,
) -> Result<(), RemotePluginMutationError> {
    Err(RemotePluginMutationError::ControlPlaneDisabled)
}

pub async fn uninstall_remote_plugin(
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
    _plugin_id: &str,
) -> Result<(), RemotePluginMutationError> {
    Err(RemotePluginMutationError::ControlPlaneDisabled)
}

#[cfg(test)]
#[path = "remote_legacy_tests.rs"]
mod tests;
