use codex_features::FEATURES;
use codex_features::Feature;
use codex_models_manager::capabilities::MODEL_CAPABILITIES_FILE_NAME;
use std::collections::BTreeMap;
use std::path::Path;

use crate::models_cache::write_models_cache;

pub fn write_mock_responses_config_toml(
    codex_home: &Path,
    server_uri: &str,
    feature_flags: &BTreeMap<Feature, bool>,
    auto_compact_limit: i64,
    requires_astral_auth: Option<bool>,
    model_provider_id: &str,
    compact_prompt: &str,
) -> std::io::Result<()> {
    // Phase 1: build the features block for config.toml.
    let mut features = BTreeMap::new();
    for (feature, enabled) in feature_flags {
        features.insert(*feature, *enabled);
    }
    let feature_entries = features
        .into_iter()
        .map(|(feature, enabled)| {
            let key = FEATURES
                .iter()
                .find(|spec| spec.id == feature)
                .map(|spec| spec.key)
                .unwrap_or_else(|| panic!("missing feature key for {feature:?}"));
            format!("{key} = {enabled}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    // Phase 2: build provider-specific config bits.
    let _ = requires_astral_auth;
    let requires_line = String::new();
    let provider_name = if matches!(requires_astral_auth, Some(true)) {
        "OpenAI"
    } else {
        "Mock provider for test"
    };
    let provider_block = format!(
        r#"
[model_providers.{model_provider_id}]
name = "{provider_name}"
base_url = "{server_uri}/v1"
wire_api = "chat_completions"
request_max_retries = 0
stream_max_retries = 0
supports_websockets = false
{requires_line}
"#
    );
    let model_capabilities = mock_model_capabilities_toml(model_provider_id);
    let openai_base_url_line = if model_provider_id == "openai" {
        format!("openai_base_url = \"{server_uri}/v1\"\n")
    } else {
        String::new()
    };
    // Phase 3: write the final config file.
    let config_toml = codex_home.join("config.toml");
    std::fs::write(
        config_toml,
        format!(
            r#"
model = "mock-model"
approval_policy = "never"
sandbox_mode = "read-only"
compact_prompt = "{compact_prompt}"
model_auto_compact_token_limit = {auto_compact_limit}

model_provider = "{model_provider_id}"
{openai_base_url_line}

[features]
{feature_entries}
{provider_block}
{model_capabilities}
"#
        ),
    )?;
    write_mock_model_capabilities_cache(codex_home, model_provider_id)?;
    write_models_cache(codex_home)
}

pub fn write_mock_responses_config_toml_with_hosted_base_url(
    codex_home: &Path,
    server_uri: &str,
    hosted_base_url: &str,
) -> std::io::Result<()> {
    let config_toml = codex_home.join("config.toml");
    let model_capabilities = mock_model_capabilities_toml("mock_provider");
    std::fs::write(
        config_toml,
        format!(
            r#"
model = "mock-model"
approval_policy = "never"
sandbox_mode = "read-only"
hosted_base_url = "{hosted_base_url}"

model_provider = "mock_provider"

[model_providers.mock_provider]
name = "Mock provider for test"
base_url = "{server_uri}/v1"
wire_api = "chat_completions"
request_max_retries = 0
stream_max_retries = 0
{model_capabilities}
"#
        ),
    )?;
    write_mock_model_capabilities_cache(codex_home, "mock_provider")?;
    write_models_cache(codex_home)
}

fn mock_model_capabilities_toml(model_provider_id: &str) -> String {
    MOCK_MODELS
        .into_iter()
        .map(|model| {
            format!(
                r#"
[model_capabilities."{model_provider_id}/{model}"]
max_context_window = 272000
max_output_tokens = 32000
supports_tools = true
supports_vision = true
supports_reasoning = true
"#
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

pub(crate) fn write_default_test_model_capabilities_cache(
    codex_home: &Path,
) -> std::io::Result<()> {
    let providers = ["mock_provider", "openai", "astral"];
    let models = providers
        .into_iter()
        .map(mock_model_capabilities_cache_toml)
        .collect::<Vec<_>>()
        .join("");
    write_model_capabilities_cache(codex_home, models)
}

fn write_mock_model_capabilities_cache(
    codex_home: &Path,
    model_provider_id: &str,
) -> std::io::Result<()> {
    write_model_capabilities_cache(
        codex_home,
        mock_model_capabilities_cache_toml(model_provider_id),
    )
}

fn write_model_capabilities_cache(codex_home: &Path, models: String) -> std::io::Result<()> {
    std::fs::write(
        codex_home.join(MODEL_CAPABILITIES_FILE_NAME),
        format!(
            r#"version = 1
source = "app-server-tests"
generated_at_unix_seconds = 0
{models}
"#
        ),
    )
}

fn mock_model_capabilities_cache_toml(model_provider_id: &str) -> String {
    MOCK_MODELS
        .into_iter()
        .map(|model| {
            format!(
                r#"
[models."{model_provider_id}/{model}"]
max_context_window = 272000
max_output_tokens = 32000
supports_tools = true
supports_vision = true
supports_reasoning = true
"#
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

const MOCK_MODELS: &[&str] = &[
    "mock-model",
    "mock-model-collab",
    "mock-model-override",
    "mock-model-3",
    "mock-model-4",
    "gpt-5.2",
    "gpt-5.2-codex",
    "gpt-5.3-codex",
    "gpt-5.4",
];
