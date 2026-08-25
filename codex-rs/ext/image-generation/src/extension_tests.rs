use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ToolName;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_model_provider_info::ModelProviderInfo;
use pretty_assertions::assert_eq;

use super::Config;
use super::ImageGenerationExtensionConfig;
use super::install;
use crate::IMAGE_GEN_NAMESPACE;
use crate::IMAGEGEN_TOOL_NAME;

#[test]
fn installed_extension_contributes_imagegen_for_codex_oauth() {
    let mut builder = ExtensionRegistryBuilder::<Config>::new();
    install(
        &mut builder,
        AuthManager::from_auth_for_testing(CodexAuth::from_api_key("dummy")),
    );
    let registry = builder.build();
    let session_store = ExtensionData::new("session");
    let thread_store = ExtensionData::new("11111111-1111-4111-8111-111111111111");
    thread_store.insert(ImageGenerationExtensionConfig {
        available: true,
        provider: ModelProviderInfo::create_codex_provider(),
        codex_home: "/tmp/astral-imagegen-test"
            .try_into()
            .expect("test path should be absolute"),
    });

    let tool_names = registry
        .tool_contributors()
        .iter()
        .flat_map(|contributor| contributor.tools(&session_store, &thread_store))
        .map(|tool| tool.tool_name())
        .collect::<Vec<_>>();

    assert_eq!(
        tool_names,
        vec![ToolName::namespaced(
            IMAGE_GEN_NAMESPACE,
            IMAGEGEN_TOOL_NAME
        )]
    );
}
