//! Prompt-area ownership for blocking app-server interactions.
//!
//! The queue and full protocol requests remain owned by [`PendingInteractions`].
//! This host presents only the front request and returns one typed JSON-RPC
//! response for the assembly layer to send through [`AstralRuntime`].

use std::time::Instant;

use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::Result as JsonRpcResult;
use codex_app_server_protocol::ServerRequest;
use crossterm::event::KeyEvent;
use crossterm::event::MouseEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::PendingInteractionStatus;
use crate::PendingInteractions;

mod approval;
mod ask_user;
mod choice_list;
mod mcp_action;
mod mcp_form;
mod mcp_url;

use approval::ApprovalPrompt;
use ask_user::AskUserPrompt;
use mcp_action::McpActionPrompt;
use mcp_form::McpFormPrompt;
use mcp_url::McpUrlPrompt;

/// One response ready for [`AstralRuntime::resolve_server_request`].
#[derive(Debug, Clone, PartialEq)]
pub struct PromptInteractionSubmission {
    pub request_id: RequestId,
    pub result: JsonRpcResult,
}

/// Result of routing input through the active prompt-area interaction.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PromptInteractionOutcome {
    Unchanged,
    Changed,
    OpenExternalUrl { url: String },
    Submit(PromptInteractionSubmission),
    Failed(String),
}

enum PromptPresenter {
    Approval(ApprovalPrompt),
    AskUser(AskUserPrompt),
    McpAction(McpActionPrompt),
    McpForm(McpFormPrompt),
    McpUrl(McpUrlPrompt),
}

/// Retained presenter for the front item in [`PendingInteractions`].
///
/// A different request id replaces presentation state. An exact replay is a
/// no-op; a changed replay with the same id refreshes its content in place.
pub struct PromptInteractionHost {
    source: Option<ServerRequest>,
    status: Option<PendingInteractionStatus>,
    queue_len: usize,
    presenter: Option<PromptPresenter>,
}

impl Default for PromptInteractionHost {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptInteractionHost {
    pub fn new() -> Self {
        Self {
            source: None,
            status: None,
            queue_len: 0,
            presenter: None,
        }
    }

    pub fn is_presentable(&self) -> bool {
        self.presenter.is_some()
    }

    pub fn queue_len(&self) -> usize {
        self.queue_len
    }

    /// Content-driven prompt height, capped by the host's available rows.
    pub fn desired_height(&self, width: u16, available: u16) -> u16 {
        self.presenter
            .as_ref()
            .map_or(0, |presenter| presenter.desired_height(width, available))
    }

    /// Synchronize from the queue without consuming or reordering it.
    pub fn sync(&mut self, pending: &PendingInteractions) -> bool {
        let active = pending.active();
        let source = active.map(|interaction| interaction.request().clone());
        let status = active.map(super::interactions::PendingInteraction::status);
        let queue_len = pending.len();
        let source_changed = self.source != source;
        if !source_changed && self.status == status && self.queue_len == queue_len {
            return false;
        }

        if source_changed {
            let same_request = self
                .source
                .as_ref()
                .zip(source.as_ref())
                .is_some_and(|(previous, current)| previous.id() == current.id());
            let previous_selection = same_request
                .then(|| self.presenter.as_ref().map(PromptPresenter::selected_index))
                .flatten();
            self.presenter = source.as_ref().and_then(PromptPresenter::from_request);
            if let Some(selected) = previous_selection
                && let Some(presenter) = self.presenter.as_mut()
            {
                presenter.set_selected_index(selected);
            }
        }
        self.source = source;
        self.status = status;
        self.queue_len = queue_len;
        true
    }

    pub fn render(&mut self, buffer: &mut Buffer, area: Rect) {
        let responding = self.status == Some(PendingInteractionStatus::Responding);
        if let Some(presenter) = self.presenter.as_mut() {
            presenter.render(buffer, area, self.queue_len, responding);
        }
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) -> PromptInteractionOutcome {
        if self.status == Some(PendingInteractionStatus::Responding) {
            return PromptInteractionOutcome::Unchanged;
        }
        self.presenter
            .as_mut()
            .map_or(PromptInteractionOutcome::Unchanged, |presenter| {
                presenter.handle_key_event(key)
            })
    }

    pub fn handle_mouse_event_at(
        &mut self,
        mouse: MouseEvent,
        now: Instant,
    ) -> PromptInteractionOutcome {
        if self.status == Some(PendingInteractionStatus::Responding) {
            return PromptInteractionOutcome::Unchanged;
        }
        self.presenter
            .as_mut()
            .map_or(PromptInteractionOutcome::Unchanged, |presenter| {
                presenter.handle_mouse_event_at(mouse, now)
            })
    }

    pub fn handle_paste(&mut self, text: &str) -> PromptInteractionOutcome {
        if self.status == Some(PendingInteractionStatus::Responding) {
            return PromptInteractionOutcome::Unchanged;
        }
        self.presenter
            .as_mut()
            .map_or(PromptInteractionOutcome::Unchanged, |presenter| {
                presenter.handle_paste(text)
            })
    }
}

impl PromptPresenter {
    fn from_request(request: &ServerRequest) -> Option<Self> {
        ApprovalPrompt::from_request(request)
            .map(Self::Approval)
            .or_else(|| AskUserPrompt::from_request(request).map(Self::AskUser))
            .or_else(|| McpActionPrompt::from_request(request).map(Self::McpAction))
            .or_else(|| McpFormPrompt::from_request(request).map(Self::McpForm))
            .or_else(|| McpUrlPrompt::from_request(request).map(Self::McpUrl))
    }

    fn selected_index(&self) -> usize {
        match self {
            Self::Approval(prompt) => prompt.selected_index(),
            Self::AskUser(_) => 0,
            Self::McpAction(prompt) => prompt.selected_index(),
            Self::McpForm(prompt) => prompt.selected_index(),
            Self::McpUrl(prompt) => prompt.selected_index(),
        }
    }

    fn set_selected_index(&mut self, selected: usize) {
        match self {
            Self::Approval(prompt) => prompt.set_selected_index(selected),
            Self::AskUser(_) => {}
            Self::McpAction(prompt) => prompt.set_selected_index(selected),
            Self::McpForm(prompt) => prompt.set_selected_index(selected),
            Self::McpUrl(prompt) => prompt.set_selected_index(selected),
        }
    }

    fn desired_height(&self, width: u16, available: u16) -> u16 {
        match self {
            Self::Approval(prompt) => prompt.desired_height(width, available),
            Self::AskUser(prompt) => prompt.desired_height(width, available),
            Self::McpAction(prompt) => prompt.desired_height(width, available),
            Self::McpForm(prompt) => prompt.desired_height(width, available),
            Self::McpUrl(prompt) => prompt.desired_height(width, available),
        }
    }

    fn render(&mut self, buffer: &mut Buffer, area: Rect, queue_len: usize, responding: bool) {
        match self {
            Self::Approval(prompt) => prompt.render(buffer, area, queue_len, responding),
            Self::AskUser(prompt) => prompt.render(buffer, area, queue_len, responding),
            Self::McpAction(prompt) => prompt.render(buffer, area, queue_len, responding),
            Self::McpForm(prompt) => prompt.render(buffer, area, queue_len, responding),
            Self::McpUrl(prompt) => prompt.render(buffer, area, queue_len, responding),
        }
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> PromptInteractionOutcome {
        match self {
            Self::Approval(prompt) => prompt.handle_key_event(key),
            Self::AskUser(prompt) => prompt.handle_key_event(key),
            Self::McpAction(prompt) => prompt.handle_key_event(key),
            Self::McpForm(prompt) => prompt.handle_key_event(key),
            Self::McpUrl(prompt) => prompt.handle_key_event(key),
        }
    }

    fn handle_mouse_event_at(
        &mut self,
        mouse: MouseEvent,
        now: Instant,
    ) -> PromptInteractionOutcome {
        match self {
            Self::Approval(prompt) => prompt.handle_mouse_event_at(mouse, now),
            Self::AskUser(prompt) => prompt.handle_mouse_event_at(mouse, now),
            Self::McpAction(prompt) => prompt.handle_mouse_event_at(mouse, now),
            Self::McpForm(prompt) => prompt.handle_mouse_event_at(mouse, now),
            Self::McpUrl(prompt) => prompt.handle_mouse_event_at(mouse, now),
        }
    }

    fn handle_paste(&mut self, text: &str) -> PromptInteractionOutcome {
        match self {
            Self::Approval(_) => PromptInteractionOutcome::Unchanged,
            Self::AskUser(prompt) => prompt.handle_paste(text),
            Self::McpAction(_) => PromptInteractionOutcome::Unchanged,
            Self::McpForm(prompt) => prompt.handle_paste(text),
            Self::McpUrl(_) => PromptInteractionOutcome::Unchanged,
        }
    }
}

#[cfg(test)]
#[path = "prompt_interaction_tests.rs"]
mod tests;
