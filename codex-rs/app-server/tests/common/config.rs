use codex_features::FEATURES;
use codex_features::Feature;
use std::collections::BTreeMap;
use std::path::Path;

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
"#
        ),
    )
}

pub fn write_mock_responses_config_toml_with_hosted_base_url(
    codex_home: &Path,
    server_uri: &str,
    hosted_base_url: &str,
) -> std::io::Result<()> {
    let config_toml = codex_home.join("config.toml");
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
"#
        ),
    )
}
