use codex_app_server_protocol::CommandExecutionApprovalDecision;
use codex_app_server_protocol::FileChangeApprovalDecision;
use codex_app_server_protocol::GrantedPermissionProfile;
use codex_app_server_protocol::McpServerElicitationAction;
use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::McpServerElicitationRequestResponse;
use codex_app_server_protocol::PermissionGrantScope;
use codex_app_server_protocol::PermissionsRequestApprovalResponse;
use codex_app_server_protocol::Thread;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;

use crate::PendingRequest;
use crate::PendingRequestResponse;
use crate::PromptSubmission;
use crate::RequestResolution;
use crate::SlashInvocation;
use crate::SurfaceActivity;
use crate::SurfaceState;
use crate::ThreadPickerAction;
use crate::permission_picker::PermissionPickerInput;
use crate::permission_picker::PermissionSelection;
use crate::permission_picker::handle_key as handle_permission_picker_key;
use crate::request_user_input::RequestUserInputEvent;
use crate::theme_picker::ThemePickerInput;
use crate::theme_picker::handle_key as handle_theme_picker_key;
use crate::thread_picker::PickerInput;
use crate::thread_picker::handle_key as handle_thread_picker_key;

mod mention_popup;

#[derive(Debug, Clone, PartialEq)]
pub enum InputAction {
    None,
    Redraw,
    Submit(PromptSubmission),
    Interrupt,
    Exit,
    ScrollUp,
    ScrollDown,
    CopyLastResponse,
    Slash(SlashInvocation),
    ThreadPickerLoadNext,
    ThreadPickerSelect {
        action: ThreadPickerAction,
        thread: Box<Thread>,
    },
    SelectTheme(String),
    SelectPermission(PermissionSelection),
    CycleMode,
    OpenShortcuts,
    Resolve(RequestResolution),
    Notice(String),
}

pub fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    if key.kind == KeyEventKind::Release {
        return InputAction::None;
    }
    if let Some(request) = state.pending_requests().front().cloned() {
        return handle_request_key(state, request, key);
    }
    if state.thread_picker().is_some() {
        return handle_thread_picker_input(state, key);
    }
    if state.permission_picker().is_some() {
        return handle_permission_picker_input(state, key);
    }
    if state.theme_picker().is_some() {
        return handle_theme_picker_input(state, key);
    }
    if state.modal().is_some() {
        if key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('.') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            state.close_modal();
            return InputAction::Redraw;
        }
        let Some(modal) = state.modal_mut() else {
            return InputAction::None;
        };
        return match key.code {
            KeyCode::Up => {
                modal.scroll_by(-1);
                InputAction::Redraw
            }
            KeyCode::Down => {
                modal.scroll_by(1);
                InputAction::Redraw
            }
            KeyCode::PageUp => {
                modal.scroll_by(-10);
                InputAction::Redraw
            }
            KeyCode::PageDown => {
                modal.scroll_by(10);
                InputAction::Redraw
            }
            KeyCode::Home => {
                modal.scroll_to_start();
                InputAction::Redraw
            }
            KeyCode::End => {
                modal.scroll_to_end();
                InputAction::Redraw
            }
            _ => InputAction::None,
        };
    }
    if key.code == KeyCode::Esc && state.clear_scrollback_selection() {
        return InputAction::Redraw;
    }
    handle_composer_key(state, key)
}

pub fn handle_paste(state: &mut SurfaceState, text: &str) -> InputAction {
    if state.permission_picker().is_some()
        || state.theme_picker().is_some()
        || state.modal().is_some()
    {
        return InputAction::None;
    }
    if let Some(picker) = state.thread_picker_mut() {
        picker.paste(text);
        return InputAction::Redraw;
    }
    let user_input = state
        .pending_requests()
        .front()
        .and_then(|request| match request {
            PendingRequest::UserInput { params, .. } => Some(params.clone()),
            _ => None,
        });
    if let Some(params) = user_input {
        return if state.request_user_input_mut().handle_paste(&params, text) {
            InputAction::Redraw
        } else {
            InputAction::None
        };
    }
    state.composer_state_mut().insert_text(text);
    state.refresh_composer_completions();
    InputAction::Redraw
}

fn handle_theme_picker_input(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    let original = state
        .theme_picker()
        .map(crate::theme_picker::ThemePickerState::original);
    let Some(picker) = state.theme_picker_mut() else {
        return InputAction::None;
    };
    match handle_theme_picker_key(picker, key) {
        ThemePickerInput::None => InputAction::None,
        ThemePickerInput::Preview(theme) => {
            state.set_theme(theme);
            InputAction::Redraw
        }
        ThemePickerInput::Select(theme) => {
            state.set_theme(theme);
            state.close_theme_picker();
            InputAction::SelectTheme(theme.config_name().to_string())
        }
        ThemePickerInput::Cancel => {
            if let Some(original) = original {
                state.set_theme(original);
            }
            state.close_theme_picker();
            InputAction::Redraw
        }
    }
}

fn handle_permission_picker_input(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    let Some(picker) = state.permission_picker_mut() else {
        return InputAction::None;
    };
    match handle_permission_picker_key(picker, key) {
        PermissionPickerInput::None => InputAction::None,
        PermissionPickerInput::Redraw => InputAction::Redraw,
        PermissionPickerInput::Select(selection) => {
            state.close_permission_picker();
            InputAction::SelectPermission(selection)
        }
        PermissionPickerInput::Cancel => {
            state.close_permission_picker();
            InputAction::Redraw
        }
    }
}

fn handle_thread_picker_input(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    let Some(picker) = state.thread_picker_mut() else {
        return InputAction::None;
    };
    match handle_thread_picker_key(picker, key, /*terminal_height*/ 24) {
        PickerInput::None => InputAction::None,
        PickerInput::Redraw => InputAction::Redraw,
        PickerInput::LoadNext => InputAction::ThreadPickerLoadNext,
        PickerInput::Select(thread) => {
            let action = picker.action();
            state.close_thread_picker();
            InputAction::ThreadPickerSelect { action, thread }
        }
        PickerInput::Cancel => {
            state.close_thread_picker();
            InputAction::Redraw
        }
    }
}

fn handle_composer_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    if state.mentions().open
        && let Some(action) = mention_popup::handle_key(state, key)
    {
        return action;
    }
    if key.code == KeyCode::BackTab
        || (key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::SHIFT))
    {
        return InputAction::CycleMode;
    }
    if key.code == KeyCode::Char('.') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return InputAction::OpenShortcuts;
    }
    if state.slash().open {
        match key.code {
            KeyCode::Esc => {
                state.close_slash();
                return InputAction::Redraw;
            }
            KeyCode::Up => {
                state.move_slash_selection(-1);
                return InputAction::Redraw;
            }
            KeyCode::Down => {
                state.move_slash_selection(1);
                return InputAction::Redraw;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                state.accept_slash_selection();
                return InputAction::Redraw;
            }
            KeyCode::Enter if !state.slash().recognized => {
                state.accept_slash_selection();
                return InputAction::Redraw;
            }
            _ => {}
        }
    }
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            if matches!(state.activity(), SurfaceActivity::Working) {
                InputAction::Interrupt
            } else if state.composer().is_empty() {
                InputAction::Exit
            } else {
                state.composer_state_mut().clear();
                state.refresh_composer_completions();
                InputAction::Redraw
            }
        }
        (KeyCode::Char('d'), KeyModifiers::CONTROL) if state.composer().is_empty() => {
            InputAction::Exit
        }
        (KeyCode::Char('o'), KeyModifiers::CONTROL) => InputAction::CopyLastResponse,
        (KeyCode::PageUp, _) => InputAction::ScrollUp,
        (KeyCode::PageDown, _) => InputAction::ScrollDown,
        (KeyCode::Enter, modifiers)
            if !modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
        {
            if state.slash().active {
                return match state.slash_invocation() {
                    Ok(Some(invocation)) => {
                        state.take_composer();
                        state.record_slash(invocation.command);
                        InputAction::Slash(invocation)
                    }
                    Ok(None) => InputAction::Notice("Choose a slash command".to_string()),
                    Err(error) => InputAction::Notice(error.to_string()),
                };
            }
            let submission = state.take_submission();
            if submission.text().trim().is_empty() {
                InputAction::None
            } else {
                InputAction::Submit(submission)
            }
        }
        (KeyCode::Enter, _) => {
            state.composer_state_mut().insert_char('\n');
            state.refresh_composer_completions();
            InputAction::Redraw
        }
        _ if state.composer_state_mut().edit_key(key) => {
            state.refresh_composer_completions();
            InputAction::Redraw
        }
        _ => InputAction::None,
    }
}

fn handle_request_key(
    state: &mut SurfaceState,
    request: PendingRequest,
    key: KeyEvent,
) -> InputAction {
    let accepts_text_input = match &request {
        PendingRequest::UserInput { .. } => false,
        PendingRequest::McpElicitation { params, .. } => {
            matches!(&params.request, McpServerElicitationRequest::Form { .. })
        }
        _ => false,
    };
    let response = match request.clone() {
        PendingRequest::CommandExecution { params, .. } => command_response(&params, key.code),
        PendingRequest::FileChange { .. } => file_change_response(key.code),
        PendingRequest::Permissions { params, .. } => permissions_response(&params, key.code),
        PendingRequest::UserInput { params, .. } => {
            match state.request_user_input_mut().handle_key(&params, key) {
                RequestUserInputEvent::None => return InputAction::None,
                RequestUserInputEvent::Redraw => return InputAction::Redraw,
                RequestUserInputEvent::Submit(response) => {
                    Some(PendingRequestResponse::UserInput(response))
                }
                RequestUserInputEvent::Cancel => Some(PendingRequestResponse::Reject {
                    code: -32000,
                    message: "user input cancelled".to_string(),
                }),
            }
        }
        PendingRequest::McpElicitation { params, .. } => {
            mcp_response(state.composer(), &params.request, key.code)
        }
        PendingRequest::DynamicTool { .. } | PendingRequest::Attestation { .. } => None,
        PendingRequest::LegacyApplyPatch { .. } | PendingRequest::LegacyExecCommand { .. } => {
            Some(PendingRequestResponse::Reject {
                code: -32601,
                message: "Astral TUI accepts app-server v2 requests only".to_string(),
            })
        }
    };

    let Some(response) = response else {
        if !accepts_text_input {
            return InputAction::None;
        }
        return if state.composer_state_mut().edit_key(key) {
            InputAction::Redraw
        } else {
            InputAction::None
        };
    };

    let request_id = request.request_id().clone();
    match state.pending_requests_mut().resolve(&request_id, response) {
        Ok(resolution) => {
            match request {
                PendingRequest::UserInput { .. } => state.reset_request_user_input(),
                PendingRequest::McpElicitation { params, .. }
                    if matches!(params.request, McpServerElicitationRequest::Form { .. }) =>
                {
                    state.composer_state_mut().clear();
                }
                _ => {}
            }
            InputAction::Resolve(resolution)
        }
        Err(error) => InputAction::Notice(error.to_string()),
    }
}

fn command_response(
    params: &codex_app_server_protocol::CommandExecutionRequestApprovalParams,
    key: KeyCode,
) -> Option<PendingRequestResponse> {
    let decision = match key {
        KeyCode::Char('y') => CommandExecutionApprovalDecision::Accept,
        KeyCode::Char('a') => CommandExecutionApprovalDecision::AcceptForSession,
        KeyCode::Char('e') => CommandExecutionApprovalDecision::AcceptWithExecpolicyAmendment {
            execpolicy_amendment: params.proposed_execpolicy_amendment.clone()?,
        },
        KeyCode::Char('p') => CommandExecutionApprovalDecision::ApplyNetworkPolicyAmendment {
            network_policy_amendment: params
                .proposed_network_policy_amendments
                .as_ref()?
                .first()?
                .clone(),
        },
        KeyCode::Char('n') => CommandExecutionApprovalDecision::Decline,
        KeyCode::Esc => CommandExecutionApprovalDecision::Cancel,
        _ => return None,
    };
    if params
        .available_decisions
        .as_ref()
        .is_some_and(|available| !available.contains(&decision))
    {
        return None;
    }
    Some(PendingRequestResponse::CommandExecution(decision))
}

fn file_change_response(key: KeyCode) -> Option<PendingRequestResponse> {
    let decision = match key {
        KeyCode::Char('y') => FileChangeApprovalDecision::Accept,
        KeyCode::Char('a') => FileChangeApprovalDecision::AcceptForSession,
        KeyCode::Char('n') => FileChangeApprovalDecision::Decline,
        KeyCode::Esc => FileChangeApprovalDecision::Cancel,
        _ => return None,
    };
    Some(PendingRequestResponse::FileChange(decision))
}

fn permissions_response(
    params: &codex_app_server_protocol::PermissionsRequestApprovalParams,
    key: KeyCode,
) -> Option<PendingRequestResponse> {
    let scope = match key {
        KeyCode::Char('y') => PermissionGrantScope::Turn,
        KeyCode::Char('a') => PermissionGrantScope::Session,
        KeyCode::Char('n') | KeyCode::Esc => {
            return Some(PendingRequestResponse::Reject {
                code: -32000,
                message: "permission request declined".to_string(),
            });
        }
        _ => return None,
    };
    Some(PendingRequestResponse::Permissions(
        PermissionsRequestApprovalResponse {
            permissions: GrantedPermissionProfile {
                network: params.permissions.network.clone(),
                file_system: params.permissions.file_system.clone(),
            },
            scope,
            strict_auto_review: None,
        },
    ))
}

fn mcp_response(
    composer: &str,
    request: &McpServerElicitationRequest,
    key: KeyCode,
) -> Option<PendingRequestResponse> {
    let (action, content) = match key {
        KeyCode::Char('n') => (McpServerElicitationAction::Decline, None),
        KeyCode::Esc => (McpServerElicitationAction::Cancel, None),
        KeyCode::Char('y') if matches!(request, McpServerElicitationRequest::Url { .. }) => {
            (McpServerElicitationAction::Accept, None)
        }
        KeyCode::Enter if matches!(request, McpServerElicitationRequest::Form { .. }) => {
            let content = serde_json::from_str(composer).ok()?;
            (McpServerElicitationAction::Accept, Some(content))
        }
        _ => return None,
    };
    Some(PendingRequestResponse::McpElicitation(
        McpServerElicitationRequestResponse {
            action,
            content,
            meta: None,
        },
    ))
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "input_editor_tests.rs"]
mod editor_tests;
