//! Registry of model providers supported by Astral.
//!
//! Providers can be defined in two places:
//!   1. Minimal bootstrap providers compiled into the binary.
//!   2. User-defined entries inside `~/.astral-code/config.toml` under the `model_providers`
//!      key. These override or extend the bootstrap entries at runtime.

use codex_api::Provider as ApiProvider;
use codex_api::RetryConfig as ApiRetryConfig;
use codex_protocol::config_types::ModelProviderAuthInfo;
use codex_protocol::error::CodexErr;
use codex_protocol::error::EnvVarError;
use codex_protocol::error::Result as CodexResult;
use http::HeaderMap;
use http::header::HeaderName;
use http::header::HeaderValue;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

const DEFAULT_STREAM_IDLE_TIMEOUT_MS: u64 = 300_000;
const DEFAULT_STREAM_MAX_RETRIES: u64 = 5;
const DEFAULT_REQUEST_MAX_RETRIES: u64 = 4;
pub const DEFAULT_WEBSOCKET_CONNECT_TIMEOUT_MS: u64 = 15_000;
/// Hard cap for user-configured `stream_max_retries`.
const MAX_STREAM_MAX_RETRIES: u64 = 100;
/// Hard cap for user-configured `request_max_retries`.
const MAX_REQUEST_MAX_RETRIES: u64 = 100;

const OPENAI_PROVIDER_NAME: &str = "OpenAI";
pub const OPENAI_PROVIDER_ID: &str = "openai";
const ASTRAL_PROVIDER_NAME: &str = "Astral";
pub const ASTRAL_PROVIDER_ID: &str = "astral";
pub const ASTRAL_API_KEY_ENV_VAR: &str = "ASTRAL_API_KEY";
pub const ASTRAL_BASE_URL_ENV_VAR: &str = "ASTRAL_BASE_URL";
const ASTRAL_OSS_BASE_URL_ENV_VAR: &str = "ASTRAL_OSS_BASE_URL";
const ASTRAL_OSS_PORT_ENV_VAR: &str = "ASTRAL_OSS_PORT";
const ANTHROPIC_PROVIDER_NAME: &str = "Anthropic";
pub const ANTHROPIC_PROVIDER_ID: &str = "anthropic";
pub const ANTHROPIC_API_KEY_ENV_VAR: &str = "ANTHROPIC_API_KEY";
const ANTHROPIC_DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1";
const AMAZON_BEDROCK_PROVIDER_NAME: &str = "Amazon Bedrock";
pub const AMAZON_BEDROCK_PROVIDER_ID: &str = "amazon-bedrock";
pub const AMAZON_BEDROCK_GPT_5_5_MODEL_ID: &str = "openai.gpt-5.5";
pub const AMAZON_BEDROCK_GPT_5_4_MODEL_ID: &str = "openai.gpt-5.4";
pub const AMAZON_BEDROCK_DEFAULT_BASE_URL: &str =
    "https://bedrock-mantle.us-east-1.api.aws/openai/v1";
const AMAZON_BEDROCK_MANTLE_CLIENT_AGENT_HEADER: &str = "x-amzn-mantle-client-agent";
const AMAZON_BEDROCK_MANTLE_CLIENT_AGENT_VALUE: &str = "codex";
const CHAT_WIRE_API_REMOVED_ERROR: &str = "`wire_api = \"chat\"` is no longer supported.\nHow to fix: set `wire_api = \"chat_completions\"` in your provider config.";
const WIRE_API_VARIANTS: &[&str] = &["responses", "anthropic_messages", "chat_completions"];
const PROVIDER_FLAVOR_VARIANTS: &[&str] = &[
    "generic_openai",
    "deepseek",
    "openrouter",
    "enable_thinking",
    "thinking_type",
    "minimax",
];
pub const LEGACY_OLLAMA_CHAT_PROVIDER_ID: &str = "ollama-chat";
pub const OLLAMA_CHAT_PROVIDER_REMOVED_ERROR: &str = "`ollama-chat` is no longer supported.\nHow to fix: replace `ollama-chat` with `ollama` in `model_provider`, `oss_provider`, or `--local-provider`.\nMore info: https://github.com/openai/codex/discussions/7782";

/// Wire protocol that the provider speaks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WireApi {
    /// The Responses API exposed by OpenAI at `/v1/responses`.
    Responses,
    /// Anthropic Messages API exposed at `/v1/messages`.
    AnthropicMessages,
    /// OpenAI-compatible Chat Completions API exposed at `/v1/chat/completions`.
    #[default]
    ChatCompletions,
}

impl fmt::Display for WireApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Responses => "responses",
            Self::AnthropicMessages => "anthropic_messages",
            Self::ChatCompletions => "chat_completions",
        };
        f.write_str(value)
    }
}

impl<'de> Deserialize<'de> for WireApi {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "responses" => Ok(Self::Responses),
            "anthropic_messages" => Ok(Self::AnthropicMessages),
            "chat_completions" => Ok(Self::ChatCompletions),
            "chat" => Err(serde::de::Error::custom(CHAT_WIRE_API_REMOVED_ERROR)),
            _ => Err(serde::de::Error::unknown_variant(&value, WIRE_API_VARIANTS)),
        }
    }
}

/// OpenAI-compatible `/v1/chat/completions` provider dialect.
///
/// `wire_api` selects the outer protocol. `ProviderFlavor` selects the small
/// provider-specific request/stream differences inside that protocol, such as
/// reasoning controls and usage accounting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFlavor {
    /// Plain OpenAI-compatible provider. Astral does not send private reasoning fields.
    #[default]
    GenericOpenAi,
    /// DeepSeek-compatible reasoning fields and cache usage.
    DeepSeek,
    /// OpenRouter gateway-specific reasoning object.
    OpenRouter,
    /// Providers such as DashScope/Qwen that use an `enable_thinking` switch.
    EnableThinking,
    /// Providers that use a `thinking.type` switch.
    ThinkingType,
    /// MiniMax-compatible reasoning fields.
    MiniMax,
}

impl fmt::Display for ProviderFlavor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::GenericOpenAi => "generic_openai",
            Self::DeepSeek => "deepseek",
            Self::OpenRouter => "openrouter",
            Self::EnableThinking => "enable_thinking",
            Self::ThinkingType => "thinking_type",
            Self::MiniMax => "minimax",
        };
        f.write_str(value)
    }
}

impl<'de> Deserialize<'de> for ProviderFlavor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "generic_openai" => Ok(Self::GenericOpenAi),
            "deepseek" => Ok(Self::DeepSeek),
            "openrouter" => Ok(Self::OpenRouter),
            "enable_thinking" => Ok(Self::EnableThinking),
            "thinking_type" => Ok(Self::ThinkingType),
            "minimax" => Ok(Self::MiniMax),
            _ => Err(serde::de::Error::unknown_variant(
                &value,
                PROVIDER_FLAVOR_VARIANTS,
            )),
        }
    }
}

/// Serializable representation of a provider definition.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ModelProviderInfo {
    /// Friendly display name.
    #[serde(default)]
    pub name: String,
    /// Base URL for the provider's HTTP API.
    pub base_url: Option<String>,
    /// Environment variable that stores the user's API key for this provider.
    pub env_key: Option<String>,

    /// Optional instructions to help the user get a valid value for the
    /// variable and set it.
    pub env_key_instructions: Option<String>,
    /// Value to use with `Authorization: Bearer <token>` header. Use of this
    /// config is discouraged in favor of `env_key` for security reasons, but
    /// this may be necessary when using this programmatically.
    pub experimental_bearer_token: Option<String>,
    /// Command-backed bearer-token configuration for this provider.
    pub auth: Option<ModelProviderAuthInfo>,
    /// AWS SigV4 auth configuration for this provider.
    pub aws: Option<ModelProviderAwsAuthInfo>,
    /// Which wire protocol this provider expects.
    #[serde(default)]
    pub wire_api: WireApi,
    /// Optional provider dialect within the selected wire protocol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_flavor: Option<ProviderFlavor>,
    /// Optional query parameters to append to the base URL.
    pub query_params: Option<HashMap<String, String>>,
    /// Additional JSON body fields to merge into provider-neutral agent requests.
    ///
    /// This is intentionally provider-scoped so OpenAI-compatible providers
    /// can opt into vendor-specific fields without changing Astral's core IR.
    pub request_body: Option<BTreeMap<String, Value>>,
    /// Request body field names to remove after Astral applies adapter defaults
    /// and `request_body` overrides.
    ///
    /// This lets strict provider-compatible gateways opt out of fields they do
    /// not support, such as `stream_options`, without adding vendor-specific
    /// branches to Astral's provider-neutral adapters.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub request_body_remove: Vec<String>,
    /// Additional HTTP headers to include in requests to this provider where
    /// the (key, value) pairs are the header name and value.
    pub http_headers: Option<HashMap<String, String>>,
    /// Optional HTTP headers to include in requests to this provider where the
    /// (key, value) pairs are the header name and _environment variable_ whose
    /// value should be used. If the environment variable is not set, or the
    /// value is empty, the header will not be included in the request.
    pub env_http_headers: Option<HashMap<String, String>>,
    /// Maximum number of times to retry a failed HTTP request to this provider.
    pub request_max_retries: Option<u64>,
    /// Number of times to retry reconnecting a dropped streaming response before failing.
    pub stream_max_retries: Option<u64>,
    /// Idle timeout (in milliseconds) to wait for activity on a streaming response before treating
    /// the connection as lost.
    pub stream_idle_timeout_ms: Option<u64>,
    /// Maximum time (in milliseconds) to wait for a websocket connection attempt before treating
    /// it as failed.
    pub websocket_connect_timeout_ms: Option<u64>,
    /// Does this provider require Astral-managed credentials? If true, the user
    /// is presented with the login screen on first run, and credentials are
    /// stored in auth.json. If false (which is the default), the login screen is
    /// skipped, and API keys (if needed) come from provider-specific auth such
    /// as "env_key" or "auth".
    #[serde(default)]
    pub requires_astral_auth: bool,
    /// Whether this provider supports the Responses API WebSocket transport.
    #[serde(default)]
    pub supports_websockets: bool,
}

/// AWS SigV4 auth configuration for a model provider.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ModelProviderAwsAuthInfo {
    /// AWS profile name to use. When unset, the AWS SDK default chain decides.
    pub profile: Option<String>,
    /// AWS region to use for provider-specific endpoints.
    pub region: Option<String>,
}

impl ModelProviderInfo {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.aws.is_some() {
            if self.supports_websockets {
                // TODO(celia-oai): Support AWS SigV4 signing for WebSocket
                // upgrade requests before allowing AWS-authenticated providers
                // to enable Responses-over-WebSocket.
                return Err("provider aws cannot be combined with supports_websockets".to_string());
            }

            let mut conflicts = Vec::new();
            if self.env_key.is_some() {
                conflicts.push("env_key");
            }
            if self.experimental_bearer_token.is_some() {
                conflicts.push("experimental_bearer_token");
            }
            if self.auth.is_some() {
                conflicts.push("auth");
            }
            if self.requires_astral_auth {
                conflicts.push("requires_astral_auth");
            }

            if !conflicts.is_empty() {
                return Err(format!(
                    "provider aws cannot be combined with {}",
                    conflicts.join(", ")
                ));
            }
        }

        let Some(auth) = self.auth.as_ref() else {
            return Ok(());
        };

        if auth.command.trim().is_empty() {
            return Err("provider auth.command must not be empty".to_string());
        }

        let mut conflicts = Vec::new();
        if self.env_key.is_some() {
            conflicts.push("env_key");
        }
        if self.experimental_bearer_token.is_some() {
            conflicts.push("experimental_bearer_token");
        }
        if self.requires_astral_auth {
            conflicts.push("requires_astral_auth");
        }

        if conflicts.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "provider auth cannot be combined with {}",
                conflicts.join(", ")
            ))
        }
    }

    fn build_header_map(&self) -> CodexResult<HeaderMap> {
        let capacity = self.http_headers.as_ref().map_or(0, HashMap::len)
            + self.env_http_headers.as_ref().map_or(0, HashMap::len);
        let mut headers = HeaderMap::with_capacity(capacity);
        if let Some(extra) = &self.http_headers {
            for (k, v) in extra {
                if let (Ok(name), Ok(value)) = (HeaderName::try_from(k), HeaderValue::try_from(v)) {
                    headers.insert(name, value);
                }
            }
        }

        if let Some(env_headers) = &self.env_http_headers {
            for (header, env_var) in env_headers {
                if let Ok(val) = std::env::var(env_var)
                    && !val.trim().is_empty()
                    && let (Ok(name), Ok(value)) =
                        (HeaderName::try_from(header), HeaderValue::try_from(val))
                {
                    headers.insert(name, value);
                }
            }
        }

        Ok(headers)
    }

    pub fn to_api_provider(
        &self,
        _auth_mode: Option<codex_app_server_protocol::AuthMode>,
    ) -> CodexResult<ApiProvider> {
        let base_url = self.base_url.clone().ok_or_else(|| {
            CodexErr::InvalidRequest(format!(
                "model provider `{}` has no base_url; set `{ASTRAL_BASE_URL_ENV_VAR}` or configure `model_providers.<id>.base_url`",
                self.name
            ))
        })?;

        let headers = self.build_header_map()?;
        let retry = ApiRetryConfig {
            max_attempts: self.request_max_retries(),
            base_delay: Duration::from_millis(200),
            retry_429: false,
            retry_5xx: true,
            retry_transport: true,
        };

        Ok(ApiProvider {
            name: self.name.clone(),
            base_url,
            query_params: self.query_params.clone(),
            headers,
            retry,
            stream_idle_timeout: self.stream_idle_timeout(),
        })
    }

    /// If `env_key` is Some, returns the API key for this provider if present
    /// (and non-empty) in the environment. If `env_key` is required but
    /// cannot be found, returns an error.
    pub fn api_key(&self) -> CodexResult<Option<String>> {
        match &self.env_key {
            Some(env_key) => {
                let api_key = std::env::var(env_key)
                    .ok()
                    .filter(|v| !v.trim().is_empty())
                    .ok_or_else(|| {
                        CodexErr::EnvVar(EnvVarError {
                            var: env_key.clone(),
                            instructions: self.env_key_instructions.clone(),
                        })
                    })?;
                Ok(Some(api_key))
            }
            None => Ok(None),
        }
    }

    /// Effective maximum number of request retries for this provider.
    pub fn request_max_retries(&self) -> u64 {
        self.request_max_retries
            .unwrap_or(DEFAULT_REQUEST_MAX_RETRIES)
            .min(MAX_REQUEST_MAX_RETRIES)
    }

    /// Effective maximum number of stream reconnection attempts for this provider.
    pub fn stream_max_retries(&self) -> u64 {
        self.stream_max_retries
            .unwrap_or(DEFAULT_STREAM_MAX_RETRIES)
            .min(MAX_STREAM_MAX_RETRIES)
    }

    /// Effective idle timeout for streaming responses.
    pub fn stream_idle_timeout(&self) -> Duration {
        self.stream_idle_timeout_ms
            .map(Duration::from_millis)
            .unwrap_or(Duration::from_millis(DEFAULT_STREAM_IDLE_TIMEOUT_MS))
    }

    /// Effective timeout for websocket connect attempts.
    pub fn websocket_connect_timeout(&self) -> Duration {
        self.websocket_connect_timeout_ms
            .map(Duration::from_millis)
            .unwrap_or(Duration::from_millis(DEFAULT_WEBSOCKET_CONNECT_TIMEOUT_MS))
    }

    /// Effective provider dialect for OpenAI-compatible chat completions.
    pub fn effective_provider_flavor(&self) -> ProviderFlavor {
        self.provider_flavor
            .unwrap_or_else(|| self.infer_provider_flavor())
    }

    fn infer_provider_flavor(&self) -> ProviderFlavor {
        let haystack = format!(
            "{} {}",
            self.name.to_ascii_lowercase(),
            self.base_url
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
        );

        if haystack.contains("deepseek") {
            ProviderFlavor::DeepSeek
        } else if haystack.contains("openrouter") {
            ProviderFlavor::OpenRouter
        } else if haystack.contains("dashscope")
            || haystack.contains("qwen")
            || haystack.contains("aliyuncs.com")
        {
            ProviderFlavor::EnableThinking
        } else if haystack.contains("minimax") {
            ProviderFlavor::MiniMax
        } else if haystack.contains("bigmodel")
            || haystack.contains("z.ai")
            || haystack.contains("glm")
            || haystack.contains("kimi")
            || haystack.contains("moonshot")
            || haystack.contains("mimo")
        {
            ProviderFlavor::ThinkingType
        } else {
            ProviderFlavor::GenericOpenAi
        }
    }

    pub fn create_astral_provider() -> ModelProviderInfo {
        let base_url = std::env::var(ASTRAL_BASE_URL_ENV_VAR)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        ModelProviderInfo {
            name: ASTRAL_PROVIDER_NAME.into(),
            base_url,
            env_key: Some(ASTRAL_API_KEY_ENV_VAR.to_string()),
            env_key_instructions: Some(format!(
                "Set {ASTRAL_API_KEY_ENV_VAR} for the active Astral model provider."
            )),
            experimental_bearer_token: None,
            auth: None,
            aws: None,
            wire_api: WireApi::ChatCompletions,
            provider_flavor: None,
            query_params: None,
            request_body: None,
            request_body_remove: Vec::new(),
            http_headers: Some(
                [("version".to_string(), env!("CARGO_PKG_VERSION").to_string())]
                    .into_iter()
                    .collect(),
            ),
            env_http_headers: None,
            // Use global defaults for retry/timeout unless overridden in config.toml.
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_astral_auth: false,
            supports_websockets: false,
        }
    }

    pub fn create_openai_provider(base_url: Option<String>) -> ModelProviderInfo {
        ModelProviderInfo {
            name: OPENAI_PROVIDER_NAME.into(),
            base_url,
            env_key: None,
            env_key_instructions: None,
            experimental_bearer_token: None,
            auth: None,
            aws: None,
            wire_api: WireApi::Responses,
            provider_flavor: None,
            query_params: None,
            request_body: None,
            request_body_remove: Vec::new(),
            http_headers: Some(
                [("version".to_string(), env!("CARGO_PKG_VERSION").to_string())]
                    .into_iter()
                    .collect(),
            ),
            env_http_headers: None,
            // Use global defaults for retry/timeout unless overridden in config.toml.
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_astral_auth: false,
            supports_websockets: false,
        }
    }

    pub fn create_anthropic_provider() -> ModelProviderInfo {
        ModelProviderInfo {
            name: ANTHROPIC_PROVIDER_NAME.into(),
            base_url: Some(ANTHROPIC_DEFAULT_BASE_URL.into()),
            env_key: None,
            env_key_instructions: Some(format!(
                "Set {ANTHROPIC_API_KEY_ENV_VAR} for the Anthropic model provider."
            )),
            experimental_bearer_token: None,
            auth: None,
            aws: None,
            wire_api: WireApi::AnthropicMessages,
            provider_flavor: None,
            query_params: None,
            request_body: None,
            request_body_remove: Vec::new(),
            http_headers: Some(
                [("version".to_string(), env!("CARGO_PKG_VERSION").to_string())]
                    .into_iter()
                    .collect(),
            ),
            env_http_headers: Some(HashMap::from([(
                "x-api-key".to_string(),
                ANTHROPIC_API_KEY_ENV_VAR.to_string(),
            )])),
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_astral_auth: false,
            supports_websockets: false,
        }
    }

    pub fn create_amazon_bedrock_provider(
        aws: Option<ModelProviderAwsAuthInfo>,
    ) -> ModelProviderInfo {
        ModelProviderInfo {
            name: AMAZON_BEDROCK_PROVIDER_NAME.into(),
            base_url: Some(AMAZON_BEDROCK_DEFAULT_BASE_URL.into()),
            env_key: None,
            env_key_instructions: None,
            experimental_bearer_token: None,
            auth: None,
            aws: Some(aws.unwrap_or(ModelProviderAwsAuthInfo {
                profile: None,
                region: None,
            })),
            wire_api: WireApi::Responses,
            provider_flavor: None,
            query_params: None,
            request_body: None,
            request_body_remove: Vec::new(),
            http_headers: Some(HashMap::from([(
                AMAZON_BEDROCK_MANTLE_CLIENT_AGENT_HEADER.to_string(),
                AMAZON_BEDROCK_MANTLE_CLIENT_AGENT_VALUE.to_string(),
            )])),
            env_http_headers: None,
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_astral_auth: false,
            supports_websockets: false,
        }
    }

    pub fn is_openai(&self) -> bool {
        self.name == OPENAI_PROVIDER_NAME
    }

    pub fn is_astral(&self) -> bool {
        self.name == ASTRAL_PROVIDER_NAME
    }

    pub fn is_anthropic(&self) -> bool {
        self.name == ANTHROPIC_PROVIDER_NAME
    }

    pub fn is_amazon_bedrock(&self) -> bool {
        self.name == AMAZON_BEDROCK_PROVIDER_NAME
    }

    pub fn has_command_auth(&self) -> bool {
        self.auth.is_some()
    }
}

pub const DEFAULT_LMSTUDIO_PORT: u16 = 1234;
pub const DEFAULT_OLLAMA_PORT: u16 = 11434;

pub const LMSTUDIO_OSS_PROVIDER_ID: &str = "lmstudio";
pub const OLLAMA_OSS_PROVIDER_ID: &str = "ollama";

/// Built-in default provider list.
pub fn built_in_model_providers() -> HashMap<String, ModelProviderInfo> {
    use ModelProviderInfo as P;
    let astral_provider = P::create_astral_provider();
    let anthropic_provider = P::create_anthropic_provider();
    let amazon_bedrock_provider = P::create_amazon_bedrock_provider(/*aws*/ None);

    // Keep the bundled catalog small: Astral's generic provider, Anthropic,
    // Bedrock, and local OSS providers. Users are encouraged
    // to add their own entries under `model_providers` in config.toml.
    [
        (ASTRAL_PROVIDER_ID, astral_provider),
        (ANTHROPIC_PROVIDER_ID, anthropic_provider),
        (AMAZON_BEDROCK_PROVIDER_ID, amazon_bedrock_provider),
        (
            OLLAMA_OSS_PROVIDER_ID,
            create_oss_provider(DEFAULT_OLLAMA_PORT, WireApi::ChatCompletions),
        ),
        (
            LMSTUDIO_OSS_PROVIDER_ID,
            create_oss_provider(DEFAULT_LMSTUDIO_PORT, WireApi::ChatCompletions),
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Merge configured providers into the bootstrap provider catalog.
///
/// Configured providers extend or replace the bootstrap set. Astral does not
/// treat compiled-in provider entries as authoritative presets; users can
/// define their own provider shapes under the same ids in config.toml.
pub fn merge_configured_model_providers(
    mut model_providers: HashMap<String, ModelProviderInfo>,
    configured_model_providers: HashMap<String, ModelProviderInfo>,
) -> Result<HashMap<String, ModelProviderInfo>, String> {
    for (key, provider) in configured_model_providers {
        model_providers.insert(key, provider);
    }

    Ok(model_providers)
}

pub fn create_oss_provider(default_provider_port: u16, wire_api: WireApi) -> ModelProviderInfo {
    // These ASTRAL_OSS_ environment variables are experimental: we may
    // switch to reading values from config.toml instead.
    let default_astral_oss_base_url = format!(
        "http://localhost:{astral_oss_port}/v1",
        astral_oss_port = std::env::var(ASTRAL_OSS_PORT_ENV_VAR)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(default_provider_port)
    );

    let astral_oss_base_url = std::env::var(ASTRAL_OSS_BASE_URL_ENV_VAR)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(default_astral_oss_base_url);
    create_oss_provider_with_base_url(&astral_oss_base_url, wire_api)
}

pub fn create_oss_provider_with_base_url(base_url: &str, wire_api: WireApi) -> ModelProviderInfo {
    ModelProviderInfo {
        name: "gpt-oss".into(),
        base_url: Some(base_url.into()),
        env_key: None,
        env_key_instructions: None,
        experimental_bearer_token: None,
        auth: None,
        aws: None,
        wire_api,
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
    }
}

#[cfg(test)]
#[path = "model_provider_info_tests.rs"]
mod tests;
