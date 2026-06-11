use std::path::Path;
use std::path::PathBuf;

use codex_utils_absolute_path::AbsolutePathBuf;

use super::RemoteMarketplaceSource;
use super::RemotePluginCatalogError;
use super::RemotePluginServiceConfig;
use super::RemotePluginShareAccessPolicy;
use super::fetch_remote_marketplaces;
use super::save_remote_plugin_share;
use super::sync_remote_installed_plugin_bundles_once;

fn service_config() -> RemotePluginServiceConfig {
    RemotePluginServiceConfig {
        hosted_base_url: "https://chatgpt.example/backend-api".to_string(),
    }
}

#[tokio::test]
async fn fetch_remote_marketplaces_returns_control_plane_disabled_before_auth() {
    let err = fetch_remote_marketplaces(
        &service_config(),
        /*auth*/ None,
        &[RemoteMarketplaceSource::Global],
        /*global_catalog_cache_path*/ None,
    )
    .await
    .expect_err("hosted remote plugin catalog should be disabled");

    assert!(matches!(
        err,
        RemotePluginCatalogError::ControlPlaneDisabled
    ));
}

#[tokio::test]
async fn save_remote_plugin_share_returns_control_plane_disabled_before_archiving() {
    let plugin_path =
        AbsolutePathBuf::try_from(PathBuf::from("/tmp/astral-disabled-remote-plugin-share"))
            .expect("absolute plugin path");

    let err = save_remote_plugin_share(
        &service_config(),
        /*auth*/ None,
        Path::new("/tmp/astral-disabled-codex-home"),
        &plugin_path,
        /*remote_plugin_id*/ None,
        RemotePluginShareAccessPolicy::default(),
    )
    .await
    .expect_err("hosted remote plugin sharing should be disabled");

    assert!(matches!(
        err,
        RemotePluginCatalogError::ControlPlaneDisabled
    ));
}

#[tokio::test]
async fn sync_remote_installed_plugin_bundles_returns_control_plane_disabled_before_auth() {
    let err = sync_remote_installed_plugin_bundles_once(
        PathBuf::from("/tmp/astral-disabled-codex-home"),
        &service_config(),
        /*auth*/ None,
    )
    .await
    .expect_err("hosted remote installed plugin sync should be disabled");

    assert!(matches!(
        err,
        super::RemoteInstalledPluginBundleSyncError::Catalog(
            RemotePluginCatalogError::ControlPlaneDisabled
        )
    ));
}
