use codex_api::ExternalWebAccess;
use codex_api::ExternalWebAccessMode;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ToolName;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::config_types::WebSearchMode;
use pretty_assertions::assert_eq;

use super::CodexWebSearchExtensionConfig;
use super::Config;
use super::external_web_access_for_mode;
use super::install;
use crate::codex_tool::RUN_TOOL_NAME;
use crate::codex_tool::WEB_NAMESPACE;

#[test]
fn external_web_access_preserves_indexed_mode() {
    assert_eq!(
        [
            WebSearchMode::Disabled,
            WebSearchMode::Cached,
            WebSearchMode::Indexed,
            WebSearchMode::Live,
        ]
        .map(external_web_access_for_mode),
        [
            ExternalWebAccess::Boolean(false),
            ExternalWebAccess::Boolean(false),
            ExternalWebAccess::Mode(ExternalWebAccessMode::Indexed),
            ExternalWebAccess::Boolean(true),
        ]
    );
}

#[test]
fn installed_extension_contributes_web_run_for_codex_oauth() {
    let mut builder = ExtensionRegistryBuilder::<Config>::new();
    install(
        &mut builder,
        AuthManager::from_auth_for_testing(CodexAuth::from_api_key("dummy")),
    );
    let registry = builder.build();
    let session_store = ExtensionData::new("session");
    let thread_store = ExtensionData::new("11111111-1111-4111-8111-111111111111");
    thread_store.insert(CodexWebSearchExtensionConfig {
        available: true,
        provider: ModelProviderInfo::create_codex_provider(),
        settings: Default::default(),
    });

    let tool_names = registry
        .tool_contributors()
        .iter()
        .flat_map(|contributor| contributor.tools(&session_store, &thread_store))
        .map(|tool| (tool.tool_name(), tool.supports_parallel_tool_calls()))
        .collect::<Vec<_>>();

    assert_eq!(
        tool_names,
        vec![(ToolName::namespaced(WEB_NAMESPACE, RUN_TOOL_NAME), true)]
    );
}
