use std::sync::Arc;

use codex_config::config_toml::WebSearchRuntimeConfig;
use codex_core::config::Config;
use codex_extension_api::ConfigContributor;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ToolContributor;
use codex_login::AuthManager;
use codex_protocol::config_types::WebSearchMode;

use crate::tool::WebFetchTool;
use crate::tool::WebSearchTool;

#[derive(Clone)]
struct WebSearchExtension;

#[derive(Clone)]
struct WebSearchExtensionConfig {
    available: bool,
    runtime_config: Option<WebSearchRuntimeConfig>,
}

impl From<&Config> for WebSearchExtensionConfig {
    fn from(config: &Config) -> Self {
        let runtime_config = config.web_search_runtime_config.clone();
        Self {
            available: config.web_search_mode.value() == WebSearchMode::Live
                && runtime_config.is_some(),
            runtime_config,
        }
    }
}

#[async_trait::async_trait]
impl ThreadLifecycleContributor<Config> for WebSearchExtension {
    async fn on_thread_start(&self, input: ThreadStartInput<'_, Config>) {
        input
            .thread_store
            .insert(WebSearchExtensionConfig::from(input.config));
    }
}

impl ConfigContributor<Config> for WebSearchExtension {
    fn on_config_changed(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
        _previous_config: &Config,
        new_config: &Config,
    ) {
        thread_store.insert(WebSearchExtensionConfig::from(new_config));
    }
}

impl ToolContributor for WebSearchExtension {
    fn tools(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
    ) -> Vec<Arc<dyn codex_extension_api::ToolExecutor<codex_extension_api::ToolCall>>> {
        let Some(config) = thread_store.get::<WebSearchExtensionConfig>() else {
            return Vec::new();
        };
        if !config.available {
            return Vec::new();
        }
        let Some(runtime_config) = config.runtime_config.clone() else {
            return Vec::new();
        };

        let client = build_web_client();
        vec![
            Arc::new(WebSearchTool {
                client: client.clone(),
                config: runtime_config,
            }),
            Arc::new(WebFetchTool { client }),
        ]
    }
}

pub fn install(registry: &mut ExtensionRegistryBuilder<Config>, _auth_manager: Arc<AuthManager>) {
    let extension = Arc::new(WebSearchExtension);
    registry.thread_lifecycle_contributor(extension.clone());
    registry.config_contributor(extension.clone());
    registry.tool_contributor(extension);
}

fn build_web_client() -> reqwest::Client {
    reqwest::Client::builder()
        .default_headers(codex_login::default_client::default_headers())
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_else(|error| {
            tracing::warn!(error = %error, "failed to build web tool HTTP client");
            reqwest::Client::new()
        })
}

#[cfg(test)]
mod tests {
    use codex_config::config_toml::SecretString;
    use codex_config::config_toml::WebSearchProvider;
    use codex_config::config_toml::WebSearchRuntimeConfig;
    use codex_extension_api::ExtensionData;
    use codex_extension_api::ExtensionRegistryBuilder;
    use codex_extension_api::ToolName;
    use codex_login::CodexAuth;
    use pretty_assertions::assert_eq;

    use super::AuthManager;
    use super::Config;
    use super::WebSearchExtensionConfig;
    use super::install;
    use crate::tool::FETCH_TOOL_NAME;
    use crate::tool::SEARCH_TOOL_NAME;
    use crate::tool::WEB_NAMESPACE;

    #[test]
    fn installed_extension_contributes_web_search_and_fetch_when_enabled() {
        let mut builder = ExtensionRegistryBuilder::<Config>::new();
        install(
            &mut builder,
            AuthManager::from_auth_for_testing(CodexAuth::from_api_key("dummy")),
        );
        let registry = builder.build();
        let session_store = ExtensionData::new("session");
        let thread_store = ExtensionData::new("11111111-1111-4111-8111-111111111111");
        thread_store.insert(WebSearchExtensionConfig {
            available: true,
            runtime_config: Some(WebSearchRuntimeConfig {
                provider: WebSearchProvider::Tavily,
                api_key: SecretString::new("secret".to_string()).expect("secret should be valid"),
                default_limit: 5,
                max_limit: 20,
            }),
        });

        let tool_names = registry
            .tool_contributors()
            .iter()
            .flat_map(|contributor| contributor.tools(&session_store, &thread_store))
            .map(|tool| (tool.tool_name(), tool.supports_parallel_tool_calls()))
            .collect::<Vec<_>>();

        assert_eq!(
            tool_names,
            vec![
                (ToolName::namespaced(WEB_NAMESPACE, SEARCH_TOOL_NAME), true),
                (ToolName::namespaced(WEB_NAMESPACE, FETCH_TOOL_NAME), true),
            ]
        );
    }
}
