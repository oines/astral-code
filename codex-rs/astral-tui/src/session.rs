use std::io;

use codex_app_server_client::AppServerClient;
use codex_app_server_client::AppServerEvent;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadForkResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnInterruptParams;
use codex_app_server_protocol::TurnInterruptResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::openai_models::ReasoningEffort;

use crate::RequestResolution;

#[derive(Debug)]
pub enum SessionError {
    NoThread,
    NoActiveTurn,
    Request(TypedRequestError),
    Transport(io::Error),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoThread => f.write_str("no Astral thread is active"),
            Self::NoActiveTurn => f.write_str("no Astral turn is active"),
            Self::Request(error) => write!(f, "{error}"),
            Self::Transport(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Request(error) => Some(error),
            Self::Transport(error) => Some(error),
            Self::NoThread | Self::NoActiveTurn => None,
        }
    }
}

impl From<TypedRequestError> for SessionError {
    fn from(value: TypedRequestError) -> Self {
        Self::Request(value)
    }
}

impl From<io::Error> for SessionError {
    fn from(value: io::Error) -> Self {
        Self::Transport(value)
    }
}

/// Runtime metadata needed by the Astral surface.
///
/// The authoritative transcript remains in app-server notifications; this
/// state only tracks lifecycle identifiers and footer metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionState {
    pub thread: Thread,
    pub model: String,
    pub model_provider: String,
    pub service_tier: Option<String>,
    pub active_turn_id: Option<String>,
    pub collaboration_mode: CollaborationMode,
}

impl SessionState {
    fn from_start(response: ThreadStartResponse) -> Self {
        let collaboration_mode =
            default_collaboration_mode(response.model.clone(), response.reasoning_effort);
        Self {
            thread: response.thread,
            model: response.model,
            model_provider: response.model_provider,
            service_tier: response.service_tier,
            active_turn_id: None,
            collaboration_mode,
        }
    }

    fn from_resume(response: ThreadResumeResponse) -> Self {
        let collaboration_mode =
            default_collaboration_mode(response.model.clone(), response.reasoning_effort);
        let active_turn_id = response
            .thread
            .turns
            .iter()
            .rev()
            .find(|turn| turn.status == codex_app_server_protocol::TurnStatus::InProgress)
            .map(|turn| turn.id.clone());
        Self {
            thread: response.thread,
            model: response.model,
            model_provider: response.model_provider,
            service_tier: response.service_tier,
            active_turn_id,
            collaboration_mode,
        }
    }

    fn from_fork(response: ThreadForkResponse) -> Self {
        let collaboration_mode =
            default_collaboration_mode(response.model.clone(), response.reasoning_effort);
        Self {
            thread: response.thread,
            model: response.model,
            model_provider: response.model_provider,
            service_tier: response.service_tier,
            active_turn_id: None,
            collaboration_mode,
        }
    }

    fn observe_notification(&mut self, notification: &ServerNotification) {
        match notification {
            ServerNotification::TurnStarted(params) if params.thread_id == self.thread.id => {
                self.active_turn_id = Some(params.turn.id.clone());
            }
            ServerNotification::TurnCompleted(params) if params.thread_id == self.thread.id => {
                if self.active_turn_id.as_deref() == Some(&params.turn.id) {
                    self.active_turn_id = None;
                }
            }
            ServerNotification::ThreadClosed(params) if params.thread_id == self.thread.id => {
                self.active_turn_id = None;
            }
            ServerNotification::ThreadNameUpdated(params) if params.thread_id == self.thread.id => {
                self.thread.name.clone_from(&params.thread_name);
            }
            ServerNotification::ThreadSettingsUpdated(params)
                if params.thread_id == self.thread.id =>
            {
                self.model.clone_from(&params.thread_settings.model);
                self.model_provider
                    .clone_from(&params.thread_settings.model_provider);
                self.service_tier
                    .clone_from(&params.thread_settings.service_tier);
                self.collaboration_mode = default_collaboration_mode(
                    params.thread_settings.model.clone(),
                    params.thread_settings.effort.clone(),
                );
            }
            _ => {}
        }
    }
}

/// Thin app-server v2 session used by the Astral surface.
///
/// It deliberately owns no model runtime or tool adapter. New and remote
/// transports share the same typed requests and event stream.
pub struct AstralSession {
    client: AppServerClient,
    next_request_id: i64,
    state: Option<SessionState>,
}

impl AstralSession {
    pub fn new(client: AppServerClient) -> Self {
        Self {
            client,
            next_request_id: 1,
            state: None,
        }
    }

    pub fn state(&self) -> Option<&SessionState> {
        self.state.as_ref()
    }

    pub async fn start(
        &mut self,
        params: ThreadStartParams,
    ) -> Result<&SessionState, SessionError> {
        let request_id = self.next_request_id();
        let response: ThreadStartResponse = self
            .client
            .request_typed(ClientRequest::ThreadStart { request_id, params })
            .await?;
        self.state = Some(SessionState::from_start(response));
        self.state.as_ref().ok_or(SessionError::NoThread)
    }

    pub async fn resume(
        &mut self,
        params: ThreadResumeParams,
    ) -> Result<&SessionState, SessionError> {
        let request_id = self.next_request_id();
        let response = self
            .client
            .request_typed(ClientRequest::ThreadResume { request_id, params })
            .await?;
        self.state = Some(SessionState::from_resume(response));
        self.state.as_ref().ok_or(SessionError::NoThread)
    }

    pub async fn fork(&mut self, params: ThreadForkParams) -> Result<&SessionState, SessionError> {
        let request_id = self.next_request_id();
        let response = self
            .client
            .request_typed(ClientRequest::ThreadFork { request_id, params })
            .await?;
        self.state = Some(SessionState::from_fork(response));
        self.state.as_ref().ok_or(SessionError::NoThread)
    }

    pub async fn start_turn(
        &mut self,
        input: Vec<UserInput>,
    ) -> Result<TurnStartResponse, SessionError> {
        let thread_id = self
            .state
            .as_ref()
            .map(|state| state.thread.id.clone())
            .ok_or(SessionError::NoThread)?;
        let collaboration_mode = self
            .state
            .as_ref()
            .map(|state| state.collaboration_mode.clone())
            .ok_or(SessionError::NoThread)?;
        let request_id = self.next_request_id();
        let response: TurnStartResponse = self
            .client
            .request_typed(turn_start_request(
                request_id,
                thread_id,
                input,
                collaboration_mode,
            ))
            .await?;
        if let Some(state) = self.state.as_mut() {
            state.active_turn_id = Some(response.turn.id.clone());
        }
        Ok(response)
    }

    pub async fn interrupt(&mut self) -> Result<(), SessionError> {
        let state = self.state.as_ref().ok_or(SessionError::NoThread)?;
        let params = TurnInterruptParams {
            thread_id: state.thread.id.clone(),
            turn_id: state
                .active_turn_id
                .clone()
                .ok_or(SessionError::NoActiveTurn)?,
        };
        let request_id = self.next_request_id();
        let _: TurnInterruptResponse = self
            .client
            .request_typed(ClientRequest::TurnInterrupt { request_id, params })
            .await?;
        Ok(())
    }

    pub async fn resolve(&self, resolution: RequestResolution) -> Result<(), SessionError> {
        match resolution {
            RequestResolution::Success { request_id, result } => {
                self.client
                    .resolve_server_request(request_id, result)
                    .await?;
            }
            RequestResolution::Reject { request_id, error } => {
                self.client.reject_server_request(request_id, error).await?;
            }
        }
        Ok(())
    }

    pub async fn next_event(&mut self) -> Option<AppServerEvent> {
        let event = self.client.next_event().await?;
        if let AppServerEvent::ServerNotification(notification) = &event
            && let Some(state) = self.state.as_mut()
        {
            state.observe_notification(notification);
        }
        Some(event)
    }

    pub async fn shutdown(self) -> Result<(), SessionError> {
        self.client.shutdown().await?;
        Ok(())
    }

    fn next_request_id(&mut self) -> RequestId {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        RequestId::Integer(request_id)
    }
}

fn turn_start_request(
    request_id: RequestId,
    thread_id: String,
    input: Vec<UserInput>,
    collaboration_mode: CollaborationMode,
) -> ClientRequest {
    ClientRequest::TurnStart {
        request_id,
        params: TurnStartParams {
            thread_id,
            client_user_message_id: None,
            input,
            model_client_metadata: None,
            additional_context: None,
            environments: None,
            cwd: None,
            runtime_workspace_roots: None,
            approval_policy: None,
            approvals_reviewer: None,
            sandbox_policy: None,
            permissions: None,
            model: None,
            model_provider: None,
            service_tier: None,
            effort: None,
            summary: None,
            personality: None,
            output_schema: None,
            collaboration_mode: Some(collaboration_mode),
        },
    }
}

fn default_collaboration_mode(
    model: String,
    reasoning_effort: Option<ReasoningEffort>,
) -> CollaborationMode {
    CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model,
            reasoning_effort,
            developer_instructions: None,
        },
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
