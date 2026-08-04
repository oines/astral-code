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
use crate::PendingInteractionError;
use crate::PendingInteractionUpdate;
use crate::PendingInteractions;
use crate::PlanImplementationRequest;
use crate::SessionError;
use crate::SessionState;
use crate::interactions::RequestObservation;
use crate::interactions::ResponseOwnership;
use crate::plan_implementation::PlanImplementationTracker;

/// Effect that one app-server notification had on the active transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptUpdate {
    Unchanged,
    Applied,
}

/// Lossless event boundary between the app-server session and future surface.
///
/// Server requests stay intact inside the typed interaction owner so modal
/// rendering never needs a second protocol response map. Queue-lag markers are
/// deliberately absent: they are internal backpressure diagnostics, not
/// conversation content or user-facing status.
#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    ServerNotification {
        notification: ServerNotification,
        transcript_update: TranscriptUpdate,
    },
    PendingInteraction(PendingInteractionUpdate),
    /// Requests that are not active-thread user interactions remain intact for
    /// the assembly layer (for example dynamic tools or another thread).
    ServerRequest(ServerRequest),
    Disconnected {
        message: String,
    },
}

#[derive(Debug)]
pub enum RuntimeError {
    Session(SessionError),
    Interaction(PendingInteractionError),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session(error) => write!(formatter, "{error}"),
            Self::Interaction(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Session(error) => Some(error),
            Self::Interaction(error) => Some(error),
        }
    }
}

impl From<SessionError> for RuntimeError {
    fn from(value: SessionError) -> Self {
        Self::Session(value)
    }
}

impl From<PendingInteractionError> for RuntimeError {
    fn from(value: PendingInteractionError) -> Self {
        Self::Interaction(value)
    }
}

/// Active Astral session plus its one canonical conversation projection.
///
/// This type consumes the app-server event stream exactly once. Dropped start
/// events are reconstructed by the transcript, and authoritative
/// completion events settle them. Pending request lifecycle is owned here;
/// modal rendering only reads that typed state and submits a response.
pub struct AstralRuntime {
    session: AstralSession,
    conversation: ConversationState,
    pending_interactions: PendingInteractions,
    plan_implementation: PlanImplementationTracker,
}

impl AstralRuntime {
    pub fn new(session: AstralSession) -> Result<Self, RuntimeError> {
        let thread = session
            .state()
            .map(|state| state.thread.clone())
            .ok_or(SessionError::NoThread)?;
        let thread_id = thread.id.clone();
        Ok(Self {
            session,
            conversation: ConversationState::from_thread(&thread),
            pending_interactions: PendingInteractions::new(thread_id),
            plan_implementation: PlanImplementationTracker::default(),
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

    pub fn pending_interactions(&self) -> &PendingInteractions {
        &self.pending_interactions
    }

    pub fn plan_implementation_request(&self) -> Option<&PlanImplementationRequest> {
        self.plan_implementation.request()
    }

    pub fn dismiss_plan_implementation(&mut self) {
        self.plan_implementation.clear();
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
        let response = self.session.start_turn(input).await?;
        self.plan_implementation.clear();
        Ok(response)
    }

    pub async fn interrupt(&mut self) -> Result<(), RuntimeError> {
        self.session.interrupt().await?;
        Ok(())
    }

    pub async fn resolve_server_request(
        &mut self,
        request_id: RequestId,
        result: JsonRpcResult,
    ) -> Result<(), RuntimeError> {
        let ownership = self.pending_interactions.begin_response(&request_id)?;
        match self
            .session
            .resolve_server_request(request_id.clone(), result)
            .await
        {
            Ok(()) => {
                if ownership == ResponseOwnership::Tracked {
                    self.pending_interactions.response_succeeded(&request_id);
                }
                Ok(())
            }
            Err(error) => {
                if ownership == ResponseOwnership::Tracked {
                    self.pending_interactions.response_failed(&request_id);
                }
                Err(error.into())
            }
        }
    }

    pub async fn reject_server_request(
        &mut self,
        request_id: RequestId,
        error: JSONRPCErrorError,
    ) -> Result<(), RuntimeError> {
        let ownership = self.pending_interactions.begin_response(&request_id)?;
        match self
            .session
            .reject_server_request(request_id.clone(), error)
            .await
        {
            Ok(()) => {
                if ownership == ResponseOwnership::Tracked {
                    self.pending_interactions.response_succeeded(&request_id);
                }
                Ok(())
            }
            Err(error) => {
                if ownership == ResponseOwnership::Tracked {
                    self.pending_interactions.response_failed(&request_id);
                }
                Err(error.into())
            }
        }
    }

    /// Wait for the next surface-relevant event.
    ///
    /// Best-effort lag markers are consumed here. Lossless deltas reconstruct
    /// dropped starts locally, and completed items remain authoritative.
    pub async fn next_event(&mut self) -> Option<RuntimeEvent> {
        loop {
            let event = self.session.next_event().await?;
            let thread_id = self
                .session
                .state()
                .map(|state| state.thread.id.as_str())
                .unwrap_or_default();
            self.plan_implementation.observe_event(thread_id, &event);
            if let Some(event) = apply_event(
                &mut self.conversation,
                &mut self.pending_interactions,
                event,
            ) {
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
    pending_interactions: &mut PendingInteractions,
    event: AppServerEvent,
) -> Option<RuntimeEvent> {
    match event {
        AppServerEvent::Lagged { .. } => None,
        AppServerEvent::ServerNotification(notification) => {
            if let Some(update) = pending_interactions.observe_notification(&notification) {
                return Some(RuntimeEvent::PendingInteraction(update));
            }
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
        AppServerEvent::ServerRequest(request) => {
            Some(match pending_interactions.observe_request(request) {
                RequestObservation::Updated(update) => RuntimeEvent::PendingInteraction(update),
                RequestObservation::PassThrough(request) => RuntimeEvent::ServerRequest(*request),
            })
        }
        AppServerEvent::Disconnected { message } => {
            pending_interactions.clear();
            Some(RuntimeEvent::Disconnected { message })
        }
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
