//! Typed ownership for active app-server user interactions.
//!
//! The app-server request remains authoritative. This module stores the full
//! request and derives only queue identity and presentation state, avoiding a
//! second copy of the protocol response mapping owned by app-server.

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
    key: InteractionKey,
    thread_id: String,
    turn_id: Option<String>,
    status: PendingInteractionStatus,
}

impl PendingInteraction {
    fn new(
        request: ServerRequest,
        kind: PendingInteractionKind,
        key: InteractionKey,
        thread_id: String,
        turn_id: Option<String>,
    ) -> Self {
        Self {
            request,
            kind,
            key,
            thread_id,
            turn_id,
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

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub fn turn_id(&self) -> Option<&str> {
        self.turn_id.as_deref()
    }
}

/// Observable queue mutation used by the future modal host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingInteractionUpdate {
    Added {
        request_id: RequestId,
    },
    Replayed {
        request_id: RequestId,
    },
    Replaced {
        previous_request_id: RequestId,
        request_id: RequestId,
    },
    Resolved {
        request_id: RequestId,
    },
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
/// Exact request replays are idempotent. A newly issued request for the same
/// logical approval/input replaces the old request in place so focus and queue
/// order remain stable while the stale JSON-RPC id can no longer be answered.
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
        let Some((kind, key)) = interaction_descriptor(&request) else {
            return RequestObservation::PassThrough(Box::new(request));
        };
        let Some((thread_id, turn_id)) = server_request_scope(&request) else {
            return RequestObservation::PassThrough(Box::new(request));
        };
        if thread_id != self.thread_id {
            return RequestObservation::PassThrough(Box::new(request));
        }
        let thread_id = thread_id.to_string();
        let turn_id = turn_id.map(str::to_string);

        if let Some(existing) = self
            .queue
            .iter_mut()
            .find(|pending| pending.request.id() == request.id())
        {
            let request_id = request.id().clone();
            if existing.request == request {
                return RequestObservation::Updated(PendingInteractionUpdate::Replayed {
                    request_id,
                });
            }
            existing.request = request;
            existing.kind = kind;
            existing.key = key;
            existing.thread_id = thread_id;
            existing.turn_id = turn_id;
            return RequestObservation::Updated(PendingInteractionUpdate::Replaced {
                previous_request_id: request_id.clone(),
                request_id,
            });
        }

        if let Some(existing) = self.queue.iter_mut().find(|pending| pending.key == key) {
            let previous_request_id = existing.request.id().clone();
            let request_id = request.id().clone();
            *existing = PendingInteraction::new(request, kind, key, thread_id, turn_id);
            return RequestObservation::Updated(PendingInteractionUpdate::Replaced {
                previous_request_id,
                request_id,
            });
        }

        let request_id = request.id().clone();
        self.queue.push_back(PendingInteraction::new(
            request, kind, key, thread_id, turn_id,
        ));
        RequestObservation::Updated(PendingInteractionUpdate::Added { request_id })
    }

    pub(crate) fn resolve_notification(
        &mut self,
        thread_id: &str,
        request_id: &RequestId,
    ) -> Option<PendingInteractionUpdate> {
        if thread_id != self.thread_id {
            return None;
        }
        self.remove(request_id)
    }

    /// Mark a response in flight. Returns false for requests this thread does
    /// not own, allowing non-interactive requests to use the same transport.
    pub(crate) fn begin_response(
        &mut self,
        request_id: &RequestId,
    ) -> Result<bool, PendingInteractionError> {
        let Some(pending) = self
            .queue
            .iter_mut()
            .find(|pending| pending.request.id() == request_id)
        else {
            return Ok(false);
        };
        match pending.status {
            PendingInteractionStatus::Waiting => {
                pending.status = PendingInteractionStatus::Responding;
                Ok(true)
            }
            PendingInteractionStatus::Responding => Err(
                PendingInteractionError::AlreadyResponding(request_id.clone()),
            ),
        }
    }

    pub(crate) fn response_succeeded(
        &mut self,
        request_id: &RequestId,
    ) -> Option<PendingInteractionUpdate> {
        self.remove(request_id)
    }

    pub(crate) fn response_failed(&mut self, request_id: &RequestId) -> bool {
        let Some(pending) = self
            .queue
            .iter_mut()
            .find(|pending| pending.request.id() == request_id)
        else {
            return false;
        };
        pending.status = PendingInteractionStatus::Waiting;
        true
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RequestObservation {
    Updated(PendingInteractionUpdate),
    PassThrough(Box<ServerRequest>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InteractionKey {
    CommandExecution {
        turn_id: String,
        callback_id: String,
    },
    FileChange {
        turn_id: String,
        item_id: String,
    },
    UserInput {
        turn_id: String,
        item_id: String,
    },
    McpElicitation {
        server_name: String,
        request_id: RequestId,
    },
    Permissions {
        turn_id: String,
        item_id: String,
    },
}

fn interaction_descriptor(
    request: &ServerRequest,
) -> Option<(PendingInteractionKind, InteractionKey)> {
    match request {
        ServerRequest::CommandExecutionRequestApproval {
            request_id: _,
            params,
        } => Some((
            PendingInteractionKind::CommandExecutionApproval,
            InteractionKey::CommandExecution {
                turn_id: params.turn_id.clone(),
                callback_id: params
                    .approval_id
                    .clone()
                    .unwrap_or_else(|| params.item_id.clone()),
            },
        )),
        ServerRequest::FileChangeRequestApproval {
            request_id: _,
            params,
        } => Some((
            PendingInteractionKind::FileChangeApproval,
            InteractionKey::FileChange {
                turn_id: params.turn_id.clone(),
                item_id: params.item_id.clone(),
            },
        )),
        ServerRequest::ToolRequestUserInput {
            request_id: _,
            params,
        } => Some((
            PendingInteractionKind::UserInput,
            InteractionKey::UserInput {
                turn_id: params.turn_id.clone(),
                item_id: params.item_id.clone(),
            },
        )),
        ServerRequest::McpServerElicitationRequest { request_id, params } => Some((
            PendingInteractionKind::McpElicitation,
            InteractionKey::McpElicitation {
                server_name: params.server_name.clone(),
                request_id: request_id.clone(),
            },
        )),
        ServerRequest::PermissionsRequestApproval {
            request_id: _,
            params,
        } => Some((
            PendingInteractionKind::PermissionsApproval,
            InteractionKey::Permissions {
                turn_id: params.turn_id.clone(),
                item_id: params.item_id.clone(),
            },
        )),
        ServerRequest::DynamicToolCall { .. }
        | ServerRequest::AttestationGenerate { .. }
        | ServerRequest::ApplyPatchApproval { .. }
        | ServerRequest::ExecCommandApproval { .. } => None,
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
