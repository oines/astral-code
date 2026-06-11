use codex_config::CloudConfigBundleLoader;
use codex_config::types::AuthCredentialsStoreMode;
use codex_login::AuthManager;
use std::path::PathBuf;
use std::sync::Arc;

pub fn cloud_config_bundle_loader(
    _auth_manager: Arc<AuthManager>,
    _chatgpt_base_url: String,
    _codex_home: PathBuf,
) -> CloudConfigBundleLoader {
    CloudConfigBundleLoader::default()
}

pub async fn cloud_config_bundle_loader_for_storage(
    _codex_home: PathBuf,
    _enable_codex_api_key_env: bool,
    _credentials_store_mode: AuthCredentialsStoreMode,
    _chatgpt_base_url: String,
) -> CloudConfigBundleLoader {
    CloudConfigBundleLoader::default()
}
