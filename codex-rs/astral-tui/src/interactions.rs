//! Typed ownership for active app-server user interactions.
//!
//! The complete app-server request remains the only semantic source. This
//! module adds ordering and presentation lifecycle, but deliberately does not
//! translate responses or invent identity beyond the JSON-RPC request id.

use std::collections::VecDeque;

use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;

/// User-facing interaction represented by an app-server reverse request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingInteractionKind {
    CommandExecutionApproval,
    FileChangeApproval,
    UserInput,
    McpElicitation,
    PermissionsApproval,
}

/// Whether an interaction is waiting for the user or for app-server to accept
/// a response already sent by this client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingInteractionStatus {
    Waiting,
    Responding,
}

/// One authoritative request plus TUI-only presentation lifecycle.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingInteraction {
    request: ServerRequest,
    kind: PendingInteractionKind,
    status: PendingInteractionStatus,
}

impl PendingInteraction {
    fn new(request: ServerRequest, kind: PendingInteractionKind) -> Self {
        Self {
            request,
            kind,
            status: PendingInteractionStatus::Waiting,
        }
    }

    pub fn request(&self) -> &ServerRequest {
        &self.request
    }

    pub fn request_id(&self) -> &RequestId {
        self.request.id()
    }

    pub fn kind(&self) -> PendingInteractionKind {
        self.kind
    }

    pub fn status(&self) -> PendingInteractionStatus {
        self.status
    }

    pub fn thread_id(&self) -> Option<&str> {
        server_request_scope(&self.request).map(|(thread_id, _)| thread_id)
    }

    pub fn turn_id(&self) -> Option<&str> {
        server_request_scope(&self.request).and_then(|(_, turn_id)| turn_id)
    }
}

/// Observable queue mutation used by the future modal host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingInteractionUpdate {
    Added { request_id: RequestId },
    Refreshed { request_id: RequestId },
    Resolved { request_id: RequestId },
}

/// Refuses a second local answer while the first is still in flight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingInteractionError {
    AlreadyResponding(RequestId),
}

impl std::fmt::Display for PendingInteractionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyResponding(request_id) => {
                write!(
                    formatter,
                    "app-server request {request_id} is already being answered"
                )
            }
        }
    }
}

impl std::error::Error for PendingInteractionError {}

/// Ordered pending interactions for one active Astral thread.
///
/// A replay with the same request id refreshes the existing queue slot. A new
/// request id always creates a distinct pending interaction, even when its
/// tool/item metadata matches another request: app-server still owns both
/// waiters and both must be resolved independently.
#[derive(Debug)]
pub struct PendingInteractions {
    thread_id: String,
    queue: VecDeque<PendingInteraction>,
}

impl PendingInteractions {
    pub fn new(thread_id: impl Into<String>) -> Self {
        Self {
            thread_id: thread_id.into(),
            queue: VecDeque::new(),
        }
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub fn active(&self) -> Option<&PendingInteraction> {
        self.queue.front()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &PendingInteraction> {
        self.queue.iter()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn clear(&mut self) {
        self.queue.clear();
    }

    pub(crate) fn observe_request(&mut self, request: ServerRequest) -> RequestObservation {
        let Some(kind) = interaction_kind(&request) else {
            return RequestObservation::PassThrough(Box::new(request));
        };
        let Some((thread_id, _)) = server_request_scope(&request) else {
            return RequestObservation::PassThrough(Box::new(request));
        };
        if thread_id != self.thread_id {
            return RequestObservation::PassThrough(Box::new(request));
        }

        if let Some(existing) = self
            .queue
            .iter_mut()
            .find(|pending| pending.request.id() == request.id())
        {
            let request_id = request.id().clone();
            let status = existing.status;
            *existing = PendingInteraction {
                request,
                kind,
                status,
            };
            return RequestObservation::Updated(PendingInteractionUpdate::Refreshed { request_id });
        }

        let request_id = request.id().clone();
        self.queue.push_back(PendingInteraction::new(request, kind));
        RequestObservation::Updated(PendingInteractionUpdate::Added { request_id })
    }

    pub(crate) fn observe_notification(
        &mut self,
        notification: &codex_app_server_protocol::ServerNotification,
    ) -> Option<PendingInteractionUpdate> {
        use codex_app_server_protocol::ServerNotification;

        match notification {
            ServerNotification::ServerRequestResolved(resolved)
                if resolved.thread_id == self.thread_id =>
            {
                self.remove(&resolved.request_id)
            }
            ServerNotification::ItemStarted(started) if started.thread_id == self.thread_id => {
                if let Some(item_id) = approval_item_id(&started.item) {
                    self.queue.retain(|pending| {
                        !pending.is_approval_for_started_item(&started.turn_id, item_id)
                    });
                }
                None
            }
            ServerNotification::TurnCompleted(completed)
                if completed.thread_id == self.thread_id =>
            {
                self.queue
                    .retain(|pending| pending.turn_id() != Some(completed.turn.id.as_str()));
                None
            }
            ServerNotification::ThreadClosed(closed) if closed.thread_id == self.thread_id => {
                self.clear();
                None
            }
            _ => None,
        }
    }

    /// Mark a response in flight while allowing untracked requests to keep
    /// using the same transport.
    pub(crate) fn begin_response(
        &mut self,
        request_id: &RequestId,
    ) -> Result<ResponseOwnership, PendingInteractionError> {
        let Some(pending) = self
            .queue
            .iter_mut()
            .find(|pending| pending.request.id() == request_id)
        else {
            return Ok(ResponseOwnership::Untracked);
        };
        match pending.status {
            PendingInteractionStatus::Waiting => {
                pending.status = PendingInteractionStatus::Responding;
                Ok(ResponseOwnership::Tracked)
            }
            PendingInteractionStatus::Responding => Err(
                PendingInteractionError::AlreadyResponding(request_id.clone()),
            ),
        }
    }

    pub(crate) fn response_succeeded(&mut self, request_id: &RequestId) {
        self.remove(request_id);
    }

    pub(crate) fn response_failed(&mut self, request_id: &RequestId) {
        let Some(pending) = self
            .queue
            .iter_mut()
            .find(|pending| pending.request.id() == request_id)
        else {
            return;
        };
        pending.status = PendingInteractionStatus::Waiting;
    }

    fn remove(&mut self, request_id: &RequestId) -> Option<PendingInteractionUpdate> {
        let index = self
            .queue
            .iter()
            .position(|pending| pending.request.id() == request_id)?;
        let pending = self.queue.remove(index)?;
        Some(PendingInteractionUpdate::Resolved {
            request_id: pending.request.id().clone(),
        })
    }
}

impl PendingInteraction {
    fn is_approval_for_started_item(&self, turn_id: &str, item_id: &str) -> bool {
        match &self.request {
            ServerRequest::CommandExecutionRequestApproval { params, .. } => {
                params.turn_id == turn_id && params.item_id == item_id
            }
            ServerRequest::FileChangeRequestApproval { params, .. } => {
                params.turn_id == turn_id && params.item_id == item_id
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RequestObservation {
    Updated(PendingInteractionUpdate),
    PassThrough(Box<ServerRequest>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResponseOwnership {
    Tracked,
    Untracked,
}

fn interaction_kind(request: &ServerRequest) -> Option<PendingInteractionKind> {
    match request {
        ServerRequest::CommandExecutionRequestApproval { .. } => {
            Some(PendingInteractionKind::CommandExecutionApproval)
        }
        ServerRequest::FileChangeRequestApproval { .. } => {
            Some(PendingInteractionKind::FileChangeApproval)
        }
        ServerRequest::ToolRequestUserInput { .. } => Some(PendingInteractionKind::UserInput),
        ServerRequest::McpServerElicitationRequest { .. } => {
            Some(PendingInteractionKind::McpElicitation)
        }
        ServerRequest::PermissionsRequestApproval { .. } => {
            Some(PendingInteractionKind::PermissionsApproval)
        }
        ServerRequest::DynamicToolCall { .. }
        | ServerRequest::AttestationGenerate { .. }
        | ServerRequest::ApplyPatchApproval { .. }
        | ServerRequest::ExecCommandApproval { .. } => None,
    }
}

fn approval_item_id(item: &codex_app_server_protocol::ThreadItem) -> Option<&str> {
    use codex_app_server_protocol::ThreadItem;

    match item {
        ThreadItem::CommandExecution { id, .. } | ThreadItem::FileChange { id, .. } => Some(id),
        _ => None,
    }
}

fn server_request_scope(request: &ServerRequest) -> Option<(&str, Option<&str>)> {
    match request {
        ServerRequest::CommandExecutionRequestApproval { params, .. } => {
            Some((&params.thread_id, Some(&params.turn_id)))
        }
        ServerRequest::FileChangeRequestApproval { params, .. } => {
            Some((&params.thread_id, Some(&params.turn_id)))
        }
        ServerRequest::ToolRequestUserInput { params, .. } => {
            Some((&params.thread_id, Some(&params.turn_id)))
        }
        ServerRequest::McpServerElicitationRequest { params, .. } => {
            Some((&params.thread_id, params.turn_id.as_deref()))
        }
        ServerRequest::PermissionsRequestApproval { params, .. } => {
            Some((&params.thread_id, Some(&params.turn_id)))
        }
        ServerRequest::DynamicToolCall { params, .. } => {
            Some((&params.thread_id, Some(&params.turn_id)))
        }
        ServerRequest::AttestationGenerate { .. }
        | ServerRequest::ApplyPatchApproval { .. }
        | ServerRequest::ExecCommandApproval { .. } => None,
    }
}

#[cfg(test)]
#[path = "interactions_tests.rs"]
mod tests;
