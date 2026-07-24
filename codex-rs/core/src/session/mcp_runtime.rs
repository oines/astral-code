use std::fmt;
use std::sync::Arc;

use codex_mcp::McpConfig;
use codex_mcp::McpConnectionManager;

/// MCP config and manager used by one model request.
pub struct McpRuntimeSnapshot {
    config: Arc<McpConfig>,
    manager: Arc<McpConnectionManager>,
}

impl McpRuntimeSnapshot {
    pub(crate) fn new(config: Arc<McpConfig>, manager: Arc<McpConnectionManager>) -> Self {
        Self { config, manager }
    }

    pub fn config(&self) -> &McpConfig {
        self.config.as_ref()
    }

    pub fn manager(&self) -> &McpConnectionManager {
        self.manager.as_ref()
    }

    pub(crate) fn manager_arc(&self) -> Arc<McpConnectionManager> {
        Arc::clone(&self.manager)
    }
}

impl fmt::Debug for McpRuntimeSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("McpRuntimeSnapshot")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "mcp_runtime_tests.rs"]
mod tests;
