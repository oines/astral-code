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

use approval::ApprovalPrompt;

/// One response ready for [`AstralRuntime::resolve_server_request`].
#[derive(Debug, Clone, PartialEq)]
pub struct PromptInteractionSubmission {
    pub request_id: RequestId,
    pub result: JsonRpcResult,
}

/// Result of routing input through the active prompt-area interaction.
#[derive(Debug, Clone, PartialEq)]
pub enum PromptInteractionOutcome {
    Unchanged,
    Changed,
    Submit(PromptInteractionSubmission),
    Failed(String),
}

enum PromptPresenter {
    Approval(ApprovalPrompt),
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
            self.presenter = source
                .as_ref()
                .and_then(ApprovalPrompt::from_request)
                .map(PromptPresenter::Approval);
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
}

impl PromptPresenter {
    fn desired_height(&self, width: u16, available: u16) -> u16 {
        match self {
            Self::Approval(prompt) => prompt.desired_height(width, available),
        }
    }

    fn selected_index(&self) -> usize {
        match self {
            Self::Approval(prompt) => prompt.selected_index(),
        }
    }

    fn set_selected_index(&mut self, selected: usize) {
        match self {
            Self::Approval(prompt) => prompt.set_selected_index(selected),
        }
    }

    fn render(&mut self, buffer: &mut Buffer, area: Rect, queue_len: usize, responding: bool) {
        match self {
            Self::Approval(prompt) => prompt.render(buffer, area, queue_len, responding),
        }
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> PromptInteractionOutcome {
        match self {
            Self::Approval(prompt) => prompt.handle_key_event(key),
        }
    }

    fn handle_mouse_event_at(
        &mut self,
        mouse: MouseEvent,
        now: Instant,
    ) -> PromptInteractionOutcome {
        match self {
            Self::Approval(prompt) => prompt.handle_mouse_event_at(mouse, now),
        }
    }
}

#[cfg(test)]
#[path = "prompt_interaction_tests.rs"]
mod tests;
