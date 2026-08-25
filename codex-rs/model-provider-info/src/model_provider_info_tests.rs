use super::*;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_absolute_path::AbsolutePathBufGuard;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::BTreeMap;
use std::num::NonZeroU64;
use tempfile::tempdir;

#[test]
fn test_deserialize_ollama_model_provider_toml() {
    let azure_provider_toml = r#"
name = "Ollama"
base_url = "http://localhost:11434/v1"
        "#;
    let expected_provider = ModelProviderInfo {
        name: "Ollama".into(),
        base_url: Some("http://localhost:11434/v1".into()),
        env_key: None,
        env_key_instructions: None,
        experimental_bearer_token: None,
        auth: None,
        aws: None,
        managed_auth: None,
        wire_api: WireApi::ChatCompletions,
        responses_builtin_tools: Default::default(),
        provider_flavor: None,
        query_params: None,
        request_body: None,
        request_body_remove: Vec::new(),
        http_headers: None,
        env_http_headers: None,
        request_max_retries: None,
        stream_max_retries: None,
        stream_idle_timeout_ms: None,
        websocket_connect_timeout_ms: None,
        requires_astral_auth: false,
        supports_websockets: false,
    };

    let provider: ModelProviderInfo = toml::from_str(azure_provider_toml).unwrap();
    assert_eq!(expected_provider, provider);
}

#[test]
fn test_deserialize_azure_model_provider_toml() {
    let azure_provider_toml = r#"
name = "Azure"
base_url = "https://xxxxx.openai.azure.com/openai"
env_key = "AZURE_OPENAI_API_KEY"
query_params = { api-version = "2025-04-01-preview" }
        "#;
    let expected_provider = ModelProviderInfo {
        name: "Azure".into(),
        base_url: Some("https://xxxxx.openai.azure.com/openai".into()),
        env_key: Some("AZURE_OPENAI_API_KEY".into()),
        env_key_instructions: None,
        experimental_bearer_token: None,
        auth: None,
        aws: None,
        managed_auth: None,
        wire_api: WireApi::ChatCompletions,
        responses_builtin_tools: Default::default(),
        provider_flavor: None,
        query_params: Some(maplit::hashmap! {
            "api-version".to_string() => "2025-04-01-preview".to_string(),
        }),
        request_body: None,
        request_body_remove: Vec::new(),
        http_headers: None,
        env_http_headers: None,
        request_max_retries: None,
        stream_max_retries: None,
        stream_idle_timeout_ms: None,
        websocket_connect_timeout_ms: None,
        requires_astral_auth: false,
        supports_websockets: false,
    };

    let provider: ModelProviderInfo = toml::from_str(azure_provider_toml).unwrap();
    assert_eq!(expected_provider, provider);
}

#[test]
fn test_deserialize_example_model_provider_toml() {
    let azure_provider_toml = r#"
name = "Example"
base_url = "https://example.com"
env_key = "API_KEY"
http_headers = { "X-Example-Header" = "example-value" }
env_http_headers = { "X-Example-Env-Header" = "EXAMPLE_ENV_VAR" }
        "#;
    let expected_provider = ModelProviderInfo {
        name: "Example".into(),
        base_url: Some("https://example.com".into()),
        env_key: Some("API_KEY".into()),
        env_key_instructions: None,
        experimental_bearer_token: None,
        auth: None,
        aws: None,
        managed_auth: None,
        wire_api: WireApi::ChatCompletions,
        responses_builtin_tools: Default::default(),
        provider_flavor: None,
        query_params: None,
        request_body: None,
        request_body_remove: Vec::new(),
        http_headers: Some(maplit::hashmap! {
            "X-Example-Header".to_string() => "example-value".to_string(),
        }),
        env_http_headers: Some(maplit::hashmap! {
            "X-Example-Env-Header".to_string() => "EXAMPLE_ENV_VAR".to_string(),
        }),
        request_max_retries: None,
        stream_max_retries: None,
        stream_idle_timeout_ms: None,
        websocket_connect_timeout_ms: None,
        requires_astral_auth: false,
        supports_websockets: false,
    };

    let provider: ModelProviderInfo = toml::from_str(azure_provider_toml).unwrap();
    assert_eq!(expected_provider, provider);
}

#[test]
fn test_deserialize_provider_request_body_config() {
    let provider_toml = r#"
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
env_key = "DEEPSEEK_API_KEY"
request_body_remove = ["stream_options", "parallel_tool_calls"]

[request_body]
temperature = 0.2
top_p = 0.9
enable_thinking = true
metadata = { app = "astral-code" }
        "#;
    let provider: ModelProviderInfo = toml::from_str(provider_toml).unwrap();

    assert_eq!(
        provider.request_body,
        Some(BTreeMap::from([
            ("enable_thinking".to_string(), json!(true)),
            ("metadata".to_string(), json!({ "app": "astral-code" })),
            ("temperature".to_string(), json!(0.2)),
            ("top_p".to_string(), json!(0.9)),
        ]))
    );
    assert_eq!(
        provider.request_body_remove,
        vec![
            "stream_options".to_string(),
            "parallel_tool_calls".to_string()
        ]
    );
}

#[test]
fn test_deserialize_provider_neutral_wire_apis() {
    let anthropic_provider_toml = r#"
name = "Anthropic"
base_url = "https://api.anthropic.com/v1"
env_key = "ANTHROPIC_API_KEY"
wire_api = "anthropic_messages"
        "#;
    let chat_provider_toml = r#"
name = "OpenAI-compatible chat"
base_url = "https://example.com/v1"
env_key = "EXAMPLE_API_KEY"
wire_api = "chat_completions"
        "#;

    let anthropic_provider: ModelProviderInfo = toml::from_str(anthropic_provider_toml).unwrap();
    let chat_provider: ModelProviderInfo = toml::from_str(chat_provider_toml).unwrap();

    assert_eq!(anthropic_provider.wire_api, WireApi::AnthropicMessages);
    assert_eq!(chat_provider.wire_api, WireApi::ChatCompletions);
    assert_eq!(WireApi::AnthropicMessages.to_string(), "anthropic_messages");
    assert_eq!(WireApi::ChatCompletions.to_string(), "chat_completions");
}

#[test]
fn to_api_provider_retries_429_by_default() {
    let provider = ModelProviderInfo {
        name: "OpenAI-compatible chat".to_string(),
        base_url: Some("https://example.com/v1".to_string()),
        ..ModelProviderInfo::default()
    };

    let api_provider = provider
        .to_api_provider(/*auth_mode*/ None)
        .expect("provider should convert");

    assert!(api_provider.retry.retry_429);
}

#[test]
fn test_deserialize_provider_flavor_override() {
    let provider_toml = r#"
name = "Custom DeepSeek Gateway"
base_url = "https://example.com/v1"
provider_flavor = "deepseek"
        "#;

    let provider: ModelProviderInfo = toml::from_str(provider_toml).unwrap();

    assert_eq!(provider.provider_flavor, Some(ProviderFlavor::DeepSeek));
    assert_eq!(
        provider.effective_provider_flavor(),
        ProviderFlavor::DeepSeek
    );
    assert_eq!(ProviderFlavor::DeepSeek.to_string(), "deepseek");
}

#[test]
fn test_provider_flavor_serializes_to_config_wire_values() {
    let provider = ModelProviderInfo {
        provider_flavor: Some(ProviderFlavor::DeepSeek),
        ..Default::default()
    };

    let serialized = toml::to_string(&provider).unwrap();
    let parsed: ModelProviderInfo = toml::from_str(&serialized).unwrap();

    assert!(serialized.contains(r#"provider_flavor = "deepseek""#));
    assert_eq!(parsed, provider);
}

#[test]
fn test_infer_provider_flavor_from_name_or_base_url() {
    let cases = [
        (
            ModelProviderInfo {
                name: "DeepSeek".to_string(),
                base_url: Some("https://api.deepseek.com/v1".to_string()),
                ..ModelProviderInfo::default()
            },
            ProviderFlavor::DeepSeek,
        ),
        (
            ModelProviderInfo {
                name: "OpenRouter".to_string(),
                base_url: Some("https://openrouter.ai/api/v1".to_string()),
                ..ModelProviderInfo::default()
            },
            ProviderFlavor::OpenRouter,
        ),
        (
            ModelProviderInfo {
                name: "DashScope Qwen".to_string(),
                base_url: Some("https://dashscope.aliyuncs.com/compatible-mode/v1".to_string()),
                ..ModelProviderInfo::default()
            },
            ProviderFlavor::EnableThinking,
        ),
        (
            ModelProviderInfo {
                name: "GLM".to_string(),
                base_url: Some("https://open.bigmodel.cn/api/paas/v4".to_string()),
                ..ModelProviderInfo::default()
            },
            ProviderFlavor::ThinkingType,
        ),
        (
            ModelProviderInfo {
                name: "MiniMax".to_string(),
                base_url: Some("https://api.minimax.chat/v1".to_string()),
                ..ModelProviderInfo::default()
            },
            ProviderFlavor::MiniMax,
        ),
    ];

    assert_eq!(
        cases
            .into_iter()
            .map(|(provider, expected)| (provider.effective_provider_flavor(), expected))
            .collect::<Vec<_>>(),
        vec![
            (ProviderFlavor::DeepSeek, ProviderFlavor::DeepSeek),
            (ProviderFlavor::OpenRouter, ProviderFlavor::OpenRouter),
            (
                ProviderFlavor::EnableThinking,
                ProviderFlavor::EnableThinking
            ),
            (ProviderFlavor::ThinkingType, ProviderFlavor::ThinkingType),
            (ProviderFlavor::MiniMax, ProviderFlavor::MiniMax),
        ]
    );
}

#[test]
fn test_deserialize_chat_wire_api_shows_helpful_error() {
    let provider_toml = r#"
name = "OpenAI using Chat Completions"
base_url = "https://api.openai.com/v1"
env_key = "OPENAI_API_KEY"
wire_api = "chat"
        "#;

    let err = toml::from_str::<ModelProviderInfo>(provider_toml).unwrap_err();
    assert!(err.to_string().contains(CHAT_WIRE_API_REMOVED_ERROR));
}

#[test]
fn test_create_astral_provider_defaults_to_chat_completions() {
    let expected_base_url = std::env::var(ASTRAL_BASE_URL_ENV_VAR)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    assert_eq!(
        ModelProviderInfo::create_astral_provider(),
        ModelProviderInfo {
            name: "Astral".into(),
            base_url: expected_base_url,
            env_key: Some(ASTRAL_API_KEY_ENV_VAR.to_string()),
            env_key_instructions: Some(format!(
                "Set {ASTRAL_API_KEY_ENV_VAR} for the active Astral model provider."
            )),
            experimental_bearer_token: None,
            auth: None,
            aws: None,
            managed_auth: None,
            wire_api: WireApi::ChatCompletions,
            responses_builtin_tools: Default::default(),
            provider_flavor: None,
            query_params: None,
            request_body: None,
            request_body_remove: Vec::new(),
            http_headers: Some(maplit::hashmap! {
                "version".to_string() => env!("CARGO_PKG_VERSION").to_string(),
            }),
            env_http_headers: None,
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_astral_auth: false,
            supports_websockets: false,
        }
    );
}

#[test]
fn test_deserialize_websocket_connect_timeout() {
    let provider_toml = r#"
name = "OpenAI"
base_url = "https://api.openai.com/v1"
websocket_connect_timeout_ms = 15000
        "#;

    let provider: ModelProviderInfo = toml::from_str(provider_toml).unwrap();
    assert_eq!(provider.websocket_connect_timeout_ms, Some(15_000));
}

#[test]
fn test_legacy_builtin_tool_policy_parses_but_validation_requires_adapter() {
    let provider_toml = r#"
name = "OpenAI"
base_url = "https://api.openai.com/v1"
wire_api = "responses"
responses_builtin_tools = ["web_search"]
        "#;

    let provider = toml::from_str::<ModelProviderInfo>(provider_toml)
        .expect("responses provider config should parse");

    assert_eq!(
        provider,
        ModelProviderInfo {
            name: "OpenAI".to_string(),
            base_url: Some("https://api.openai.com/v1".to_string()),
            wire_api: WireApi::Responses,
            responses_builtin_tools: ResponsesBuiltinTools::Selected(vec![
                "web_search".to_string()
            ]),
            ..ModelProviderInfo::default()
        }
    );
    assert_eq!(
        provider.validate(),
        Err(
            "provider responses_builtin_tools is no longer supported; use a built-in provider adapter for hosted tools"
                .to_string()
        )
    );
}

#[test]
fn test_custom_provider_without_base_url_returns_configuration_error() {
    let err = ModelProviderInfo {
        name: "Custom".to_string(),
        base_url: None,
        ..ModelProviderInfo::default()
    }
    .to_api_provider(/*auth_mode*/ None)
    .expect_err("custom provider without base_url should fail");

    assert!(
        err.to_string().contains("has no base_url"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_auth_mode_does_not_route_provider_to_chatgpt_codex_backend() {
    let api_provider =
        ModelProviderInfo::create_openai_provider(Some("https://models.example/v1".to_string()))
            .to_api_provider(Some(codex_app_server_protocol::AuthMode::ApiKey))
            .expect("provider should build API provider");

    assert_eq!(api_provider.base_url, "https://models.example/v1");
}

#[test]
fn test_deserialize_provider_auth_config_defaults() {
    let base_dir = tempdir().unwrap();
    let provider_toml = r#"
name = "Corp"

[auth]
command = "./scripts/print-token"
args = ["--format=text"]
        "#;

    let provider: ModelProviderInfo = {
        let _guard = AbsolutePathBufGuard::new(base_dir.path());
        toml::from_str(provider_toml).unwrap()
    };

    assert_eq!(
        provider.auth,
        Some(ModelProviderAuthInfo {
            command: "./scripts/print-token".to_string(),
            args: vec!["--format=text".to_string()],
            timeout_ms: NonZeroU64::new(5_000).unwrap(),
            refresh_interval_ms: 300_000,
            cwd: AbsolutePathBuf::resolve_path_against_base(".", base_dir.path()),
        })
    );
}

#[test]
fn test_deserialize_provider_aws_config() {
    let provider_toml = r#"
name = "Amazon Bedrock"
base_url = "https://bedrock.example.com/v1"

[aws]
profile = "codex-bedrock"
region = "us-west-2"
        "#;

    let provider: ModelProviderInfo = toml::from_str(provider_toml).unwrap();

    assert_eq!(
        provider.aws,
        Some(ModelProviderAwsAuthInfo {
            profile: Some("codex-bedrock".to_string()),
            region: Some("us-west-2".to_string()),
        })
    );
}

#[test]
fn test_create_amazon_bedrock_provider() {
    assert_eq!(
        ModelProviderInfo::create_amazon_bedrock_provider(/*aws*/ None),
        ModelProviderInfo {
            name: "Amazon Bedrock".to_string(),
            base_url: Some("https://bedrock-mantle.us-east-1.api.aws/openai/v1".to_string()),
            env_key: None,
            env_key_instructions: None,
            experimental_bearer_token: None,
            auth: None,
            aws: Some(ModelProviderAwsAuthInfo {
                profile: None,
                region: None,
            }),
            managed_auth: None,
            wire_api: WireApi::ChatCompletions,
            responses_builtin_tools: Default::default(),
            provider_flavor: None,
            query_params: None,
            request_body: None,
            request_body_remove: Vec::new(),
            http_headers: Some(maplit::hashmap! {
                AMAZON_BEDROCK_MANTLE_CLIENT_AGENT_HEADER.to_string() =>
                    AMAZON_BEDROCK_MANTLE_CLIENT_AGENT_VALUE.to_string(),
            }),
            env_http_headers: None,
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_astral_auth: false,
            supports_websockets: false,
        }
    );
}

#[test]
fn test_amazon_bedrock_provider_adds_mantle_client_agent_header() {
    let api_provider = ModelProviderInfo::create_amazon_bedrock_provider(/*aws*/ None)
        .to_api_provider(/*auth_mode*/ None)
        .expect("Amazon Bedrock provider should build API provider");

    assert_eq!(
        api_provider
            .headers
            .get(AMAZON_BEDROCK_MANTLE_CLIENT_AGENT_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some(AMAZON_BEDROCK_MANTLE_CLIENT_AGENT_VALUE)
    );
}

#[test]
fn test_built_in_model_providers_include_amazon_bedrock() {
    let providers = built_in_model_providers();

    assert_eq!(
        providers
            .get(AMAZON_BEDROCK_PROVIDER_ID)
            .map(ModelProviderInfo::is_amazon_bedrock),
        Some(true)
    );
}

#[test]
fn test_built_in_model_providers_include_astral() {
    let providers = built_in_model_providers();

    assert_eq!(
        providers
            .get(ASTRAL_PROVIDER_ID)
            .map(ModelProviderInfo::is_astral),
        Some(true)
    );
}

#[test]
fn test_built_in_model_providers_include_anthropic() {
    let providers = built_in_model_providers();

    assert_eq!(
        providers
            .get(ANTHROPIC_PROVIDER_ID)
            .map(|provider| (provider.is_anthropic(), provider.wire_api)),
        Some((true, WireApi::AnthropicMessages))
    );
}

#[test]
fn test_built_in_oss_providers_default_to_chat_completions() {
    let providers = built_in_model_providers();

    assert_eq!(
        providers
            .get(OLLAMA_OSS_PROVIDER_ID)
            .map(|provider| provider.wire_api),
        Some(WireApi::ChatCompletions)
    );
    assert_eq!(
        providers
            .get(LMSTUDIO_OSS_PROVIDER_ID)
            .map(|provider| provider.wire_api),
        Some(WireApi::ChatCompletions)
    );
}

#[test]
fn test_merge_configured_model_providers_adds_custom_provider() {
    let custom_provider = ModelProviderInfo {
        name: "Custom".to_string(),
        base_url: Some("https://example.com/v1".to_string()),
        ..ModelProviderInfo::default()
    };
    let configured_model_providers =
        std::collections::HashMap::from([("custom".to_string(), custom_provider.clone())]);

    let mut expected = built_in_model_providers();
    expected.insert("custom".to_string(), custom_provider);

    assert_eq!(
        merge_configured_model_providers(built_in_model_providers(), configured_model_providers),
        Ok(expected)
    );
}

#[test]
fn test_merge_configured_model_providers_overrides_bootstrap_provider() {
    let custom_astral = ModelProviderInfo {
        name: "Custom Astral".to_string(),
        base_url: Some("https://models.example/v1".to_string()),
        wire_api: WireApi::AnthropicMessages,
        ..ModelProviderInfo::default()
    };
    let configured_model_providers =
        std::collections::HashMap::from([(ASTRAL_PROVIDER_ID.to_string(), custom_astral.clone())]);

    let mut expected = built_in_model_providers();
    expected.insert(ASTRAL_PROVIDER_ID.to_string(), custom_astral);

    assert_eq!(
        merge_configured_model_providers(built_in_model_providers(), configured_model_providers),
        Ok(expected)
    );
}

#[test]
fn test_validate_provider_aws_rejects_conflicting_auth() {
    let provider = ModelProviderInfo {
        aws: Some(ModelProviderAwsAuthInfo {
            profile: None,
            region: None,
        }),
        env_key: Some("AWS_BEARER_TOKEN_BEDROCK".to_string()),
        supports_websockets: false,
        ..ModelProviderInfo::create_openai_provider(/*base_url*/ None)
    };

    assert_eq!(
        provider.validate(),
        Err("provider aws cannot be combined with env_key".to_string())
    );
}

#[test]
fn test_validate_provider_aws_rejects_websockets() {
    let provider = ModelProviderInfo {
        aws: Some(ModelProviderAwsAuthInfo {
            profile: None,
            region: None,
        }),
        requires_astral_auth: false,
        supports_websockets: true,
        ..ModelProviderInfo::create_openai_provider(/*base_url*/ None)
    };

    assert_eq!(
        provider.validate(),
        Err(
            "provider supports_websockets is not supported; use streaming Responses over HTTP"
                .to_string()
        )
    );
}

#[test]
fn test_deserialize_provider_auth_config_allows_zero_refresh_interval() {
    let base_dir = tempdir().unwrap();
    let provider_toml = r#"
name = "Corp"

[auth]
command = "./scripts/print-token"
refresh_interval_ms = 0
        "#;

    let provider: ModelProviderInfo = {
        let _guard = AbsolutePathBufGuard::new(base_dir.path());
        toml::from_str(provider_toml).unwrap()
    };

    let auth = provider.auth.expect("auth config should deserialize");
    assert_eq!(auth.refresh_interval_ms, 0);
    assert_eq!(auth.refresh_interval(), None);
}
