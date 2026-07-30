//! Interaction state for simple app-server approval requests.
//!
//! Keyboard and pointer behavior follows Grok Build's permission view at
//! commit 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0). Astral keeps
//! its own typed app-server decisions; this module only owns TUI selection.

use std::time::Duration;
use std::time::Instant;

use codex_app_server_protocol::CommandExecutionApprovalDecision;
use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::FileChangeApprovalDecision;
use codex_app_server_protocol::GrantedPermissionProfile;
use codex_app_server_protocol::McpServerElicitationAction;
use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::McpServerElicitationRequestResponse;
use codex_app_server_protocol::PermissionGrantScope;
use codex_app_server_protocol::PermissionsRequestApprovalResponse;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::layout::Rect;
use url::Url;

use crate::PendingRequest;
use crate::PendingRequestResponse;

const MULTI_CLICK_TIMEOUT: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestChoiceId {
    CommandAccept,
    CommandAcceptForSession,
    CommandExecpolicyAmendment,
    CommandNetworkPolicyAmendment,
    CommandDecline,
    FileAccept,
    FileAcceptForSession,
    FileDecline,
    PermissionTurn,
    PermissionSession,
    PermissionDecline,
    McpUrlAccept,
    McpUrlDecline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RequestChoice {
    pub(crate) id: RequestChoiceId,
    pub(crate) shortcut: char,
    pub(crate) label: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RequestChoiceEvent {
    None,
    Redraw,
    FocusScrollback,
    OpenUrl(String),
    Notice(String),
    Activate(RequestChoiceId),
    Cancel,
}

#[derive(Debug, Default)]
pub(crate) struct RequestChoiceState {
    source: Option<PendingRequest>,
    choices: Vec<RequestChoice>,
    selected: usize,
    hovered: Option<usize>,
    hit_rows: Vec<(usize, Rect)>,
    last_click: Option<(Instant, usize)>,
    mcp_url_opened: bool,
}

impl RequestChoiceState {
    pub(crate) fn sync(&mut self, request: Option<&PendingRequest>) {
        let Some(request) = request.filter(|request| is_simple_request(request)) else {
            self.reset();
            return;
        };
        if self.source.as_ref() == Some(request) {
            return;
        }
        self.source = Some(request.clone());
        self.choices = choices_for(request);
        self.selected = 0;
        self.hovered = None;
        self.hit_rows.clear();
        self.last_click = None;
        self.mcp_url_opened = false;
    }

    pub(crate) fn reset(&mut self) {
        self.source = None;
        self.choices.clear();
        self.selected = 0;
        self.hovered = None;
        self.hit_rows.clear();
        self.last_click = None;
        self.mcp_url_opened = false;
    }

    pub(crate) fn choices(&self) -> &[RequestChoice] {
        &self.choices
    }

    pub(crate) fn selected(&self) -> Option<usize> {
        (!self.choices.is_empty()).then_some(self.selected.min(self.choices.len() - 1))
    }

    pub(crate) fn hovered(&self) -> Option<usize> {
        self.hovered
    }

    pub(crate) fn observe_rows(&mut self, hit_rows: Vec<(usize, Rect)>) {
        self.hit_rows = hit_rows;
        if self
            .hovered
            .is_some_and(|hovered| !self.hit_rows.iter().any(|(index, _)| *index == hovered))
        {
            self.hovered = None;
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> RequestChoiceEvent {
        match (key.code, key.modifiers) {
            (KeyCode::Up | KeyCode::Char('k'), KeyModifiers::NONE) => {
                self.selected = self.selected.saturating_sub(1);
                RequestChoiceEvent::Redraw
            }
            (KeyCode::Down | KeyCode::Char('j'), KeyModifiers::NONE) => {
                self.selected = (self.selected + 1).min(self.choices.len().saturating_sub(1));
                RequestChoiceEvent::Redraw
            }
            (KeyCode::Enter, KeyModifiers::NONE) => self
                .selected_choice()
                .map_or(RequestChoiceEvent::None, |choice| self.activate(choice)),
            (KeyCode::Tab, KeyModifiers::NONE) => RequestChoiceEvent::FocusScrollback,
            (KeyCode::Esc, KeyModifiers::NONE) => RequestChoiceEvent::Cancel,
            (KeyCode::Char(character), KeyModifiers::NONE) => {
                let index = if ('1'..='9').contains(&character) {
                    usize::try_from(character.to_digit(10).unwrap_or_default())
                        .unwrap_or_default()
                        .saturating_sub(1)
                } else {
                    let shortcut = character.to_ascii_lowercase();
                    let Some(index) = self
                        .choices
                        .iter()
                        .position(|choice| choice.shortcut == shortcut)
                    else {
                        return RequestChoiceEvent::None;
                    };
                    index
                };
                let Some(choice) = self.choices.get(index).copied() else {
                    return RequestChoiceEvent::None;
                };
                self.selected = index;
                self.activate(choice)
            }
            _ => RequestChoiceEvent::None,
        }
    }

    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent) -> RequestChoiceEvent {
        self.handle_mouse_at(mouse, Instant::now())
    }

    fn handle_mouse_at(&mut self, mouse: MouseEvent, now: Instant) -> RequestChoiceEvent {
        let hit = self.hit_test(mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Moved => {
                if self.hovered == hit {
                    RequestChoiceEvent::None
                } else {
                    self.hovered = hit;
                    RequestChoiceEvent::Redraw
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(index) = hit else {
                    self.last_click = None;
                    return RequestChoiceEvent::None;
                };
                let is_double_click = self.last_click.is_some_and(|(last, last_index)| {
                    last_index == index && now.duration_since(last) < MULTI_CLICK_TIMEOUT
                });
                self.selected = index;
                self.hovered = Some(index);
                if is_double_click {
                    self.last_click = None;
                    self.choices
                        .get(index)
                        .copied()
                        .map_or(RequestChoiceEvent::Redraw, |choice| self.activate(choice))
                } else {
                    self.last_click = Some((now, index));
                    RequestChoiceEvent::Redraw
                }
            }
            MouseEventKind::Drag(MouseButton::Left)
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => {
                self.last_click = None;
                RequestChoiceEvent::None
            }
            MouseEventKind::Down(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Up(MouseButton::Left | MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Drag(MouseButton::Right | MouseButton::Middle) => {
                RequestChoiceEvent::None
            }
        }
    }

    fn selected_choice(&self) -> Option<RequestChoice> {
        self.selected()
            .and_then(|selected| self.choices.get(selected))
            .copied()
    }

    fn activate(&mut self, selected: RequestChoice) -> RequestChoiceEvent {
        if selected.id != RequestChoiceId::McpUrlAccept || self.mcp_url_opened {
            return RequestChoiceEvent::Activate(selected.id);
        }
        let Some(url) = self.source.as_ref().and_then(mcp_url).map(str::to_string) else {
            return RequestChoiceEvent::Activate(selected.id);
        };
        let Some(url) = validate_external_url(&url) else {
            return RequestChoiceEvent::Notice(
                "Refused to open an invalid or insecure MCP URL".to_string(),
            );
        };

        self.mcp_url_opened = true;
        self.choices = vec![
            choice(RequestChoiceId::McpUrlAccept, 'y', "I finished"),
            choice(RequestChoiceId::McpUrlDecline, 'n', "Decline"),
        ];
        self.selected = 0;
        self.hovered = None;
        self.hit_rows.clear();
        self.last_click = None;
        RequestChoiceEvent::OpenUrl(url)
    }

    fn hit_test(&self, column: u16, row: u16) -> Option<usize> {
        self.hit_rows
            .iter()
            .find(|(_, area)| area.contains((column, row).into()))
            .map(|(index, _)| *index)
    }
}

fn mcp_url(request: &PendingRequest) -> Option<&str> {
    let PendingRequest::McpElicitation { params, .. } = request else {
        return None;
    };
    let McpServerElicitationRequest::Url { url, .. } = &params.request else {
        return None;
    };
    Some(url)
}

fn validate_external_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    Some(parsed.into())
}

pub(crate) fn is_simple_request(request: &PendingRequest) -> bool {
    matches!(
        request,
        PendingRequest::CommandExecution { .. }
            | PendingRequest::FileChange { .. }
            | PendingRequest::Permissions { .. }
            | PendingRequest::McpElicitation {
                params: codex_app_server_protocol::McpServerElicitationRequestParams {
                    request: McpServerElicitationRequest::Url { .. },
                    ..
                },
                ..
            }
    )
}

pub(crate) fn response_for(
    request: &PendingRequest,
    choice: RequestChoiceId,
) -> Option<PendingRequestResponse> {
    if !choices_for(request)
        .iter()
        .any(|candidate| candidate.id == choice)
    {
        return None;
    }
    match (request, choice) {
        (
            PendingRequest::CommandExecution { params, .. },
            RequestChoiceId::CommandAccept
            | RequestChoiceId::CommandAcceptForSession
            | RequestChoiceId::CommandExecpolicyAmendment
            | RequestChoiceId::CommandNetworkPolicyAmendment
            | RequestChoiceId::CommandDecline,
        ) => Some(PendingRequestResponse::CommandExecution(command_decision(
            params, choice,
        )?)),
        (PendingRequest::FileChange { .. }, RequestChoiceId::FileAccept) => Some(
            PendingRequestResponse::FileChange(FileChangeApprovalDecision::Accept),
        ),
        (PendingRequest::FileChange { .. }, RequestChoiceId::FileAcceptForSession) => Some(
            PendingRequestResponse::FileChange(FileChangeApprovalDecision::AcceptForSession),
        ),
        (PendingRequest::FileChange { .. }, RequestChoiceId::FileDecline) => Some(
            PendingRequestResponse::FileChange(FileChangeApprovalDecision::Decline),
        ),
        (PendingRequest::Permissions { params, .. }, RequestChoiceId::PermissionTurn) => {
            Some(permission_response(params, PermissionGrantScope::Turn))
        }
        (PendingRequest::Permissions { params, .. }, RequestChoiceId::PermissionSession) => {
            Some(permission_response(params, PermissionGrantScope::Session))
        }
        (PendingRequest::Permissions { .. }, RequestChoiceId::PermissionDecline) => {
            Some(permission_decline())
        }
        (
            PendingRequest::McpElicitation { params, .. },
            RequestChoiceId::McpUrlAccept | RequestChoiceId::McpUrlDecline,
        ) if matches!(params.request, McpServerElicitationRequest::Url { .. }) => {
            let action = if choice == RequestChoiceId::McpUrlAccept {
                McpServerElicitationAction::Accept
            } else {
                McpServerElicitationAction::Decline
            };
            Some(mcp_response(action))
        }
        _ => None,
    }
}

pub(crate) fn cancel_response(request: &PendingRequest) -> Option<PendingRequestResponse> {
    match request {
        PendingRequest::CommandExecution { params, .. } => {
            let decision = CommandExecutionApprovalDecision::Cancel;
            command_available(params, &decision)
                .then_some(PendingRequestResponse::CommandExecution(decision))
        }
        PendingRequest::FileChange { .. } => Some(PendingRequestResponse::FileChange(
            FileChangeApprovalDecision::Cancel,
        )),
        PendingRequest::Permissions { .. } => Some(permission_decline()),
        PendingRequest::McpElicitation { params, .. }
            if matches!(params.request, McpServerElicitationRequest::Url { .. }) =>
        {
            Some(mcp_response(McpServerElicitationAction::Cancel))
        }
        PendingRequest::UserInput { .. }
        | PendingRequest::McpElicitation { .. }
        | PendingRequest::DynamicTool { .. }
        | PendingRequest::Attestation { .. }
        | PendingRequest::LegacyApplyPatch { .. }
        | PendingRequest::LegacyExecCommand { .. } => None,
    }
}

fn choices_for(request: &PendingRequest) -> Vec<RequestChoice> {
    match request {
        PendingRequest::CommandExecution { params, .. } => command_choices(params),
        PendingRequest::FileChange { .. } => vec![
            choice(RequestChoiceId::FileAccept, 'y', "Allow once"),
            choice(
                RequestChoiceId::FileAcceptForSession,
                'a',
                "Allow for this session",
            ),
            choice(RequestChoiceId::FileDecline, 'n', "Deny"),
        ],
        PendingRequest::Permissions { .. } => vec![
            choice(RequestChoiceId::PermissionTurn, 'y', "Allow for this turn"),
            choice(
                RequestChoiceId::PermissionSession,
                'a',
                "Allow for this session",
            ),
            choice(RequestChoiceId::PermissionDecline, 'n', "Deny"),
        ],
        PendingRequest::McpElicitation { params, .. }
            if matches!(params.request, McpServerElicitationRequest::Url { .. }) =>
        {
            vec![
                choice(RequestChoiceId::McpUrlAccept, 'y', "Open and continue"),
                choice(RequestChoiceId::McpUrlDecline, 'n', "Decline"),
            ]
        }
        PendingRequest::UserInput { .. }
        | PendingRequest::McpElicitation { .. }
        | PendingRequest::DynamicTool { .. }
        | PendingRequest::Attestation { .. }
        | PendingRequest::LegacyApplyPatch { .. }
        | PendingRequest::LegacyExecCommand { .. } => Vec::new(),
    }
}

fn command_choices(params: &CommandExecutionRequestApprovalParams) -> Vec<RequestChoice> {
    let candidates = [
        choice(RequestChoiceId::CommandAccept, 'y', "Allow once"),
        choice(
            RequestChoiceId::CommandAcceptForSession,
            'a',
            "Allow for this session",
        ),
        choice(
            RequestChoiceId::CommandExecpolicyAmendment,
            'e',
            "Trust the proposed command pattern",
        ),
        choice(
            RequestChoiceId::CommandNetworkPolicyAmendment,
            'p',
            "Apply the proposed network rule",
        ),
        choice(RequestChoiceId::CommandDecline, 'n', "Deny"),
    ];
    candidates
        .into_iter()
        .filter(|candidate| {
            command_decision(params, candidate.id)
                .is_some_and(|decision| command_available(params, &decision))
        })
        .collect()
}

fn command_decision(
    params: &CommandExecutionRequestApprovalParams,
    choice: RequestChoiceId,
) -> Option<CommandExecutionApprovalDecision> {
    match choice {
        RequestChoiceId::CommandAccept => Some(CommandExecutionApprovalDecision::Accept),
        RequestChoiceId::CommandAcceptForSession => {
            Some(CommandExecutionApprovalDecision::AcceptForSession)
        }
        RequestChoiceId::CommandExecpolicyAmendment => Some(
            CommandExecutionApprovalDecision::AcceptWithExecpolicyAmendment {
                execpolicy_amendment: params.proposed_execpolicy_amendment.clone()?,
            },
        ),
        RequestChoiceId::CommandNetworkPolicyAmendment => Some(
            CommandExecutionApprovalDecision::ApplyNetworkPolicyAmendment {
                network_policy_amendment: params
                    .proposed_network_policy_amendments
                    .as_ref()?
                    .first()?
                    .clone(),
            },
        ),
        RequestChoiceId::CommandDecline => Some(CommandExecutionApprovalDecision::Decline),
        RequestChoiceId::FileAccept
        | RequestChoiceId::FileAcceptForSession
        | RequestChoiceId::FileDecline
        | RequestChoiceId::PermissionTurn
        | RequestChoiceId::PermissionSession
        | RequestChoiceId::PermissionDecline
        | RequestChoiceId::McpUrlAccept
        | RequestChoiceId::McpUrlDecline => None,
    }
}

fn command_available(
    params: &CommandExecutionRequestApprovalParams,
    decision: &CommandExecutionApprovalDecision,
) -> bool {
    params
        .available_decisions
        .as_ref()
        .is_none_or(|available| available.contains(decision))
}

fn permission_response(
    params: &codex_app_server_protocol::PermissionsRequestApprovalParams,
    scope: PermissionGrantScope,
) -> PendingRequestResponse {
    PendingRequestResponse::Permissions(PermissionsRequestApprovalResponse {
        permissions: GrantedPermissionProfile {
            network: params.permissions.network.clone(),
            file_system: params.permissions.file_system.clone(),
        },
        scope,
        strict_auto_review: None,
    })
}

fn permission_decline() -> PendingRequestResponse {
    PendingRequestResponse::Reject {
        code: -32000,
        message: "permission request declined".to_string(),
    }
}

fn mcp_response(action: McpServerElicitationAction) -> PendingRequestResponse {
    PendingRequestResponse::McpElicitation(McpServerElicitationRequestResponse {
        action,
        content: None,
        meta: None,
    })
}

const fn choice(id: RequestChoiceId, shortcut: char, label: &'static str) -> RequestChoice {
    RequestChoice {
        id,
        shortcut,
        label,
    }
}

#[cfg(test)]
#[path = "request_choice_tests.rs"]
mod tests;
