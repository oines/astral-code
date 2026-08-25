use std::sync::Arc;

use codex_api::AllowedCaller;
use codex_api::ApproximateLocation;
use codex_api::ExternalWebAccess;
use codex_api::ExternalWebAccessMode;
use codex_api::LocationType;
use codex_api::SearchContextSize;
use codex_api::SearchFilters;
use codex_api::SearchSettings;
use codex_core::config::Config;
use codex_extension_api::ConfigContributor;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ToolContributor;
use codex_login::AuthManager;
use codex_model_provider::create_model_provider;
use codex_model_provider_info::ManagedAuthKind;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::config_types::WebSearchContextSize;
use codex_protocol::config_types::WebSearchMode;

use crate::codex_tool::CodexWebSearchTool;

#[derive(Clone)]
struct CodexWebSearchExtension {
    auth_manager: Arc<AuthManager>,
}

#[derive(Clone)]
struct CodexWebSearchExtensionConfig {
    available: bool,
    provider: ModelProviderInfo,
    settings: SearchSettings,
}

impl From<&Config> for CodexWebSearchExtensionConfig {
    fn from(config: &Config) -> Self {
        let web_search_mode = config.web_search_mode.value();
        Self {
            // Core applies the per-turn model and Full/Lite selection policy.
            available: config.model_provider.managed_auth == Some(ManagedAuthKind::CodexOAuth)
                && web_search_mode != WebSearchMode::Disabled,
            provider: config.model_provider.clone(),
            settings: search_settings(config, web_search_mode),
        }
    }
}

fn search_settings(config: &Config, web_search_mode: WebSearchMode) -> SearchSettings {
    let web_search_config = config.web_search_config.as_ref();
    SearchSettings {
        user_location: web_search_config
            .and_then(|config| config.user_location.as_ref())
            .map(|location| ApproximateLocation {
                r#type: LocationType::Approximate,
                country: location.country.clone(),
                region: location.region.clone(),
                city: location.city.clone(),
                timezone: location.timezone.clone(),
            }),
        search_context_size: web_search_config
            .and_then(|config| config.search_context_size)
            .map(|size| match size {
                WebSearchContextSize::Low => SearchContextSize::Low,
                WebSearchContextSize::Medium => SearchContextSize::Medium,
                WebSearchContextSize::High => SearchContextSize::High,
            }),
        filters: web_search_config
            .and_then(|config| config.filters.as_ref())
            .map(|filters| SearchFilters {
                allowed_domains: filters.allowed_domains.clone(),
                blocked_domains: None,
            }),
        allowed_callers: Some(vec![AllowedCaller::Direct]),
        external_web_access: Some(external_web_access_for_mode(web_search_mode)),
        ..Default::default()
    }
}

fn external_web_access_for_mode(web_search_mode: WebSearchMode) -> ExternalWebAccess {
    match web_search_mode {
        WebSearchMode::Disabled | WebSearchMode::Cached => ExternalWebAccess::Boolean(false),
        WebSearchMode::Indexed => ExternalWebAccess::Mode(ExternalWebAccessMode::Indexed),
        WebSearchMode::Live => ExternalWebAccess::Boolean(true),
    }
}

#[async_trait::async_trait]
impl ThreadLifecycleContributor<Config> for CodexWebSearchExtension {
    async fn on_thread_start(&self, input: ThreadStartInput<'_, Config>) {
        input
            .thread_store
            .insert(CodexWebSearchExtensionConfig::from(input.config));
    }
}

impl ConfigContributor<Config> for CodexWebSearchExtension {
    fn on_config_changed(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
        _previous_config: &Config,
        new_config: &Config,
    ) {
        thread_store.insert(CodexWebSearchExtensionConfig::from(new_config));
    }
}

impl ToolContributor for CodexWebSearchExtension {
    fn tools(
        &self,
        session_store: &ExtensionData,
        thread_store: &ExtensionData,
    ) -> Vec<Arc<dyn codex_extension_api::ToolExecutor<codex_extension_api::ToolCall>>> {
        let Some(config) = thread_store.get::<CodexWebSearchExtensionConfig>() else {
            return Vec::new();
        };
        if !config.available {
            return Vec::new();
        }

        vec![Arc::new(CodexWebSearchTool {
            session_id: session_store.level_id().to_string(),
            provider: create_model_provider(
                config.provider.clone(),
                Some(self.auth_manager.clone()),
            ),
            settings: config.settings.clone(),
        })]
    }
}

pub(crate) fn install(
    registry: &mut ExtensionRegistryBuilder<Config>,
    auth_manager: Arc<AuthManager>,
) {
    let extension = Arc::new(CodexWebSearchExtension { auth_manager });
    registry.thread_lifecycle_contributor(extension.clone());
    registry.config_contributor(extension.clone());
    registry.tool_contributor(extension);
}

#[cfg(test)]
#[path = "codex_extension_tests.rs"]
mod tests;
