use codex_protocol::protocol::Product;

use super::RemotePluginFetchError;
use super::RemotePluginMutationError;
use super::enable_remote_plugin;
use super::fetch_remote_featured_plugin_ids;
use super::uninstall_remote_plugin;
use crate::remote::RemotePluginServiceConfig;

fn service_config() -> RemotePluginServiceConfig {
    RemotePluginServiceConfig {
        hosted_base_url: "https://hosted.example/api".to_string(),
    }
}

#[tokio::test]
async fn fetch_remote_featured_plugin_ids_is_disabled() {
    let err = fetch_remote_featured_plugin_ids(
        &service_config(),
        /*auth*/ None,
        Some(Product::Codex),
    )
    .await
    .expect_err("legacy featured plugin fetch should be disabled");

    assert!(matches!(err, RemotePluginFetchError::ControlPlaneDisabled));
}

#[tokio::test]
async fn enable_remote_plugin_is_disabled() {
    let err = enable_remote_plugin(&service_config(), /*auth*/ None, "plugin_test")
        .await
        .expect_err("legacy remote plugin enable should be disabled");

    assert!(matches!(
        err,
        RemotePluginMutationError::ControlPlaneDisabled
    ));
}

#[tokio::test]
async fn uninstall_remote_plugin_is_disabled() {
    let err = uninstall_remote_plugin(&service_config(), /*auth*/ None, "plugin_test")
        .await
        .expect_err("legacy remote plugin uninstall should be disabled");

    assert!(matches!(
        err,
        RemotePluginMutationError::ControlPlaneDisabled
    ));
}
