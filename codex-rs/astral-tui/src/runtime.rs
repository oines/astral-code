use astral_tui_scrollback::ApplyOutcome;
use astral_tui_scrollback::EntryRenderOptions;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::Result as JsonRpcResult;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput;

use crate::AstralSession;
use crate::ConversationState;
use crate::ConversationSurface;
use crate::SessionError;
use crate::SessionState;

/// Effect that one app-server notification had on the active transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptUpdate {
    Unchanged,
    Applied,
}

/// Lossless event boundary between the app-server session and future surface.
///
/// Server requests stay intact so the modal controller remains their only
/// owner. Queue-lag markers are deliberately absent: they are internal
/// backpressure diagnostics, not conversation content or user-facing status.
#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    ServerNotification {
        notification: ServerNotification,
        transcript_update: TranscriptUpdate,
    },
    ServerRequest(ServerRequest),
    Disconnected {
        message: String,
    },
}

#[derive(Debug)]
pub enum RuntimeError {
    Session(SessionError),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Session(error) => Some(error),
        }
    }
}

impl From<SessionError> for RuntimeError {
    fn from(value: SessionError) -> Self {
        Self::Session(value)
    }
}

/// Active Astral session plus its one canonical conversation projection.
///
/// This type consumes the app-server event stream exactly once. Dropped start
/// events are reconstructed by the transcript, and authoritative
/// completion events settle them. Request/response UI state remains owned by
/// the modal layer.
pub struct AstralRuntime {
    session: AstralSession,
    conversation: ConversationState,
}

impl AstralRuntime {
    pub fn new(session: AstralSession) -> Result<Self, RuntimeError> {
        let thread = session
            .state()
            .map(|state| state.thread.clone())
            .ok_or(SessionError::NoThread)?;
        Ok(Self {
            session,
            conversation: ConversationState::from_thread(&thread),
        })
    }

    pub fn session_state(&self) -> Option<&SessionState> {
        self.session.state()
    }

    pub fn conversation(&self) -> &ConversationState {
        &self.conversation
    }

    pub fn conversation_mut(&mut self) -> &mut ConversationState {
        &mut self.conversation
    }

    /// Materialize the one canonical rendered tree consumed by both terminal
    /// viewport modes. Viewport and commit policy remain host concerns.
    pub fn render_surface(&self, options: EntryRenderOptions) -> ConversationSurface {
        ConversationSurface::render(&self.conversation, options)
    }

    pub async fn start_turn(
        &mut self,
        input: Vec<UserInput>,
    ) -> Result<TurnStartResponse, RuntimeError> {
        Ok(self.session.start_turn(input).await?)
    }

    pub async fn interrupt(&mut self) -> Result<(), RuntimeError> {
        self.session.interrupt().await?;
        Ok(())
    }

    pub async fn resolve_server_request(
        &self,
        request_id: RequestId,
        result: JsonRpcResult,
    ) -> Result<(), RuntimeError> {
        self.session
            .resolve_server_request(request_id, result)
            .await?;
        Ok(())
    }

    pub async fn reject_server_request(
        &self,
        request_id: RequestId,
        error: JSONRPCErrorError,
    ) -> Result<(), RuntimeError> {
        self.session
            .reject_server_request(request_id, error)
            .await?;
        Ok(())
    }

    /// Wait for the next surface-relevant event.
    ///
    /// Best-effort lag markers are consumed here. Lossless deltas reconstruct
    /// dropped starts locally, and completed items remain authoritative.
    pub async fn next_event(&mut self) -> Option<RuntimeEvent> {
        loop {
            let event = self.session.next_event().await?;
            if let Some(event) = apply_event(&mut self.conversation, event) {
                return Some(event);
            }
        }
    }

    pub async fn shutdown(self) -> Result<(), RuntimeError> {
        self.session.shutdown().await?;
        Ok(())
    }
}

fn apply_event(
    conversation: &mut ConversationState,
    event: AppServerEvent,
) -> Option<RuntimeEvent> {
    match event {
        AppServerEvent::Lagged { .. } => None,
        AppServerEvent::ServerNotification(notification) => {
            match conversation.apply(&notification) {
                ApplyOutcome::Applied => Some(RuntimeEvent::ServerNotification {
                    notification,
                    transcript_update: TranscriptUpdate::Applied,
                }),
                ApplyOutcome::Ignored(_)
                | ApplyOutcome::DifferentThread
                | ApplyOutcome::NotTranscript => Some(RuntimeEvent::ServerNotification {
                    notification,
                    transcript_update: TranscriptUpdate::Unchanged,
                }),
            }
        }
        AppServerEvent::ServerRequest(request) => Some(RuntimeEvent::ServerRequest(request)),
        AppServerEvent::Disconnected { message } => Some(RuntimeEvent::Disconnected { message }),
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
