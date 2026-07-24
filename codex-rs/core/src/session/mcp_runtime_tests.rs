use super::*;
use codex_features::Feature;
use rmcp::model::ElicitationCapability;

impl McpRuntimeSnapshot {
    pub(crate) fn new_uninitialized_for_test(config: &crate::config::Config) -> Arc<Self> {
        let mcp_config = McpConfig {
            hosted_base_url: config.hosted_base_url.clone(),
            apps_mcp_path_override: config.apps_mcp_path_override.clone(),
            apps_mcp_product_sku: config.apps_mcp_product_sku.clone(),
            codex_home: config.codex_home.to_path_buf(),
            mcp_oauth_credentials_store_mode: config.mcp_oauth_credentials_store_mode,
            mcp_oauth_callback_port: config.mcp_oauth_callback_port,
            mcp_oauth_callback_url: config.mcp_oauth_callback_url.clone(),
            skill_mcp_dependency_install_enabled: config
                .features
                .enabled(Feature::SkillMcpDependencyInstall),
            approval_policy: config.permissions.approval_policy.clone(),
            codex_linux_sandbox_exe: config.codex_linux_sandbox_exe.clone(),
            use_legacy_landlock: config.features.use_legacy_landlock(),
            apps_enabled: config.features.enabled(Feature::Apps),
            prefix_mcp_tool_names: config.prefix_mcp_tool_names(),
            client_elicitation_capability: ElicitationCapability::default(),
            configured_mcp_servers: config.mcp_servers.get().clone(),
            plugin_ids_by_mcp_server_name: Default::default(),
            plugin_capability_summaries: Vec::new(),
        };
        let manager = McpConnectionManager::new_uninitialized_with_permission_profile(
            &config.permissions.approval_policy,
            config.permissions.permission_profile(),
            config.prefix_mcp_tool_names(),
        );
        Arc::new(Self::new(Arc::new(mcp_config), Arc::new(manager)))
    }
}
