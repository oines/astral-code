use crate::ModelsManagerConfig;
use crate::capabilities::ModelCapabilitiesCache;
use crate::capabilities::ModelCapability;
use crate::manager::ModelsManager;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ToolMode;
use codex_protocol::openai_models::TruncationPolicyConfig;
use codex_utils_output_truncation::approx_bytes_for_tokens;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;
use tempfile::TempDir;

use super::TestModelsEndpoint;
use super::openai_manager_for_tests;
use super::remote_model;
use super::static_manager_for_tests;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_model_info_without_tool_output_override() {
    let codex_home = TempDir::new().expect("create temp dir");
    let config = ModelsManagerConfig::default();
    let manager = openai_manager_for_tests(
        codex_home.path().to_path_buf(),
        TestModelsEndpoint::new(Vec::new()),
    );

    let model_info = manager.get_model_info("gpt-5.2", &config).await;

    assert_eq!(
        model_info.truncation_policy,
        TruncationPolicyConfig::bytes(/*limit*/ 10_000)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_model_info_with_tool_output_override() {
    let codex_home = TempDir::new().expect("create temp dir");
    let config = ModelsManagerConfig {
        tool_output_token_limit: Some(123),
        ..Default::default()
    };
    let manager = openai_manager_for_tests(
        codex_home.path().to_path_buf(),
        TestModelsEndpoint::new(Vec::new()),
    );

    let model_info = manager.get_model_info("unknown-model", &config).await;

    assert_eq!(
        model_info.truncation_policy,
        TruncationPolicyConfig::bytes(
            i64::try_from(approx_bytes_for_tokens(/*tokens*/ 123)).expect("fits i64")
        )
    );
}

#[tokio::test]
async fn model_capability_tool_mode_overrides_catalog_metadata() {
    let model_slug = "configured-code-mode";
    let mut catalog_model = remote_model(model_slug, "Configured Code Mode", /*priority*/ 0);
    catalog_model.tool_mode = Some(ToolMode::Direct);
    let mut expected = catalog_model.clone();
    expected.tool_mode = Some(ToolMode::CodeMode);
    let config = ModelsManagerConfig {
        model_capabilities: Some(ModelCapabilitiesCache {
            version: 1,
            source: "test".to_string(),
            generated_at_unix_seconds: 0,
            models: BTreeMap::from([(
                model_slug.to_string(),
                ModelCapability {
                    tool_mode: Some(ToolMode::CodeMode),
                    ..Default::default()
                },
            )]),
        }),
        ..Default::default()
    };
    let manager = static_manager_for_tests(ModelsResponse {
        models: vec![catalog_model],
    });

    assert_eq!(manager.get_model_info(model_slug, &config).await, expected);
}
