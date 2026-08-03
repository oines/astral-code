use std::io;

use codex_app_server_client::AppServerClient;
use codex_app_server_client::AppServerEvent;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ActivePermissionProfile;
use codex_app_server_protocol::ApprovalsReviewer;
use codex_app_server_protocol::AskForApproval;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::Result as JsonRpcResult;
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
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_protocol::openai_models::ReasoningEffort;

#[derive(Debug)]
pub enum SessionError {
    NoThread,
    NoActiveTurn,
    Request(TypedRequestError),
    Transport(io::Error),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoThread => formatter.write_str("no Astral thread is active"),
            Self::NoActiveTurn => formatter.write_str("no Astral turn is active"),
            Self::Request(error) => write!(formatter, "{error}"),
            Self::Transport(error) => write!(formatter, "{error}"),
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
/// The authoritative transcript remains in app-server snapshots and
/// notifications. This state tracks only lifecycle identifiers and settings
/// that the runtime needs for input, status, and interactive requests. The
/// thread's turns are the launch snapshot; live transcript state is owned by
/// the scrollback projection.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionState {
    pub thread: Thread,
    pub model: String,
    pub model_provider: String,
    pub service_tier: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub approval_policy: AskForApproval,
    pub approvals_reviewer: ApprovalsReviewer,
    pub active_permission_profile: Option<ActivePermissionProfile>,
    pub active_turn_id: Option<String>,
}

impl SessionState {
    fn from_start(response: ThreadStartResponse) -> Self {
        Self {
            thread: response.thread,
            model: response.model,
            model_provider: response.model_provider,
            service_tier: response.service_tier,
            reasoning_effort: response.reasoning_effort,
            approval_policy: response.approval_policy,
            approvals_reviewer: response.approvals_reviewer,
            active_permission_profile: response.active_permission_profile,
            active_turn_id: None,
        }
    }

    fn from_resume(response: ThreadResumeResponse) -> Self {
        let active_turn_id = response
            .thread
            .turns
            .iter()
            .rev()
            .find(|turn| turn.status == TurnStatus::InProgress)
            .map(|turn| turn.id.clone());
        Self {
            thread: response.thread,
            model: response.model,
            model_provider: response.model_provider,
            service_tier: response.service_tier,
            reasoning_effort: response.reasoning_effort,
            approval_policy: response.approval_policy,
            approvals_reviewer: response.approvals_reviewer,
            active_permission_profile: response.active_permission_profile,
            active_turn_id,
        }
    }

    fn from_fork(response: ThreadForkResponse) -> Self {
        Self {
            thread: response.thread,
            model: response.model,
            model_provider: response.model_provider,
            service_tier: response.service_tier,
            reasoning_effort: response.reasoning_effort,
            approval_policy: response.approval_policy,
            approvals_reviewer: response.approvals_reviewer,
            active_permission_profile: response.active_permission_profile,
            active_turn_id: None,
        }
    }

    fn observe_notification(&mut self, notification: &ServerNotification) {
        match notification {
            ServerNotification::TurnStarted(params) if params.thread_id == self.thread.id => {
                self.active_turn_id = Some(params.turn.id.clone());
            }
            ServerNotification::TurnCompleted(params) if params.thread_id == self.thread.id => {
                if self.active_turn_id.as_deref() == Some(params.turn.id.as_str()) {
                    self.active_turn_id = None;
                }
            }
            ServerNotification::ThreadClosed(params) if params.thread_id == self.thread.id => {
                self.active_turn_id = None;
            }
            ServerNotification::ThreadNameUpdated(params) if params.thread_id == self.thread.id => {
                self.thread.name.clone_from(&params.thread_name);
            }
            ServerNotification::ThreadStatusChanged(params)
                if params.thread_id == self.thread.id =>
            {
                self.thread.status = params.status.clone();
            }
            ServerNotification::ThreadSettingsUpdated(params)
                if params.thread_id == self.thread.id =>
            {
                let settings = &params.thread_settings;
                self.thread.cwd.clone_from(&settings.cwd);
                self.model.clone_from(&settings.model);
                self.model_provider.clone_from(&settings.model_provider);
                self.thread
                    .model_provider
                    .clone_from(&settings.model_provider);
                self.service_tier.clone_from(&settings.service_tier);
                self.reasoning_effort.clone_from(&settings.effort);
                self.approval_policy = settings.approval_policy;
                self.approvals_reviewer = settings.approvals_reviewer;
                self.active_permission_profile
                    .clone_from(&settings.active_permission_profile);
            }
            _ => {}
        }
    }
}

/// Thin app-server v2 session used by the Astral surface.
///
/// It deliberately owns no transcript reducer, model runtime, tool adapter, or
/// interactive request queue. New and remote transports share the same typed
/// requests and event stream.
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
        let request_id = self.next_request_id();
        let response: TurnStartResponse = self
            .client
            .request_typed(turn_start_request(request_id, thread_id, input))
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

    pub async fn resolve_server_request(
        &self,
        request_id: RequestId,
        result: JsonRpcResult,
    ) -> Result<(), SessionError> {
        self.client
            .resolve_server_request(request_id, result)
            .await?;
        Ok(())
    }

    pub async fn reject_server_request(
        &self,
        request_id: RequestId,
        error: JSONRPCErrorError,
    ) -> Result<(), SessionError> {
        self.client.reject_server_request(request_id, error).await?;
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
) -> ClientRequest {
    ClientRequest::TurnStart {
        request_id,
        params: TurnStartParams {
            thread_id,
            input,
            ..TurnStartParams::default()
        },
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
