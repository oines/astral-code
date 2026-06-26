use super::*;
use crate::ModelsManagerConfig;
use crate::capabilities::ModelCapabilitiesCache;
use crate::capabilities::ModelCapability;
use chrono::Utc;
use codex_app_server_protocol::AuthMode;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_login::ExternalAuth;
use codex_login::ExternalAuthRefreshContext;
use codex_login::ExternalAuthTokens;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ModelsResponse;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use tempfile::tempdir;

#[path = "model_info_overrides_tests.rs"]
mod model_info_overrides_tests;

fn remote_model(slug: &str, display: &str, priority: i32) -> ModelInfo {
    remote_model_with_visibility(slug, display, priority, "list")
}

fn remote_model_with_visibility(
    slug: &str,
    display: &str,
    priority: i32,
    visibility: &str,
) -> ModelInfo {
    serde_json::from_value(json!({
            "slug": slug,
            "display_name": display,
            "description": format!("{display} desc"),
            "default_reasoning_level": "medium",
            "supported_reasoning_levels": [{"effort": "low", "description": "low"}, {"effort": "medium", "description": "medium"}],
            "shell_type": "shell_command",
            "visibility": visibility,
            "minimal_client_version": [0, 1, 0],
            "supported_in_api": true,
            "priority": priority,
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

fn assert_models_contain(actual: &[ModelInfo], expected: &[ModelInfo]) {
    for model in expected {
        assert!(
            actual.iter().any(|candidate| candidate.slug == model.slug),
            "expected model {} in cached list",
            model.slug
        );
    }
}

#[derive(Debug)]
struct TestModelsEndpoint {
    cache_key: String,
    has_command_auth: bool,
    has_provider_auth: bool,
    responses: Mutex<VecDeque<RemoteModelCatalog>>,
    fetch_count: AtomicUsize,
}

impl TestModelsEndpoint {
    fn new(responses: Vec<Vec<ModelInfo>>) -> Arc<Self> {
        Self::with_catalogs(responses.into_iter().map(catalog_response).collect())
    }

    fn with_catalogs(responses: Vec<RemoteModelCatalog>) -> Arc<Self> {
        Self::with_cache_key("test-provider", responses)
    }

    fn with_cache_key(cache_key: &str, responses: Vec<RemoteModelCatalog>) -> Arc<Self> {
        Arc::new(Self {
            cache_key: cache_key.to_string(),
            has_command_auth: false,
            has_provider_auth: true,
            responses: Mutex::new(responses.into()),
            fetch_count: AtomicUsize::new(0),
        })
    }

    fn without_refresh(responses: Vec<Vec<ModelInfo>>) -> Arc<Self> {
        Arc::new(Self {
            cache_key: "test-provider".to_string(),
            has_command_auth: false,
            has_provider_auth: false,
            responses: Mutex::new(responses.into_iter().map(catalog_response).collect()),
            fetch_count: AtomicUsize::new(0),
        })
    }

    fn with_provider_auth(responses: Vec<Vec<ModelInfo>>) -> Arc<Self> {
        Arc::new(Self {
            cache_key: "test-provider".to_string(),
            has_command_auth: false,
            has_provider_auth: true,
            responses: Mutex::new(responses.into_iter().map(catalog_response).collect()),
            fetch_count: AtomicUsize::new(0),
        })
    }

    fn fetch_count(&self) -> usize {
        self.fetch_count.load(Ordering::SeqCst)
    }
}

fn catalog_response(models: Vec<ModelInfo>) -> RemoteModelCatalog {
    RemoteModelCatalog::Catalog { models, etag: None }
}

#[derive(Debug)]
struct TestExternalApiKeyAuth;

#[async_trait]
impl ExternalAuth for TestExternalApiKeyAuth {
    fn auth_mode(&self) -> AuthMode {
        AuthMode::ApiKey
    }

    async fn resolve(&self) -> std::io::Result<Option<ExternalAuthTokens>> {
        Ok(Some(ExternalAuthTokens::access_token_only(
            "test-external-api-key",
        )))
    }

    async fn refresh(
        &self,
        _context: ExternalAuthRefreshContext,
    ) -> std::io::Result<ExternalAuthTokens> {
        Ok(ExternalAuthTokens::access_token_only(
            "test-external-api-key",
        ))
    }
}

#[derive(Debug)]
struct TestUnresolvedExternalApiKeyAuth;

#[async_trait]
impl ExternalAuth for TestUnresolvedExternalApiKeyAuth {
    fn auth_mode(&self) -> AuthMode {
        AuthMode::ApiKey
    }

    async fn refresh(
        &self,
        _context: ExternalAuthRefreshContext,
    ) -> std::io::Result<ExternalAuthTokens> {
        Err(std::io::Error::other("unresolved test auth"))
    }
}

#[async_trait]
impl ModelsEndpointClient for TestModelsEndpoint {
    fn cache_key(&self) -> String {
        self.cache_key.clone()
    }

    fn has_command_auth(&self) -> bool {
        self.has_command_auth
    }

    fn has_provider_auth(&self) -> bool {
        self.has_provider_auth
    }

    async fn list_models(&self, _client_version: &str) -> CoreResult<RemoteModelCatalog> {
        self.fetch_count.fetch_add(1, Ordering::SeqCst);
        let catalog = self
            .responses
            .lock()
            .expect("responses lock should not be poisoned")
            .pop_front()
            .unwrap_or_else(|| catalog_response(Vec::new()));
        Ok(catalog)
    }
}

fn openai_manager_for_tests(
    codex_home: std::path::PathBuf,
    endpoint_client: Arc<dyn ModelsEndpointClient>,
) -> OpenAiModelsManager {
    openai_manager_for_tests_with_auth(
        codex_home,
        endpoint_client,
        Some(AuthManager::from_auth_for_testing(
            CodexAuth::create_dummy_api_key_auth_for_testing(),
        )),
    )
}

fn openai_manager_for_tests_with_auth(
    codex_home: std::path::PathBuf,
    endpoint_client: Arc<dyn ModelsEndpointClient>,
    auth_manager: Option<Arc<AuthManager>>,
) -> OpenAiModelsManager {
    OpenAiModelsManager::new(codex_home, endpoint_client, auth_manager)
}

fn static_manager_for_tests(model_catalog: ModelsResponse) -> StaticModelsManager {
    StaticModelsManager::new(/*auth_manager*/ None, model_catalog)
}

#[tokio::test]
async fn get_model_info_tracks_fallback_usage() {
    let config = ModelsManagerConfig::default();
    let known_model = remote_model("provider-known", "Provider Known", /*priority*/ 0);
    let manager = static_manager_for_tests(ModelsResponse {
        models: vec![known_model],
    });

    let known = manager.get_model_info("provider-known", &config).await;
    assert!(!known.used_fallback_model_metadata);
    assert_eq!(known.slug, "provider-known");

    let codex_home = tempdir().expect("temp dir");
    let empty_manager = openai_manager_for_tests(
        codex_home.path().to_path_buf(),
        TestModelsEndpoint::new(Vec::new()),
    );
    assert_eq!(empty_manager.get_remote_models().await, Vec::new());

    let unknown = empty_manager
        .get_model_info("model-that-does-not-exist", &config)
        .await;
    assert!(unknown.used_fallback_model_metadata);
    assert_eq!(unknown.slug, "model-that-does-not-exist");
}

#[tokio::test]
async fn get_model_info_uses_bundled_metadata_when_provider_catalog_misses() {
    let config = ModelsManagerConfig::default();
    let codex_home = tempdir().expect("temp dir");
    let manager = openai_manager_for_tests(
        codex_home.path().to_path_buf(),
        TestModelsEndpoint::new(Vec::new()),
    );

    let model_info = manager.get_model_info("deepseek-v4-pro", &config).await;

    assert_eq!(model_info.slug, "deepseek-v4-pro");
    assert_eq!(model_info.display_name, "DeepSeek V4 Pro");
    assert!(!model_info.supported_reasoning_levels.is_empty());
    assert!(!model_info.used_fallback_model_metadata);
}

#[tokio::test]
async fn get_model_info_uses_custom_catalog() {
    let config = ModelsManagerConfig::default();
    let mut overlay = remote_model("gpt-overlay", "Overlay", /*priority*/ 0);
    overlay.supports_image_detail_original = true;

    let manager = static_manager_for_tests(ModelsResponse {
        models: vec![overlay],
    });

    let model_info = manager
        .get_model_info("gpt-overlay-experiment", &config)
        .await;

    assert_eq!(model_info.slug, "gpt-overlay-experiment");
    assert_eq!(model_info.display_name, "Overlay");
    assert_eq!(model_info.context_window, Some(272_000));
    assert!(model_info.supports_image_detail_original);
    assert!(!model_info.supports_parallel_tool_calls);
    assert!(!model_info.used_fallback_model_metadata);
}

#[tokio::test]
async fn get_model_info_matches_namespaced_suffix() {
    let config = ModelsManagerConfig::default();
    let mut remote = remote_model("gpt-image", "Image", /*priority*/ 0);
    remote.supports_image_detail_original = true;
    let manager = static_manager_for_tests(ModelsResponse {
        models: vec![remote],
    });
    let namespaced_model = "custom/gpt-image".to_string();

    let model_info = manager.get_model_info(&namespaced_model, &config).await;

    assert_eq!(model_info.slug, namespaced_model);
    assert!(model_info.supports_image_detail_original);
    assert!(!model_info.used_fallback_model_metadata);
}

#[tokio::test]
async fn get_model_info_matches_hyphenated_provider_namespace_suffix() {
    let config = ModelsManagerConfig::default();
    let remote = remote_model("gpt-image", "Image", /*priority*/ 0);
    let manager = static_manager_for_tests(ModelsResponse {
        models: vec![remote],
    });
    let namespaced_model = "openai-codex/gpt-image".to_string();

    let model_info = manager.get_model_info(&namespaced_model, &config).await;

    assert_eq!(model_info.slug, namespaced_model);
    assert!(!model_info.used_fallback_model_metadata);
}

#[tokio::test]
async fn get_model_info_rejects_multi_segment_namespace_suffix_matching() {
    let config = ModelsManagerConfig::default();
    let remote = remote_model("gpt-image", "Image", /*priority*/ 0);
    let manager = static_manager_for_tests(ModelsResponse {
        models: vec![remote],
    });
    let namespaced_model = "ns1/ns2/gpt-image".to_string();

    let model_info = manager.get_model_info(&namespaced_model, &config).await;

    assert_eq!(model_info.slug, namespaced_model);
    assert!(model_info.used_fallback_model_metadata);
}

#[tokio::test]
async fn get_model_info_uses_current_provider_capability_for_bare_model_name() {
    let mut models = BTreeMap::new();
    models.insert(
        "mimo/mimo-v2.5-pro".to_string(),
        ModelCapability {
            max_context_window: Some(1_000_000),
            supports_tools: Some(true),
            ..Default::default()
        },
    );
    let config = ModelsManagerConfig {
        model_provider_id: Some("mimo".to_string()),
        model_capabilities: Some(ModelCapabilitiesCache {
            version: 1,
            source: "test".to_string(),
            generated_at_unix_seconds: 0,
            models,
        }),
        ..Default::default()
    };
    let manager = static_manager_for_tests(ModelsResponse { models: Vec::new() });

    let model_info = manager.get_model_info("mimo-v2.5-pro", &config).await;

    assert_eq!(model_info.slug, "mimo-v2.5-pro");
    assert_eq!(model_info.max_context_window, Some(1_000_000));
    assert!(!model_info.used_fallback_model_metadata);
}

#[tokio::test]
async fn model_capability_vision_signal_overrides_global_input_modalities() {
    let mut models = BTreeMap::new();
    models.insert(
        "mimo/mimo-v2.5-pro".to_string(),
        ModelCapability {
            supports_vision: Some(false),
            ..Default::default()
        },
    );
    let config = ModelsManagerConfig {
        model_provider_id: Some("mimo".to_string()),
        model_input_modalities: Some(vec![InputModality::Text, InputModality::Image]),
        model_capabilities: Some(ModelCapabilitiesCache {
            version: 1,
            source: "test".to_string(),
            generated_at_unix_seconds: 0,
            models,
        }),
        ..Default::default()
    };
    let manager = static_manager_for_tests(ModelsResponse { models: Vec::new() });

    let model_info = manager.get_model_info("mimo-v2.5-pro", &config).await;

    assert_eq!(model_info.input_modalities, vec![InputModality::Text]);
}

#[tokio::test]
async fn refresh_available_models_sorts_by_priority() {
    let remote_models = vec![
        remote_model("priority-low", "Low", /*priority*/ 1),
        remote_model("priority-high", "High", /*priority*/ 0),
    ];
    let codex_home = tempdir().expect("temp dir");
    let endpoint = TestModelsEndpoint::new(vec![remote_models.clone()]);
    let manager = openai_manager_for_tests(codex_home.path().to_path_buf(), endpoint.clone());

    manager
        .refresh_available_models(RefreshStrategy::OnlineIfUncached)
        .await
        .expect("refresh succeeds");
    let cached_remote = manager.get_remote_models().await;
    assert_models_contain(&cached_remote, &remote_models);

    let available = manager.list_models(RefreshStrategy::OnlineIfUncached).await;
    let high_idx = available
        .iter()
        .position(|model| model.model == "priority-high")
        .expect("priority-high should be listed");
    let low_idx = available
        .iter()
        .position(|model| model.model == "priority-low")
        .expect("priority-low should be listed");
    assert!(
        high_idx < low_idx,
        "higher priority should be listed before lower priority"
    );
    assert_eq!(endpoint.fetch_count(), 1, "expected a single model fetch");
}

#[tokio::test]
async fn refresh_available_models_uses_visible_provider_catalog() {
    let remote_models = vec![remote_model(
        "provider-visible-model",
        "Provider Visible",
        /*priority*/ 0,
    )];
    let codex_home = tempdir().expect("temp dir");
    let endpoint = TestModelsEndpoint::new(vec![remote_models.clone()]);
    let manager = openai_manager_for_tests(codex_home.path().to_path_buf(), endpoint.clone());
    manager
        .refresh_available_models(RefreshStrategy::OnlineIfUncached)
        .await
        .expect("refresh succeeds");

    assert_eq!(manager.get_remote_models().await, remote_models);
    assert_eq!(endpoint.fetch_count(), 1, "expected a single model fetch");
}

#[tokio::test]
async fn refresh_available_models_uses_cached_provider_catalog() {
    let remote_models = vec![remote_model(
        "provider-cached-model",
        "Provider Cached",
        /*priority*/ 0,
    )];
    let codex_home = tempdir().expect("temp dir");
    let fetch_endpoint = TestModelsEndpoint::new(vec![remote_models.clone()]);
    let fetch_manager =
        openai_manager_for_tests(codex_home.path().to_path_buf(), fetch_endpoint.clone());
    fetch_manager
        .refresh_available_models(RefreshStrategy::OnlineIfUncached)
        .await
        .expect("initial refresh succeeds");

    let cache_endpoint = TestModelsEndpoint::new(Vec::new());
    let cache_manager =
        openai_manager_for_tests(codex_home.path().to_path_buf(), cache_endpoint.clone());

    cache_manager
        .refresh_available_models(RefreshStrategy::OnlineIfUncached)
        .await
        .expect("cached refresh succeeds");

    assert_eq!(cache_manager.get_remote_models().await, remote_models);
    assert_eq!(
        cache_endpoint.fetch_count(),
        0,
        "fresh cache should avoid a model fetch"
    );
}

#[tokio::test]
async fn refresh_available_models_isolates_cache_by_provider_key() {
    let provider_a_models = vec![remote_model(
        "provider-a-cached-model",
        "Provider A Cached",
        /*priority*/ 0,
    )];
    let provider_b_models = vec![remote_model(
        "provider-b-refreshed-model",
        "Provider B Refreshed",
        /*priority*/ 0,
    )];
    let codex_home = tempdir().expect("temp dir");
    let provider_a_endpoint =
        TestModelsEndpoint::with_cache_key("provider-a", vec![catalog_response(provider_a_models)]);
    let provider_a_manager =
        openai_manager_for_tests(codex_home.path().to_path_buf(), provider_a_endpoint.clone());
    provider_a_manager
        .refresh_available_models(RefreshStrategy::OnlineIfUncached)
        .await
        .expect("provider a refresh succeeds");

    let provider_b_endpoint = TestModelsEndpoint::with_cache_key(
        "provider-b",
        vec![catalog_response(provider_b_models.clone())],
    );
    let provider_b_manager =
        openai_manager_for_tests(codex_home.path().to_path_buf(), provider_b_endpoint.clone());

    provider_b_manager
        .refresh_available_models(RefreshStrategy::OnlineIfUncached)
        .await
        .expect("provider b refresh succeeds");

    assert_eq!(
        provider_b_manager.get_remote_models().await,
        provider_b_models
    );
    assert_eq!(
        provider_b_endpoint.fetch_count(),
        1,
        "different provider key should not reuse another provider cache"
    );
}

#[tokio::test]
async fn get_model_info_uses_fallback_when_model_is_absent_from_provider_catalog() {
    let remote_models = vec![remote_model(
        "provider-refreshed-model-info",
        "Provider Model Info",
        /*priority*/ 0,
    )];
    let codex_home = tempdir().expect("temp dir");
    let endpoint = TestModelsEndpoint::new(vec![remote_models]);
    let manager = openai_manager_for_tests(codex_home.path().to_path_buf(), endpoint);

    manager
        .refresh_available_models(RefreshStrategy::OnlineIfUncached)
        .await
        .expect("refresh succeeds");

    let model_info = manager
        .get_model_info("missing-from-provider", &ModelsManagerConfig::default())
        .await;

    assert_eq!(model_info.slug, "missing-from-provider");
    assert!(model_info.used_fallback_model_metadata);
    assert_eq!(model_info.context_window, None);
}

#[tokio::test]
async fn refresh_available_models_keeps_empty_catalog_for_empty_provider_remote() {
    let codex_home = tempdir().expect("temp dir");
    let endpoint = TestModelsEndpoint::new(vec![Vec::new()]);
    let manager = openai_manager_for_tests(codex_home.path().to_path_buf(), endpoint);

    manager
        .refresh_available_models(RefreshStrategy::OnlineIfUncached)
        .await
        .expect("refresh succeeds");

    assert_eq!(manager.get_remote_models().await, Vec::new());
}

#[tokio::test]
async fn refresh_available_models_keeps_current_catalog_when_provider_catalog_unavailable() {
    let remote_models = vec![remote_model(
        "provider-catalog-before-unavailable",
        "Provider Catalog",
        /*priority*/ 0,
    )];
    let codex_home = tempdir().expect("temp dir");
    let endpoint = TestModelsEndpoint::with_catalogs(vec![
        catalog_response(remote_models.clone()),
        RemoteModelCatalog::Unavailable,
    ]);
    let manager = openai_manager_for_tests(codex_home.path().to_path_buf(), endpoint.clone());
    manager
        .refresh_available_models(RefreshStrategy::Online)
        .await
        .expect("initial refresh succeeds");

    manager
        .refresh_available_models(RefreshStrategy::Online)
        .await
        .expect("unavailable model catalog is not a refresh failure");

    assert_eq!(manager.get_remote_models().await, remote_models);
    assert_eq!(
        endpoint.fetch_count(),
        2,
        "unavailable model catalog should still count as an attempted fetch"
    );
}

#[tokio::test]
async fn refresh_available_models_uses_hidden_only_provider_remote() {
    let hidden_remote = remote_model_with_visibility(
        "provider-hidden-only",
        "Provider Hidden",
        /*priority*/ 0,
        "hide",
    );
    let codex_home = tempdir().expect("temp dir");
    let endpoint = TestModelsEndpoint::new(vec![vec![hidden_remote.clone()]]);
    let manager = openai_manager_for_tests(codex_home.path().to_path_buf(), endpoint);

    manager
        .refresh_available_models(RefreshStrategy::OnlineIfUncached)
        .await
        .expect("refresh succeeds");

    assert_eq!(manager.get_remote_models().await, vec![hidden_remote]);
}

#[tokio::test]
async fn refresh_available_models_keeps_merging_for_api_auth() {
    let remote_models = vec![remote_model(
        "api-auth-visible-remote",
        "API Auth Visible",
        /*priority*/ 0,
    )];
    let codex_home = tempdir().expect("temp dir");
    let endpoint = Arc::new(TestModelsEndpoint {
        cache_key: "api-auth-provider".to_string(),
        has_command_auth: true,
        has_provider_auth: false,
        responses: Mutex::new(vec![catalog_response(remote_models.clone())].into()),
        fetch_count: AtomicUsize::new(0),
    });
    let manager = openai_manager_for_tests_with_auth(
        codex_home.path().to_path_buf(),
        endpoint.clone(),
        Some(AuthManager::from_auth_for_testing(CodexAuth::from_api_key(
            "test-api-key",
        ))),
    );
    manager
        .refresh_available_models(RefreshStrategy::OnlineIfUncached)
        .await
        .expect("refresh succeeds");

    assert_eq!(manager.get_remote_models().await, remote_models);
    assert_eq!(endpoint.fetch_count(), 1, "expected a single model fetch");
}

#[tokio::test]
async fn refresh_available_models_uses_cache_when_fresh() {
    let remote_models = vec![remote_model("cached", "Cached", /*priority*/ 5)];
    let codex_home = tempdir().expect("temp dir");
    let endpoint = TestModelsEndpoint::new(vec![remote_models.clone()]);
    let manager = openai_manager_for_tests(codex_home.path().to_path_buf(), endpoint.clone());

    manager
        .refresh_available_models(RefreshStrategy::OnlineIfUncached)
        .await
        .expect("first refresh succeeds");
    assert_models_contain(&manager.get_remote_models().await, &remote_models);

    // Second call should read from cache and avoid the network.
    manager
        .refresh_available_models(RefreshStrategy::OnlineIfUncached)
        .await
        .expect("cached refresh succeeds");
    assert_models_contain(&manager.get_remote_models().await, &remote_models);
    assert_eq!(
        endpoint.fetch_count(),
        1,
        "cache hit should avoid a second model fetch"
    );
}

#[tokio::test]
async fn refresh_available_models_refetches_when_cache_stale() {
    let initial_models = vec![remote_model("stale", "Stale", /*priority*/ 1)];
    let codex_home = tempdir().expect("temp dir");
    let updated_models = vec![remote_model("fresh", "Fresh", /*priority*/ 9)];
    let endpoint = TestModelsEndpoint::new(vec![initial_models.clone(), updated_models.clone()]);
    let manager = openai_manager_for_tests(codex_home.path().to_path_buf(), endpoint.clone());

    manager
        .refresh_available_models(RefreshStrategy::OnlineIfUncached)
        .await
        .expect("initial refresh succeeds");

    // Rewrite cache with an old timestamp so it is treated as stale.
    manager
        .cache_manager
        .manipulate_cache_for_test(|fetched_at| {
            *fetched_at = Utc::now() - chrono::Duration::hours(1);
        })
        .await
        .expect("cache manipulation succeeds");

    manager
        .refresh_available_models(RefreshStrategy::OnlineIfUncached)
        .await
        .expect("second refresh succeeds");
    assert_models_contain(&manager.get_remote_models().await, &updated_models);
    assert_eq!(
        endpoint.fetch_count(),
        2,
        "stale cache refresh should fetch models again"
    );
}

#[tokio::test]
async fn refresh_available_models_refetches_when_version_mismatch() {
    let initial_models = vec![remote_model("old", "Old", /*priority*/ 1)];
    let codex_home = tempdir().expect("temp dir");
    let updated_models = vec![remote_model("new", "New", /*priority*/ 2)];
    let endpoint = TestModelsEndpoint::new(vec![initial_models.clone(), updated_models.clone()]);
    let manager = openai_manager_for_tests(codex_home.path().to_path_buf(), endpoint.clone());

    manager
        .refresh_available_models(RefreshStrategy::OnlineIfUncached)
        .await
        .expect("initial refresh succeeds");

    manager
        .cache_manager
        .mutate_cache_for_test(|cache| {
            let client_version = crate::client_version_to_whole();
            cache.client_version = Some(format!("{client_version}-mismatch"));
        })
        .await
        .expect("cache mutation succeeds");

    manager
        .refresh_available_models(RefreshStrategy::OnlineIfUncached)
        .await
        .expect("second refresh succeeds");
    assert_models_contain(&manager.get_remote_models().await, &updated_models);
    assert_eq!(
        endpoint.fetch_count(),
        2,
        "version mismatch should fetch models again"
    );
}

#[tokio::test]
async fn refresh_available_models_drops_removed_remote_models() {
    let initial_models = vec![remote_model(
        "remote-old",
        "Remote Old",
        /*priority*/ 1,
    )];
    let codex_home = tempdir().expect("temp dir");
    let refreshed_models = vec![remote_model(
        "remote-new",
        "Remote New",
        /*priority*/ 1,
    )];
    let endpoint = TestModelsEndpoint::new(vec![initial_models, refreshed_models]);
    let mut manager = openai_manager_for_tests(codex_home.path().to_path_buf(), endpoint.clone());
    manager.cache_manager.set_ttl(Duration::ZERO);

    manager
        .refresh_available_models(RefreshStrategy::OnlineIfUncached)
        .await
        .expect("initial refresh succeeds");

    manager
        .refresh_available_models(RefreshStrategy::OnlineIfUncached)
        .await
        .expect("second refresh succeeds");

    let available = manager
        .try_list_models()
        .expect("models should be available");
    assert!(
        available.iter().any(|preset| preset.model == "remote-new"),
        "new remote model should be listed"
    );
    assert!(
        !available.iter().any(|preset| preset.model == "remote-old"),
        "removed remote model should not be listed"
    );
    assert_eq!(
        endpoint.fetch_count(),
        2,
        "second refresh should fetch models again"
    );
}

#[tokio::test]
async fn refresh_available_models_skips_network_without_provider_refresh_auth() {
    let dynamic_slug = "dynamic-model-only-for-test-noauth";
    let codex_home = tempdir().expect("temp dir");
    let endpoint = TestModelsEndpoint::without_refresh(vec![vec![remote_model(
        dynamic_slug,
        "No Auth",
        /*priority*/ 1,
    )]]);
    let manager = openai_manager_for_tests_with_auth(
        codex_home.path().to_path_buf(),
        endpoint.clone(),
        /*auth_manager*/ None,
    );

    manager
        .refresh_available_models(RefreshStrategy::Online)
        .await
        .expect("refresh should no-op without provider refresh auth");
    let cached_remote = manager.get_remote_models().await;
    assert!(
        !cached_remote
            .iter()
            .any(|candidate| candidate.slug == dynamic_slug),
        "remote refresh should be skipped without provider refresh auth"
    );
    assert_eq!(
        endpoint.fetch_count(),
        0,
        "endpoint that cannot refresh should avoid model fetches"
    );
}

#[derive(Debug)]
struct TestNoRefreshAuthModelsEndpoint {
    cache_key: String,
    responses: Mutex<VecDeque<RemoteModelCatalog>>,
    fetch_count: AtomicUsize,
}

impl TestNoRefreshAuthModelsEndpoint {
    fn new(responses: Vec<Vec<ModelInfo>>) -> Arc<Self> {
        Arc::new(Self {
            cache_key: "test-no-refresh-provider".to_string(),
            responses: Mutex::new(responses.into_iter().map(catalog_response).collect()),
            fetch_count: AtomicUsize::new(0),
        })
    }

    fn fetch_count(&self) -> usize {
        self.fetch_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ModelsEndpointClient for TestNoRefreshAuthModelsEndpoint {
    fn cache_key(&self) -> String {
        self.cache_key.clone()
    }

    fn has_command_auth(&self) -> bool {
        false
    }

    async fn list_models(&self, _client_version: &str) -> CoreResult<RemoteModelCatalog> {
        self.fetch_count.fetch_add(1, Ordering::SeqCst);
        let catalog = self
            .responses
            .lock()
            .expect("responses lock should not be poisoned")
            .pop_front()
            .unwrap_or_else(|| catalog_response(Vec::new()));
        Ok(catalog)
    }
}

#[tokio::test]
async fn refresh_available_models_skips_network_when_external_api_key_overrides_cached_hosted_auth()
{
    let dynamic_slug = "dynamic-model-only-for-test-external-api-key";
    let codex_home = tempdir().expect("temp dir");
    let auth_manager =
        AuthManager::from_auth_for_testing(CodexAuth::create_dummy_api_key_auth_for_testing());
    auth_manager.set_external_auth(Arc::new(TestExternalApiKeyAuth));
    let endpoint = TestNoRefreshAuthModelsEndpoint::new(vec![vec![remote_model(
        dynamic_slug,
        "External API Key",
        /*priority*/ 1,
    )]]);
    let manager = openai_manager_for_tests_with_auth(
        codex_home.path().to_path_buf(),
        endpoint.clone(),
        Some(auth_manager),
    );

    manager
        .refresh_available_models(RefreshStrategy::Online)
        .await
        .expect("refresh should no-op with API key auth");
    let cached_remote = manager.get_remote_models().await;

    assert!(
        !cached_remote
            .iter()
            .any(|candidate| candidate.slug == dynamic_slug),
        "remote refresh should be skipped when external API key auth is active"
    );
    assert_eq!(
        endpoint.fetch_count(),
        0,
        "endpoint should avoid model fetches when external API key auth is active"
    );
}

#[tokio::test]
async fn refresh_available_models_skips_network_when_external_api_key_is_unresolved() {
    let dynamic_slug = "dynamic-model-only-for-test-unresolved-external-api-key";
    let codex_home = tempdir().expect("temp dir");
    let auth_manager =
        AuthManager::from_auth_for_testing(CodexAuth::create_dummy_api_key_auth_for_testing());
    auth_manager.set_external_auth(Arc::new(TestUnresolvedExternalApiKeyAuth));
    let endpoint = TestNoRefreshAuthModelsEndpoint::new(vec![vec![remote_model(
        dynamic_slug,
        "Unresolved External API Key",
        /*priority*/ 1,
    )]]);
    let manager = openai_manager_for_tests_with_auth(
        codex_home.path().to_path_buf(),
        endpoint.clone(),
        Some(auth_manager),
    );

    manager
        .refresh_available_models(RefreshStrategy::Online)
        .await
        .expect("refresh should no-op with unresolved external API key auth");

    assert!(
        !manager
            .get_remote_models()
            .await
            .iter()
            .any(|candidate| candidate.slug == dynamic_slug),
        "remote refresh should be skipped when external API key auth cannot resolve"
    );
    assert_eq!(
        endpoint.fetch_count(),
        0,
        "endpoint should avoid model fetches when external API key auth cannot resolve"
    );
}

#[tokio::test]
async fn refresh_available_models_fetches_with_provider_auth() {
    let dynamic_slug = "dynamic-model-only-for-test-provider-auth";
    let codex_home = tempdir().expect("temp dir");
    let endpoint = TestModelsEndpoint::with_provider_auth(vec![vec![remote_model(
        dynamic_slug,
        "Provider Auth",
        /*priority*/ 1,
    )]]);
    let manager = openai_manager_for_tests_with_auth(
        codex_home.path().to_path_buf(),
        endpoint.clone(),
        /*auth_manager*/ None,
    );

    manager
        .refresh_available_models(RefreshStrategy::Online)
        .await
        .expect("refresh should fetch with provider auth");

    assert!(
        manager
            .get_remote_models()
            .await
            .iter()
            .any(|candidate| candidate.slug == dynamic_slug),
        "remote refresh should include models fetched with provider auth"
    );
    assert_eq!(
        endpoint.fetch_count(),
        1,
        "endpoint should fetch models with provider auth"
    );
}

#[test]
fn build_available_models_picks_default_after_hiding_hidden_models() {
    let manager = static_manager_for_tests(ModelsResponse { models: Vec::new() });

    let hidden_model =
        remote_model_with_visibility("hidden", "Hidden", /*priority*/ 0, "hide");
    let visible_model =
        remote_model_with_visibility("visible", "Visible", /*priority*/ 1, "list");

    let expected_hidden = ModelPreset::from(hidden_model.clone());
    let mut expected_visible = ModelPreset::from(visible_model.clone());
    expected_visible.is_default = true;

    let available = manager.build_available_models(vec![hidden_model, visible_model]);

    assert_eq!(available, vec![expected_hidden, expected_visible]);
}

#[tokio::test]
async fn static_manager_hides_models_not_supported_in_api_even_with_cached_hosted_auth() {
    let auth_manager =
        AuthManager::from_auth_for_testing(CodexAuth::create_dummy_api_key_auth_for_testing());
    let hosted_only_model = {
        let mut model = remote_model("hosted-only", "Hosted Only", /*priority*/ 0);
        model.supported_in_api = false;
        model
    };
    let api_model = remote_model("api-model", "API Model", /*priority*/ 1);
    let manager = StaticModelsManager::new(
        Some(Arc::clone(&auth_manager)),
        ModelsResponse {
            models: vec![hosted_only_model, api_model],
        },
    );

    let cached_hosted_auth_models = manager.list_models(RefreshStrategy::Online).await;
    assert_eq!(
        cached_hosted_auth_models
            .iter()
            .map(|model| model.model.as_str())
            .collect::<Vec<_>>(),
        vec!["api-model"]
    );

    auth_manager.set_external_auth(Arc::new(TestExternalApiKeyAuth));
    let api_models = manager.list_models(RefreshStrategy::Online).await;

    assert_eq!(
        api_models
            .iter()
            .map(|model| model.model.as_str())
            .collect::<Vec<_>>(),
        vec!["api-model"]
    );
}

#[test]
fn bundled_models_json_roundtrips() {
    let response = crate::bundled_models_response()
        .unwrap_or_else(|err| panic!("bundled models.json should parse: {err}"));

    let serialized =
        serde_json::to_string(&response).expect("bundled models.json should serialize");
    let roundtripped: ModelsResponse =
        serde_json::from_str(&serialized).expect("serialized models.json should deserialize");

    assert_eq!(
        response, roundtripped,
        "bundled models.json should round trip through serde"
    );
    assert!(
        !response.models.is_empty(),
        "bundled models.json should contain at least one model"
    );
}
