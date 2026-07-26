// Derived from xai-grok-pager/src/scrollback/blocks/subagent.rs at
// Grok Build commit 47348d13ec4508dcfe440e34c6d511bb02998fb2.
// Adapted to Astral's app-server v2 collab lifecycle; see the repository NOTICE.

use std::collections::HashMap;

use codex_app_server_protocol::CollabAgentState;
use codex_app_server_protocol::CollabAgentStatus;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;

use crate::ToolStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentAction {
    Spawn,
    SendInput,
    Resume,
    Wait,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentAgentStatus {
    Pending,
    Running,
    Interrupted,
    Completed,
    Failed,
    Shutdown,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentAgent {
    pub thread_id: String,
    pub status: SubagentAgentStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentPresentation {
    pub action: SubagentAction,
    pub status: ToolStatus,
    pub thread_ids: Vec<String>,
    pub prompt: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub agents: Vec<SubagentAgent>,
}

impl SubagentPresentation {
    pub(super) fn from_collab(
        tool: &CollabAgentTool,
        status: &CollabAgentToolCallStatus,
        thread_ids: &[String],
        prompt: Option<&str>,
        model: Option<&str>,
        reasoning_effort: Option<String>,
        agents_states: &HashMap<String, CollabAgentState>,
    ) -> Self {
        let action = match tool {
            CollabAgentTool::SpawnAgent => SubagentAction::Spawn,
            CollabAgentTool::SendInput => SubagentAction::SendInput,
            CollabAgentTool::ResumeAgent => SubagentAction::Resume,
            CollabAgentTool::Wait => SubagentAction::Wait,
            CollabAgentTool::CloseAgent => SubagentAction::Close,
        };
        let status = match status {
            CollabAgentToolCallStatus::InProgress => ToolStatus::Running,
            CollabAgentToolCallStatus::Completed => ToolStatus::Success,
            CollabAgentToolCallStatus::Failed => ToolStatus::Failed,
        };

        let mut agents = thread_ids
            .iter()
            .map(|thread_id| agent_from_state(thread_id, agents_states.get(thread_id)))
            .collect::<Vec<_>>();
        let mut extra_agents = agents_states
            .iter()
            .filter(|(thread_id, _)| !thread_ids.contains(thread_id))
            .map(|(thread_id, state)| agent_from_state(thread_id, Some(state)))
            .collect::<Vec<_>>();
        extra_agents.sort_by(|left, right| left.thread_id.cmp(&right.thread_id));
        agents.extend(extra_agents);

        Self {
            action,
            status,
            thread_ids: thread_ids.to_vec(),
            prompt: prompt.map(str::to_string),
            model: model.map(str::to_string),
            reasoning_effort,
            agents,
        }
    }
}

fn agent_from_state(thread_id: &str, state: Option<&CollabAgentState>) -> SubagentAgent {
    let (status, message) = match state {
        Some(state) => (
            match state.status {
                CollabAgentStatus::PendingInit => SubagentAgentStatus::Pending,
                CollabAgentStatus::Running => SubagentAgentStatus::Running,
                CollabAgentStatus::Interrupted => SubagentAgentStatus::Interrupted,
                CollabAgentStatus::Completed => SubagentAgentStatus::Completed,
                CollabAgentStatus::Errored => SubagentAgentStatus::Failed,
                CollabAgentStatus::Shutdown => SubagentAgentStatus::Shutdown,
                CollabAgentStatus::NotFound => SubagentAgentStatus::Missing,
            },
            state.message.clone(),
        ),
        None => (SubagentAgentStatus::Pending, None),
    };
    SubagentAgent {
        thread_id: thread_id.to_string(),
        status,
        message,
    }
}
