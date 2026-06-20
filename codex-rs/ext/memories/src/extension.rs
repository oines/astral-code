use std::sync::Arc;
use std::sync::Weak;

use codex_config::types::CompactMemoryMode;
use codex_core::ThreadManager;
use codex_core::config::Config;
use codex_extension_api::CompactLifecycleContributor;
use codex_extension_api::CompactStartInput;
use codex_extension_api::ConfigContributor;
use codex_extension_api::ContextContributor;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::PromptFragment;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ToolContributor;
use codex_features::Feature;
use codex_login::AuthManager;
use codex_otel::MetricsClient;
use codex_utils_absolute_path::AbsolutePathBuf;
use tracing::warn;

use crate::local::LocalMemoriesBackend;
use crate::prompts::build_memory_tool_developer_instructions;
use crate::tools;

/// Contributes Codex memory read-path prompt context and memory read tools.
#[derive(Clone, Default)]
pub(crate) struct MemoriesExtension {
    metrics_client: Option<MetricsClient>,
    compact_memory: Option<CompactMemoryRunner>,
}

impl MemoriesExtension {
    fn new(metrics_client: Option<MetricsClient>) -> Self {
        Self {
            metrics_client,
            compact_memory: None,
        }
    }

    fn with_compact_memory(
        metrics_client: Option<MetricsClient>,
        auth_manager: Arc<AuthManager>,
        thread_manager: Weak<ThreadManager>,
    ) -> Self {
        Self {
            metrics_client,
            compact_memory: Some(CompactMemoryRunner {
                auth_manager,
                thread_manager,
            }),
        }
    }
}

#[derive(Clone)]
struct CompactMemoryRunner {
    auth_manager: Arc<AuthManager>,
    thread_manager: Weak<ThreadManager>,
}

#[derive(Clone, Debug)]
pub(crate) struct MemoriesExtensionConfig {
    pub(crate) enabled: bool,
    pub(crate) dedicated_tools: bool,
    pub(crate) codex_home: AbsolutePathBuf,
    pub(crate) compact_memory: CompactMemoryMode,
}

impl MemoriesExtensionConfig {
    fn from_config(config: &Config) -> Self {
        Self {
            enabled: config.features.enabled(Feature::MemoryTool) && config.memories.use_memories,
            dedicated_tools: config.memories.dedicated_tools,
            codex_home: config.codex_home.clone(),
            compact_memory: config.memories.compact_memory,
        }
    }
}

impl ContextContributor for MemoriesExtension {
    fn contribute<'a>(
        &'a self,
        _session_store: &'a ExtensionData,
        thread_store: &'a ExtensionData,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<PromptFragment>> + Send + 'a>> {
        Box::pin(async move {
            let Some(config) = thread_store.get::<MemoriesExtensionConfig>() else {
                return Vec::new();
            };
            if !config.enabled {
                return Vec::new();
            }

            build_memory_tool_developer_instructions(&config.codex_home)
                .await
                .map(PromptFragment::developer_policy)
                .into_iter()
                .collect()
        })
    }
}

#[async_trait::async_trait]
impl ThreadLifecycleContributor<Config> for MemoriesExtension {
    async fn on_thread_start(&self, input: ThreadStartInput<'_, Config>) {
        input
            .thread_store
            .insert(MemoriesExtensionConfig::from_config(input.config));
    }
}

impl ConfigContributor<Config> for MemoriesExtension {
    fn on_config_changed(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
        _previous_config: &Config,
        new_config: &Config,
    ) {
        thread_store.insert(MemoriesExtensionConfig::from_config(new_config));
    }
}

impl ToolContributor for MemoriesExtension {
    fn tools(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
    ) -> Vec<Arc<dyn codex_extension_api::ToolExecutor<codex_extension_api::ToolCall>>> {
        let Some(config) = thread_store.get::<MemoriesExtensionConfig>() else {
            return Vec::new();
        };
        if !config.enabled || !config.dedicated_tools {
            return Vec::new();
        }

        tools::memory_tools(
            LocalMemoriesBackend::from_codex_home(&config.codex_home),
            self.metrics_client.clone(),
        )
    }
}

#[async_trait::async_trait]
impl CompactLifecycleContributor for MemoriesExtension {
    async fn before_compact(&self, input: CompactStartInput<'_>) {
        let Some(runner) = self.compact_memory.as_ref() else {
            return;
        };
        let Some(thread_manager) = runner.thread_manager.upgrade() else {
            warn!("compact memory skipped: thread manager is unavailable");
            return;
        };
        let config = if let Some(config) = input.thread_store.get::<MemoriesExtensionConfig>() {
            config
        } else {
            let thread = match thread_manager.get_thread(input.thread_id).await {
                Ok(thread) => thread,
                Err(err) => {
                    warn!(
                        "compact memory skipped: failed to load thread config for {}: {err}",
                        input.thread_id
                    );
                    return;
                }
            };
            let thread_config = thread.config().await;
            let config = MemoriesExtensionConfig::from_config(thread_config.as_ref());
            input.thread_store.insert(config);
            match input.thread_store.get::<MemoriesExtensionConfig>() {
                Some(config) => config,
                None => {
                    warn!(
                        "compact memory skipped: failed to initialize memories extension config for {}",
                        input.thread_id
                    );
                    return;
                }
            }
        };
        if !config.enabled {
            warn!("compact memory skipped: memories are disabled for this thread");
            return;
        }
        match config.compact_memory {
            CompactMemoryMode::Off => {}
            CompactMemoryMode::Enqueue => {
                codex_memories_write::start_compact_memory_task(
                    thread_manager,
                    Arc::clone(&runner.auth_manager),
                    input.thread_id,
                );
            }
            CompactMemoryMode::Blocking => {
                codex_memories_write::run_compact_memory_task(
                    thread_manager,
                    Arc::clone(&runner.auth_manager),
                    input.thread_id,
                )
                .await;
            }
        }
    }
}

/// Installs the memories extension contributors into the extension registry.
pub fn install(
    registry: &mut ExtensionRegistryBuilder<Config>,
    metrics_client: Option<MetricsClient>,
) {
    let extension = Arc::new(MemoriesExtension::new(metrics_client));
    install_extension(registry, extension);
}

/// Installs the memories extension with compact-triggered memory extraction.
pub fn install_with_compact_memory(
    registry: &mut ExtensionRegistryBuilder<Config>,
    metrics_client: Option<MetricsClient>,
    auth_manager: Arc<AuthManager>,
    thread_manager: Weak<ThreadManager>,
) {
    let extension = Arc::new(MemoriesExtension::with_compact_memory(
        metrics_client,
        auth_manager,
        thread_manager,
    ));
    install_extension(registry, extension);
}

fn install_extension(
    registry: &mut ExtensionRegistryBuilder<Config>,
    extension: Arc<MemoriesExtension>,
) {
    registry.thread_lifecycle_contributor(extension.clone());
    registry.config_contributor(extension.clone());
    registry.prompt_contributor(extension.clone());
    registry.tool_contributor(extension.clone());
    registry.compact_lifecycle_contributor(extension);
}
