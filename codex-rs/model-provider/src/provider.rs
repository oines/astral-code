use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use codex_api::Provider;
use codex_api::SharedAuthProvider;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_login::UnauthorizedRecovery;
use codex_model_provider_info::ManagedAuthKind;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use codex_models_manager::manager::OpenAiModelsManager;
use codex_models_manager::manager::SharedModelsManager;
use codex_models_manager::manager::StaticModelsManager;
use codex_protocol::account::ProviderAccount;
use codex_protocol::openai_models::ModelsResponse;

use crate::amazon_bedrock::AmazonBedrockModelProvider;
use crate::auth::auth_manager_for_provider;
use crate::auth::provider_info_for_request;
use crate::auth::resolve_provider_auth;
use crate::models_endpoint::OpenAiModelsEndpoint;

/// Optional provider-backed features that Codex may expose at runtime.
///
/// These capabilities are a provider-owned upper bound. Callers can disable
/// more functionality through normal config, but should not expose a feature
/// that the active provider marks unsupported here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderCapabilities {
    pub namespace_tools: bool,
    pub image_generation: bool,
    pub web_search: bool,
}

impl Default for ProviderCapabilities {
    fn default() -> Self {
        Self {
            namespace_tools: true,
            image_generation: false,
            web_search: false,
        }
    }
}

/// Current app-visible account state for a model provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAccountState {
    pub account: Option<ProviderAccount>,
    pub requires_astral_auth: bool,
    pub requires_openai_auth: bool,
}

/// Runtime provider abstraction used by model execution.
///
/// Implementations own provider-specific behavior for a model backend. The
/// `ModelProviderInfo` returned by `info` is the serialized/configured provider
/// metadata used by the default OpenAI-compatible implementation.
#[async_trait::async_trait]
pub trait ModelProvider: fmt::Debug + Send + Sync {
    /// Returns the configured provider metadata.
    fn info(&self) -> &ModelProviderInfo;

    /// Returns the provider-owned capability upper bounds.
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::default()
    }

    /// Returns whether requests made through this provider should include attestation.
    fn supports_attestation(&self) -> bool {
        false
    }

    /// Returns the provider-scoped auth manager, when this provider uses one.
    ///
    /// TODO(celia-oai): Make auth manager access internal to this crate so callers
    /// resolve provider-specific auth only through `ModelProvider`. We first need
    /// to think through whether Codex should have a unified provider-specific auth
    /// manager throughout the codebase; that is a larger refactor than this change.
    fn auth_manager(&self) -> Option<Arc<AuthManager>>;

    /// Returns bounded 401 recovery for this provider's credential authority.
    fn unauthorized_recovery(&self) -> Option<UnauthorizedRecovery> {
        self.auth_manager()
            .map(|manager| manager.unauthorized_recovery())
    }

    /// Returns the current provider-scoped auth value, if one is configured.
    async fn auth(&self) -> Option<CodexAuth>;

    /// Returns the current app-visible account state for this provider.
    fn account_state(&self) -> ProviderAccountState;

    /// Returns provider configuration adapted for the API client.
    async fn api_provider(&self) -> codex_protocol::error::Result<Provider> {
        let auth = self.auth().await;
        provider_info_for_request(self.info())
            .to_api_provider(auth.as_ref().map(CodexAuth::auth_mode))
    }

    /// Returns the provider base URL that will be used at request time.
    async fn runtime_base_url(&self) -> codex_protocol::error::Result<Option<String>> {
        Ok(self.info().base_url.clone())
    }

    /// Returns the auth provider used to attach request credentials.
    async fn api_auth(&self) -> codex_protocol::error::Result<SharedAuthProvider> {
        let auth = self.auth().await;
        resolve_provider_auth(auth.as_ref(), self.info())
    }

    /// Creates the model manager implementation appropriate for this provider.
    fn models_manager(
        &self,
        codex_home: PathBuf,
        config_model_catalog: Option<ModelsResponse>,
    ) -> SharedModelsManager;
}

/// Shared runtime model provider handle.
pub type SharedModelProvider = Arc<dyn ModelProvider>;

/// Creates the default runtime model provider for configured provider metadata.
pub fn create_model_provider(
    provider_info: ModelProviderInfo,
    auth_manager: Option<Arc<AuthManager>>,
) -> SharedModelProvider {
    if provider_info.is_amazon_bedrock() {
        Arc::new(AmazonBedrockModelProvider::new(provider_info))
    } else {
        Arc::new(ConfiguredModelProvider::new(provider_info, auth_manager))
    }
}

/// Runtime model provider backed by configured `ModelProviderInfo`.
#[derive(Clone, Debug)]
struct ConfiguredModelProvider {
    info: ModelProviderInfo,
    auth_manager: Option<Arc<AuthManager>>,
}

impl ConfiguredModelProvider {
    fn new(provider_info: ModelProviderInfo, auth_manager: Option<Arc<AuthManager>>) -> Self {
        let auth_manager = auth_manager_for_provider(auth_manager, &provider_info);
        Self {
            info: provider_info,
            auth_manager,
        }
    }
}

#[async_trait::async_trait]
impl ModelProvider for ConfiguredModelProvider {
    fn info(&self) -> &ModelProviderInfo {
        &self.info
    }

    fn capabilities(&self) -> ProviderCapabilities {
        let responses = self.info.wire_api == WireApi::Responses;
        ProviderCapabilities {
            namespace_tools: true,
            image_generation: responses
                && self.info.responses_builtin_tools.allows("image_generation"),
            web_search: responses && self.info.responses_builtin_tools.allows("web_search"),
        }
    }

    fn auth_manager(&self) -> Option<Arc<AuthManager>> {
        self.auth_manager.clone()
    }

    fn supports_attestation(&self) -> bool {
        false
    }

    async fn auth(&self) -> Option<CodexAuth> {
        match self.auth_manager.as_ref() {
            Some(auth_manager) if self.info.managed_auth == Some(ManagedAuthKind::CodexOAuth) => {
                auth_manager.codex_oauth_auth().await
            }
            Some(auth_manager) => auth_manager.auth().await,
            None => None,
        }
    }

    fn unauthorized_recovery(&self) -> Option<UnauthorizedRecovery> {
        self.auth_manager.as_ref().map(|manager| {
            if self.info.managed_auth == Some(ManagedAuthKind::CodexOAuth) {
                manager.codex_oauth_unauthorized_recovery()
            } else {
                manager.unauthorized_recovery()
            }
        })
    }

    fn account_state(&self) -> ProviderAccountState {
        let account = if self.info.api_key().ok().flatten().is_some() {
            Some(ProviderAccount::ApiKey)
        } else {
            let provider_auth = self.auth_manager.as_ref().and_then(|auth_manager| {
                let auth = if self.info.managed_auth == Some(ManagedAuthKind::CodexOAuth) {
                    auth_manager.codex_oauth_auth_cached()?
                } else {
                    auth_manager.auth_cached()?
                };
                if auth_manager.refresh_failure_for_auth(&auth).is_some() {
                    return None;
                }
                Some(auth)
            });
            provider_auth.map(|auth| match auth {
                CodexAuth::ApiKey(_) => ProviderAccount::ApiKey,
                CodexAuth::Chatgpt(_) => ProviderAccount::Chatgpt {
                    email: auth.account_email(),
                    plan_type: auth.account_plan_type().unwrap_or_default(),
                },
            })
        };

        ProviderAccountState {
            account,
            requires_astral_auth: self.info.requires_astral_auth,
            requires_openai_auth: self.info.managed_auth == Some(ManagedAuthKind::CodexOAuth),
        }
    }

    fn models_manager(
        &self,
        codex_home: PathBuf,
        config_model_catalog: Option<ModelsResponse>,
    ) -> SharedModelsManager {
        if let Some(model_catalog) = config_model_catalog {
            return Arc::new(StaticModelsManager::new(
                self.auth_manager.clone(),
                model_catalog,
            ));
        }

        match self.info.wire_api {
            // Anthropic-compatible providers commonly expose Messages without a
            // model catalog. Do not infer discovery support from the wire format;
            // callers can opt in with `model_catalog_json` above.
            WireApi::AnthropicMessages => Arc::new(StaticModelsManager::new(
                self.auth_manager.clone(),
                ModelsResponse { models: Vec::new() },
            )),
            WireApi::Responses | WireApi::ChatCompletions => {
                let endpoint = Arc::new(OpenAiModelsEndpoint::new(
                    self.info.clone(),
                    self.auth_manager.clone(),
                ));
                Arc::new(OpenAiModelsManager::new(
                    codex_home,
                    endpoint,
                    self.auth_manager.clone(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use codex_login::AuthCredentialsStoreMode;
    use codex_login::AuthDotJson;
    use codex_login::TokenData;
    use codex_login::save_codex_oauth_auth;
    use codex_login::token_data::parse_chatgpt_jwt_claims;
    use codex_model_provider_info::ModelProviderAwsAuthInfo;
    use codex_models_manager::manager::RefreshStrategy;
    use codex_protocol::config_types::ModelProviderAuthInfo;
    use codex_protocol::openai_models::ModelInfo;
    use codex_protocol::openai_models::ModelsResponse;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::header_regex;
    use wiremock::matchers::method;
    use wiremock::matchers::path;

    use super::*;

    fn provider_info_with_command_auth() -> ModelProviderInfo {
        ModelProviderInfo {
            auth: Some(ModelProviderAuthInfo {
                command: "print-token".to_string(),
                args: Vec::new(),
                timeout_ms: NonZeroU64::new(5_000).expect("timeout should be non-zero"),
                refresh_interval_ms: 300_000,
                cwd: std::env::current_dir()
                    .expect("current dir should be available")
                    .try_into()
                    .expect("current dir should be absolute"),
            }),
            requires_astral_auth: false,
            ..ModelProviderInfo::create_openai_provider(/*base_url*/ None)
        }
    }

    fn test_codex_home() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("codex-model-provider-test-{}", std::process::id()))
    }

    fn provider_for(base_url: String) -> ModelProviderInfo {
        ModelProviderInfo {
            name: "mock".into(),
            base_url: Some(base_url),
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
            request_max_retries: Some(0),
            stream_max_retries: Some(0),
            stream_idle_timeout_ms: Some(5_000),
            websocket_connect_timeout_ms: None,
            requires_astral_auth: false,
            supports_websockets: false,
        }
    }

    fn remote_model(slug: &str) -> ModelInfo {
        serde_json::from_value(json!({
            "slug": slug,
            "display_name": slug,
            "description": null,
            "default_reasoning_level": "medium",
            "supported_reasoning_levels": [],
            "shell_type": "shell_command",
            "visibility": "list",
            "supported_in_api": true,
            "priority": 0,
            "upgrade": null,
            "base_instructions": "base instructions",
            "supports_reasoning_summaries": false,
            "support_verbosity": false,
            "default_verbosity": null,
            "apply_patch_tool_type": null,
            "truncation_policy": {"mode": "bytes", "limit": 10_000},
            "supports_parallel_tool_calls": false,
            "supports_image_detail_original": false,
            "context_window": 272_000,
            "max_context_window": 272_000,
            "experimental_supported_tools": [],
        }))
        .expect("valid model")
    }

    #[test]
    fn configured_provider_uses_default_capabilities() {
        let provider = create_model_provider(
            ModelProviderInfo::create_openai_provider(/*base_url*/ None),
            /*auth_manager*/ None,
        );

        assert_eq!(provider.capabilities(), ProviderCapabilities::default());
    }

    #[test]
    fn non_openai_provider_disables_hosted_provider_capabilities() {
        let provider = create_model_provider(
            ModelProviderInfo::create_astral_provider(),
            /*auth_manager*/ None,
        );

        assert_eq!(
            provider.capabilities(),
            ProviderCapabilities {
                namespace_tools: true,
                image_generation: false,
                web_search: false,
            }
        );
    }

    #[tokio::test]
    async fn configured_provider_runtime_base_url_uses_configured_base_url() {
        let provider = create_model_provider(
            provider_for("https://example.test/v1".to_string()),
            /*auth_manager*/ None,
        );

        assert_eq!(
            provider
                .runtime_base_url()
                .await
                .expect("runtime base URL should resolve"),
            Some("https://example.test/v1".to_string())
        );
    }

    #[test]
    fn create_model_provider_builds_command_auth_manager_without_base_manager() {
        let provider = create_model_provider(
            provider_info_with_command_auth(),
            /*auth_manager*/ None,
        );

        let auth_manager = provider
            .auth_manager()
            .expect("command auth provider should have an auth manager");

        assert!(auth_manager.has_external_auth());
    }

    #[test]
    fn create_model_provider_does_not_use_openai_auth_manager_for_amazon_bedrock_provider() {
        let provider = create_model_provider(
            ModelProviderInfo::create_amazon_bedrock_provider(Some(ModelProviderAwsAuthInfo {
                profile: Some("codex-bedrock".to_string()),
                region: None,
            })),
            Some(AuthManager::from_auth_for_testing(CodexAuth::from_api_key(
                "openai-api-key",
            ))),
        );

        assert!(provider.auth_manager().is_none());
    }

    #[test]
    fn create_model_provider_does_not_use_openai_auth_manager_for_astral_provider() {
        let provider = create_model_provider(
            ModelProviderInfo::create_astral_provider(),
            Some(AuthManager::from_auth_for_testing(CodexAuth::from_api_key(
                "openai-api-key",
            ))),
        );

        assert!(provider.auth_manager().is_none());
    }

    #[test]
    fn legacy_responses_provider_returns_no_account_state() {
        let provider = create_model_provider(
            ModelProviderInfo::create_openai_provider(/*base_url*/ None),
            /*auth_manager*/ None,
        );

        assert_eq!(
            provider.account_state(),
            ProviderAccountState {
                account: None,
                requires_astral_auth: false,
                requires_openai_auth: false,
            }
        );
    }

    #[test]
    fn legacy_responses_provider_does_not_use_astral_auth_manager() {
        let provider = create_model_provider(
            ModelProviderInfo::create_openai_provider(/*base_url*/ None),
            Some(AuthManager::from_auth_for_testing(CodexAuth::from_api_key(
                "openai-api-key",
            ))),
        );

        assert_eq!(
            provider.account_state(),
            ProviderAccountState {
                account: None,
                requires_astral_auth: false,
                requires_openai_auth: false,
            }
        );
    }

    #[test]
    fn legacy_responses_provider_ignores_chatgpt_account_state() {
        let provider = create_model_provider(
            ModelProviderInfo::create_openai_provider(/*base_url*/ None),
            Some(AuthManager::from_auth_for_testing(
                CodexAuth::create_dummy_api_key_auth_for_testing(),
            )),
        );

        assert_eq!(
            provider.account_state(),
            ProviderAccountState {
                account: None,
                requires_astral_auth: false,
                requires_openai_auth: false,
            }
        );
    }

    #[test]
    fn custom_non_openai_provider_returns_no_account_state() {
        let provider = create_model_provider(
            ModelProviderInfo {
                name: "Custom".to_string(),
                base_url: Some("http://localhost:1234/v1".to_string()),
                wire_api: WireApi::ChatCompletions,
                requires_astral_auth: false,
                ..Default::default()
            },
            /*auth_manager*/ None,
        );

        assert_eq!(
            provider.account_state(),
            ProviderAccountState {
                account: None,
                requires_astral_auth: false,
                requires_openai_auth: false,
            }
        );
    }

    #[test]
    fn amazon_bedrock_provider_returns_bedrock_account_state() {
        let provider = create_model_provider(
            ModelProviderInfo::create_amazon_bedrock_provider(/*aws*/ None),
            /*auth_manager*/ None,
        );

        assert_eq!(
            provider.account_state(),
            ProviderAccountState {
                account: Some(ProviderAccount::AmazonBedrock),
                requires_astral_auth: false,
                requires_openai_auth: false,
            }
        );
    }

    #[tokio::test]
    async fn amazon_bedrock_provider_creates_static_models_manager() {
        let provider = create_model_provider(
            ModelProviderInfo::create_amazon_bedrock_provider(/*aws*/ None),
            /*auth_manager*/ None,
        );
        let manager =
            provider.models_manager(test_codex_home(), /*config_model_catalog*/ None);

        let catalog = manager.raw_model_catalog(RefreshStrategy::Online).await;
        let model_ids = catalog
            .models
            .iter()
            .map(|model| model.slug.as_str())
            .collect::<Vec<_>>();

        assert_eq!(model_ids, vec!["openai.gpt-5.5", "openai.gpt-5.4"]);

        let default_model = manager
            .list_models(RefreshStrategy::Online)
            .await
            .into_iter()
            .find(|preset| preset.is_default)
            .expect("Bedrock catalog should have a default model");

        assert_eq!(default_model.model, "openai.gpt-5.5");
    }

    #[tokio::test]
    async fn configured_bedrock_catalog_only_allows_default_service_tier() {
        let configured_model = codex_models_manager::bundled_models_response()
            .expect("bundled models should parse")
            .models
            .into_iter()
            .find(|model| model.slug == "gpt-5.4")
            .expect("bundled models should include GPT-5.4");
        assert!(!configured_model.additional_speed_tiers.is_empty());
        assert!(!configured_model.service_tiers.is_empty());

        let provider = create_model_provider(
            ModelProviderInfo::create_amazon_bedrock_provider(/*aws*/ None),
            /*auth_manager*/ None,
        );
        let manager = provider.models_manager(
            test_codex_home(),
            Some(ModelsResponse {
                models: vec![configured_model],
            }),
        );

        let catalog = manager.raw_model_catalog(RefreshStrategy::Online).await;

        assert_eq!(catalog.models.len(), 1);
        assert_eq!(catalog.models[0].slug, "gpt-5.4");
        assert_eq!(
            catalog.models[0].additional_speed_tiers,
            Vec::<String>::new()
        );
        assert_eq!(catalog.models[0].service_tiers, Vec::new());
        assert_eq!(catalog.models[0].default_service_tier, None);
    }

    #[tokio::test]
    async fn configured_provider_models_manager_uses_provider_bearer_token() {
        let server = MockServer::start().await;
        let remote_models = vec![remote_model("provider-model")];

        Mock::given(method("GET"))
            .and(path("/models"))
            .and(header_regex("Authorization", "Bearer provider-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_json(ModelsResponse {
                        models: remote_models.clone(),
                    }),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut provider_info = provider_for(server.uri());
        provider_info.experimental_bearer_token = Some("provider-token".to_string());
        let provider = create_model_provider(
            provider_info,
            Some(AuthManager::from_auth_for_testing(
                CodexAuth::create_dummy_api_key_auth_for_testing(),
            )),
        );

        let manager =
            provider.models_manager(test_codex_home(), /*config_model_catalog*/ None);
        let catalog = manager.raw_model_catalog(RefreshStrategy::Online).await;

        assert!(
            catalog
                .models
                .iter()
                .any(|model| model.slug == "provider-model")
        );
    }

    #[tokio::test]
    async fn codex_models_request_has_chatgpt_headers() -> Result<(), Box<dyn std::error::Error>> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .and(header_regex("Authorization", "Bearer codex-access"))
            .and(header_regex("ChatGPT-Account-ID", "workspace-123"))
            .and(header_regex("originator", "codex_cli_rs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ModelsResponse {
                models: vec![remote_model("codex-model")],
            }))
            .expect(1)
            .mount(&server)
            .await;

        let home = test_codex_home().join("codex-oauth");
        std::fs::create_dir_all(&home)?;
        let id_token = concat!(
            "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.",
            "eyJlbWFpbCI6InVzZXJAZXhhbXBsZS5jb20iLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgi",
            "OnsiY2hhdGdwdF9hY2NvdW50X2lkIjoid29ya3NwYWNlLTEyMyIsImNoYXRncHRfcGxhbl90eXBl",
            "IjoicGx1cyJ9fQ.sig"
        );
        save_codex_oauth_auth(
            &home,
            &AuthDotJson {
                auth_mode: Some("chatgpt".to_string()),
                api_key: None,
                tokens: Some(TokenData {
                    id_token: parse_chatgpt_jwt_claims(id_token)?,
                    access_token: "codex-access".to_string(),
                    refresh_token: "codex-refresh".to_string(),
                    account_id: Some("workspace-123".to_string()),
                }),
                last_refresh: None,
            },
            AuthCredentialsStoreMode::File,
        )?;
        let auth_manager = Arc::new(
            AuthManager::new(
                home.clone(),
                /*enable_astral_api_key_env*/ false,
                AuthCredentialsStoreMode::File,
            )
            .await,
        );
        let mut provider_info = ModelProviderInfo::create_codex_provider();
        provider_info.base_url = Some(server.uri());
        let provider = create_model_provider(provider_info, Some(auth_manager));
        let manager = provider.models_manager(home, /*config_model_catalog*/ None);

        let models = manager.list_models(RefreshStrategy::Online).await;

        assert!(models.iter().any(|model| model.model == "codex-model"));
        Ok(())
    }

    #[tokio::test]
    async fn anthropic_provider_models_manager_skips_remote_model_discovery() {
        let server = MockServer::start().await;
        let mut provider_info = provider_for(server.uri());
        provider_info.wire_api = WireApi::AnthropicMessages;
        provider_info.experimental_bearer_token = Some("provider-token".to_string());
        let provider = create_model_provider(provider_info, /*auth_manager*/ None);

        let manager =
            provider.models_manager(test_codex_home(), /*config_model_catalog*/ None);
        let catalog = manager.raw_model_catalog(RefreshStrategy::Online).await;
        let configured_model = manager
            .get_default_model(
                &Some("deepseek-v4-pro".to_string()),
                RefreshStrategy::Online,
            )
            .await;
        let requests = server.received_requests().await.unwrap_or_default();

        assert_eq!(catalog, ModelsResponse { models: Vec::new() });
        assert_eq!(configured_model, "deepseek-v4-pro");
        assert!(
            requests.is_empty(),
            "Anthropic providers must not request a remote model catalog"
        );
    }
}
