use std::io;

mod ecosystem;

use codex_app_server_client::AppServerClient;
use codex_app_server_client::AppServerEvent;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ActivePermissionProfile;
use codex_app_server_protocol::ApprovalsReviewer;
use codex_app_server_protocol::AskForApproval;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::CollaborationModeListParams;
use codex_app_server_protocol::CollaborationModeListResponse;
use codex_app_server_protocol::CollaborationModeMask;
use codex_app_server_protocol::Model;
use codex_app_server_protocol::ModelListParams;
use codex_app_server_protocol::ModelListResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadCompactStartParams;
use codex_app_server_protocol::ThreadCompactStartResponse;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadForkResponse;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadSetNameParams;
use codex_app_server_protocol::ThreadSetNameResponse;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_app_server_protocol::ThreadSettingsUpdateResponse;
use codex_app_server_protocol::ThreadSortKey;
use codex_app_server_protocol::ThreadSource;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::ThreadStartSource;
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
use crate::model_command::ModelSelection;
use crate::permission_picker::PermissionSelection;

#[derive(Debug)]
pub enum SessionError {
    NoThread,
    NoActiveTurn,
    CollaborationModeUnavailable(ModeKind),
    Request(TypedRequestError),
    Transport(io::Error),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoThread => f.write_str("no Astral thread is active"),
            Self::NoActiveTurn => f.write_str("no Astral turn is active"),
            Self::CollaborationModeUnavailable(mode) => {
                write!(f, "{} mode is unavailable", mode.display_name())
            }
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
            Self::NoThread | Self::NoActiveTurn | Self::CollaborationModeUnavailable(_) => None,
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
    pub approval_policy: AskForApproval,
    pub approvals_reviewer: ApprovalsReviewer,
    pub active_permission_profile: Option<ActivePermissionProfile>,
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
            approval_policy: response.approval_policy,
            approvals_reviewer: response.approvals_reviewer,
            active_permission_profile: response.active_permission_profile,
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
            approval_policy: response.approval_policy,
            approvals_reviewer: response.approvals_reviewer,
            active_permission_profile: response.active_permission_profile,
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
            approval_policy: response.approval_policy,
            approvals_reviewer: response.approvals_reviewer,
            active_permission_profile: response.active_permission_profile,
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
                self.collaboration_mode = params.thread_settings.collaboration_mode.clone();
                self.approval_policy = params.thread_settings.approval_policy;
                self.approvals_reviewer = params.thread_settings.approvals_reviewer;
                self.active_permission_profile
                    .clone_from(&params.thread_settings.active_permission_profile);
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
    default_reasoning_effort: Option<ReasoningEffort>,
}

impl AstralSession {
    pub fn new(client: AppServerClient) -> Self {
        Self {
            client,
            next_request_id: 1,
            state: None,
            default_reasoning_effort: None,
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
        let state = SessionState::from_start(response);
        self.default_reasoning_effort = state.collaboration_mode.settings.reasoning_effort.clone();
        self.state = Some(state);
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
        let state = SessionState::from_resume(response);
        self.default_reasoning_effort = state.collaboration_mode.settings.reasoning_effort.clone();
        self.state = Some(state);
        self.state.as_ref().ok_or(SessionError::NoThread)
    }

    pub async fn fork(&mut self, params: ThreadForkParams) -> Result<&SessionState, SessionError> {
        let request_id = self.next_request_id();
        let response = self
            .client
            .request_typed(ClientRequest::ThreadFork { request_id, params })
            .await?;
        let state = SessionState::from_fork(response);
        self.default_reasoning_effort = state.collaboration_mode.settings.reasoning_effort.clone();
        self.state = Some(state);
        self.state.as_ref().ok_or(SessionError::NoThread)
    }

    pub(crate) async fn start_new(&mut self) -> Result<&SessionState, SessionError> {
        let state = self.state.as_ref().ok_or(SessionError::NoThread)?;
        let params = ThreadStartParams {
            model: Some(state.model.clone()),
            model_provider: Some(state.model_provider.clone()),
            service_tier: Some(state.service_tier.clone()),
            cwd: Some(state.thread.cwd.to_string_lossy().to_string()),
            approval_policy: Some(state.approval_policy),
            approvals_reviewer: Some(state.approvals_reviewer),
            permissions: state
                .active_permission_profile
                .as_ref()
                .map(|profile| profile.id.clone()),
            session_start_source: Some(ThreadStartSource::Startup),
            thread_source: Some(ThreadSource::User),
            ..ThreadStartParams::default()
        };
        self.start(params).await
    }

    pub(crate) async fn resume_thread(
        &mut self,
        thread_id: String,
    ) -> Result<&SessionState, SessionError> {
        self.resume(ThreadResumeParams {
            thread_id,
            ..ThreadResumeParams::default()
        })
        .await
    }

    pub(crate) async fn fork_current(&mut self) -> Result<&SessionState, SessionError> {
        let thread_id = self
            .state
            .as_ref()
            .map(|state| state.thread.id.clone())
            .ok_or(SessionError::NoThread)?;
        self.fork(ThreadForkParams {
            thread_id,
            ..ThreadForkParams::default()
        })
        .await
    }

    pub(crate) async fn list_threads(
        &mut self,
        cursor: Option<String>,
    ) -> Result<ThreadListResponse, SessionError> {
        let request_id = self.next_request_id();
        let response = self
            .client
            .request_typed(ClientRequest::ThreadList {
                request_id,
                params: ThreadListParams {
                    cursor,
                    limit: Some(100),
                    sort_key: Some(ThreadSortKey::UpdatedAt),
                    sort_direction: None,
                    model_providers: None,
                    source_kinds: None,
                    archived: Some(false),
                    cwd: None,
                    use_state_db_only: false,
                    search_term: None,
                },
            })
            .await?;
        Ok(response)
    }

    pub(crate) async fn rename(&mut self, name: String) -> Result<(), SessionError> {
        let thread_id = self
            .state
            .as_ref()
            .map(|state| state.thread.id.clone())
            .ok_or(SessionError::NoThread)?;
        let request_id = self.next_request_id();
        let _: ThreadSetNameResponse = self
            .client
            .request_typed(ClientRequest::ThreadSetName {
                request_id,
                params: ThreadSetNameParams { thread_id, name },
            })
            .await?;
        Ok(())
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

    pub(crate) async fn list_models(&mut self) -> Result<Vec<Model>, SessionError> {
        let mut cursor = None;
        let mut models = Vec::new();
        for _ in 0..5 {
            let request_id = self.next_request_id();
            let response: ModelListResponse = self
                .client
                .request_typed(ClientRequest::ModelList {
                    request_id,
                    params: ModelListParams {
                        cursor,
                        model_provider: None,
                        limit: Some(100),
                        include_hidden: Some(false),
                    },
                })
                .await?;
            models.extend(response.data);
            if models.len() >= 500 {
                models.truncate(500);
                break;
            }
            let Some(next_cursor) = response.next_cursor else {
                break;
            };
            cursor = Some(next_cursor);
        }
        Ok(models)
    }

    pub(crate) async fn compact(&mut self) -> Result<(), SessionError> {
        let thread_id = self
            .state
            .as_ref()
            .map(|state| state.thread.id.clone())
            .ok_or(SessionError::NoThread)?;
        let request_id = self.next_request_id();
        let _: ThreadCompactStartResponse = self
            .client
            .request_typed(ClientRequest::ThreadCompactStart {
                request_id,
                params: ThreadCompactStartParams { thread_id },
            })
            .await?;
        Ok(())
    }

    pub(crate) async fn update_model(
        &mut self,
        selection: &ModelSelection,
    ) -> Result<(), SessionError> {
        let (thread_id, mode) = self
            .state
            .as_ref()
            .map(|state| (state.thread.id.clone(), state.collaboration_mode.mode))
            .ok_or(SessionError::NoThread)?;
        let collaboration_mode = if mode == ModeKind::Plan {
            let mask = self.collaboration_mode_mask(mode).await?;
            Some(
                collaboration_mode_from_mask(
                    &selection.model,
                    Some(selection.effort.clone()),
                    mask,
                )
                .ok_or(SessionError::CollaborationModeUnavailable(mode))?,
            )
        } else {
            None
        };
        let request_id = self.next_request_id();
        let _: ThreadSettingsUpdateResponse = self
            .client
            .request_typed(ClientRequest::ThreadSettingsUpdate {
                request_id,
                params: ThreadSettingsUpdateParams {
                    thread_id,
                    model: Some(selection.model.clone()),
                    model_provider: Some(selection.model_provider.clone()),
                    effort: Some(selection.effort.clone()),
                    collaboration_mode: collaboration_mode.clone(),
                    ..ThreadSettingsUpdateParams::default()
                },
            })
            .await?;
        self.default_reasoning_effort = Some(selection.effort.clone());
        if let Some(state) = self.state.as_mut() {
            state.model.clone_from(&selection.model);
            state.model_provider.clone_from(&selection.model_provider);
            state.collaboration_mode = collaboration_mode.unwrap_or_else(|| {
                state.collaboration_mode.with_updates(
                    Some(selection.model.clone()),
                    Some(Some(selection.effort.clone())),
                    None,
                )
            });
        }
        Ok(())
    }

    pub(crate) async fn update_collaboration_mode(
        &mut self,
        mode: ModeKind,
    ) -> Result<(), SessionError> {
        let mask = self.collaboration_mode_mask(mode).await?;
        let (thread_id, model) = self
            .state
            .as_ref()
            .map(|state| (state.thread.id.clone(), state.model.clone()))
            .ok_or(SessionError::NoThread)?;
        let collaboration_mode =
            collaboration_mode_from_mask(&model, self.default_reasoning_effort.clone(), mask)
                .ok_or(SessionError::CollaborationModeUnavailable(mode))?;
        let request_id = self.next_request_id();
        let _: ThreadSettingsUpdateResponse = self
            .client
            .request_typed(ClientRequest::ThreadSettingsUpdate {
                request_id,
                params: ThreadSettingsUpdateParams {
                    thread_id,
                    collaboration_mode: Some(collaboration_mode.clone()),
                    ..ThreadSettingsUpdateParams::default()
                },
            })
            .await?;
        if let Some(state) = self.state.as_mut() {
            state.collaboration_mode = collaboration_mode;
        }
        Ok(())
    }

    async fn collaboration_mode_mask(
        &mut self,
        mode: ModeKind,
    ) -> Result<CollaborationModeMask, SessionError> {
        let request_id = self.next_request_id();
        let response: CollaborationModeListResponse = self
            .client
            .request_typed(ClientRequest::CollaborationModeList {
                request_id,
                params: CollaborationModeListParams::default(),
            })
            .await?;
        response
            .data
            .into_iter()
            .find(|mask| mask.mode == Some(mode))
            .ok_or(SessionError::CollaborationModeUnavailable(mode))
    }

    pub(crate) async fn update_permissions(
        &mut self,
        selection: PermissionSelection,
    ) -> Result<(), SessionError> {
        let thread_id = self
            .state
            .as_ref()
            .map(|state| state.thread.id.clone())
            .ok_or(SessionError::NoThread)?;
        let request_id = self.next_request_id();
        let _: ThreadSettingsUpdateResponse = self
            .client
            .request_typed(ClientRequest::ThreadSettingsUpdate {
                request_id,
                params: ThreadSettingsUpdateParams {
                    thread_id,
                    approval_policy: Some(selection.approval_policy()),
                    permissions: Some(selection.profile_id().to_string()),
                    ..ThreadSettingsUpdateParams::default()
                },
            })
            .await?;
        if let Some(state) = self.state.as_mut() {
            state.approval_policy = selection.approval_policy();
            state.active_permission_profile =
                Some(ActivePermissionProfile::new(selection.profile_id()));
        }
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
        if let AppServerEvent::ServerNotification(notification) = &event {
            if let ServerNotification::ThreadSettingsUpdated(params) = notification
                && self
                    .state
                    .as_ref()
                    .is_some_and(|state| state.thread.id == params.thread_id)
            {
                self.default_reasoning_effort = params.thread_settings.effort.clone();
            }
            if let Some(state) = self.state.as_mut() {
                state.observe_notification(notification);
            }
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

fn collaboration_mode_from_mask(
    default_model: &str,
    default_reasoning_effort: Option<ReasoningEffort>,
    mask: CollaborationModeMask,
) -> Option<CollaborationMode> {
    Some(CollaborationMode {
        mode: mask.mode?,
        settings: Settings {
            model: mask.model.unwrap_or_else(|| default_model.to_string()),
            reasoning_effort: mask.reasoning_effort.unwrap_or(default_reasoning_effort),
            developer_instructions: None,
        },
    })
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
