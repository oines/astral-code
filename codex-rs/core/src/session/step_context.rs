use std::sync::Arc;

use crate::agents_md::LoadedAgentsMd;
use crate::environment_selection::TurnEnvironmentSnapshot;
use crate::session::McpRuntimeSnapshot;
use crate::session::PreviousTurnSettings;
use crate::session::turn_context::TurnContext;
use codex_execpolicy::Policy;
use codex_mcp::ToolInfo;
use codex_protocol::protocol::TurnContextItem;
use tokio::sync::OnceCell;

/// Request-scoped state that may change between model sampling requests.
#[derive(Debug)]
pub(crate) struct StepContext {
    pub(crate) turn: Arc<TurnContext>,
    pub(crate) environments: TurnEnvironmentSnapshot,
    /// The canonical AGENTS.md value observed with this environment snapshot.
    pub(crate) loaded_agents_md: Option<Arc<LoadedAgentsMd>>,
    /// The exact MCP config and manager used to advertise and execute tools for this step.
    pub(crate) mcp: Arc<McpRuntimeSnapshot>,
    /// Resolved session instructions and command policy used by this sampling step.
    pub(crate) base_instructions: String,
    pub(crate) exec_policy: Arc<Policy>,
    /// Session-history baselines observed by this exact sampling step.
    pub(crate) reference_context_item: Option<TurnContextItem>,
    pub(crate) previous_turn_settings: Option<PreviousTurnSettings>,
    /// The fixed MCP tool list used for this exact sampling request.
    mcp_tool_snapshot: OnceCell<Vec<ToolInfo>>,
}

impl StepContext {
    pub(crate) fn new(
        turn: Arc<TurnContext>,
        environments: TurnEnvironmentSnapshot,
        loaded_agents_md: Option<Arc<LoadedAgentsMd>>,
        mcp: Arc<McpRuntimeSnapshot>,
        base_instructions: String,
        exec_policy: Arc<Policy>,
        reference_context_item: Option<TurnContextItem>,
        previous_turn_settings: Option<PreviousTurnSettings>,
    ) -> Self {
        Self {
            turn,
            environments,
            loaded_agents_md,
            mcp,
            base_instructions,
            exec_policy,
            reference_context_item,
            previous_turn_settings,
            mcp_tool_snapshot: OnceCell::new(),
        }
    }

    pub(crate) async fn mcp_tools(&self) -> &[ToolInfo] {
        self.mcp_tool_snapshot
            .get_or_init(|| self.mcp.manager().list_all_tools())
            .await
    }
}
