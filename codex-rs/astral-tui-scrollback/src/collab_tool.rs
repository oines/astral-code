//! Typed presentation view for app-server collaboration tool calls.

use std::collections::HashMap;

use codex_app_server_protocol::CollabAgentState;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::ThreadItem;

/// Exact view of one collaboration operation. A spawn call is not treated as
/// the child agent's lifecycle: completing the call only means the child was
/// created, and the child may still be running.
#[derive(Debug, Clone, PartialEq)]
pub struct CollabAgentToolCallBlock<'a> {
    tool: &'a CollabAgentTool,
    status: &'a CollabAgentToolCallStatus,
    receiver_thread_ids: &'a [String],
    prompt: Option<&'a str>,
    model: Option<&'a str>,
    reasoning_effort: Option<String>,
    agents_states: &'a HashMap<String, CollabAgentState>,
}

impl<'a> CollabAgentToolCallBlock<'a> {
    pub(crate) fn from_item(item: &'a ThreadItem) -> Option<Self> {
        let ThreadItem::CollabAgentToolCall {
            tool,
            status,
            receiver_thread_ids,
            prompt,
            model,
            reasoning_effort,
            agents_states,
            ..
        } = item
        else {
            return None;
        };
        Some(Self {
            tool,
            status,
            receiver_thread_ids,
            prompt: prompt.as_deref(),
            model: model.as_deref(),
            reasoning_effort: reasoning_effort.as_ref().map(ToString::to_string),
            agents_states,
        })
    }

    pub fn tool(&self) -> &CollabAgentTool {
        self.tool
    }

    pub fn receiver_thread_ids(&self) -> &'a [String] {
        self.receiver_thread_ids
    }

    pub fn prompt(&self) -> Option<&'a str> {
        self.prompt
    }

    pub fn model(&self) -> Option<&'a str> {
        self.model
    }

    pub fn reasoning_effort(&self) -> Option<&str> {
        self.reasoning_effort.as_deref()
    }

    pub fn agents_states(&self) -> &'a HashMap<String, CollabAgentState> {
        self.agents_states
    }

    pub fn running(&self) -> bool {
        matches!(self.status, CollabAgentToolCallStatus::InProgress)
    }

    pub fn failed(&self) -> bool {
        matches!(self.status, CollabAgentToolCallStatus::Failed)
    }

    pub fn has_details(&self) -> bool {
        self.prompt.is_some_and(|prompt| !prompt.trim().is_empty())
            || !self.agents_states.is_empty()
            || self.receiver_thread_ids.len() > 1
    }
}
