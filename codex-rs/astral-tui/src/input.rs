use std::collections::HashMap;

use codex_app_server_protocol::CommandExecutionApprovalDecision;
use codex_app_server_protocol::FileChangeApprovalDecision;
use codex_app_server_protocol::GrantedPermissionProfile;
use codex_app_server_protocol::McpServerElicitationAction;
use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::McpServerElicitationRequestResponse;
use codex_app_server_protocol::PermissionGrantScope;
use codex_app_server_protocol::PermissionsRequestApprovalResponse;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ToolRequestUserInputAnswer;
use codex_app_server_protocol::ToolRequestUserInputResponse;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;

use crate::PendingRequest;
use crate::PendingRequestResponse;
use crate::RequestResolution;
use crate::SlashInvocation;
use crate::SurfaceActivity;
use crate::SurfaceState;
use crate::ThreadPickerAction;
use crate::permission_picker::PermissionPickerInput;
use crate::permission_picker::PermissionSelection;
use crate::permission_picker::handle_key as handle_permission_picker_key;
use crate::theme_picker::ThemePickerInput;
use crate::theme_picker::handle_key as handle_theme_picker_key;
use crate::thread_picker::PickerInput;
use crate::thread_picker::handle_key as handle_thread_picker_key;

#[derive(Debug, Clone, PartialEq)]
pub enum InputAction {
    None,
    Redraw,
    Submit(String),
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
    state.composer_mut().push_str(text);
    state.refresh_slash();
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
            InputAction::Redraw
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
                state.composer_mut().clear();
                state.refresh_slash();
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
            let prompt = state.take_composer();
            if prompt.trim().is_empty() {
                InputAction::None
            } else {
                InputAction::Submit(prompt)
            }
        }
        (KeyCode::Enter, _) => {
            state.composer_mut().push('\n');
            state.refresh_slash();
            InputAction::Redraw
        }
        (KeyCode::Backspace, _) => {
            state.composer_mut().pop();
            state.refresh_slash();
            InputAction::Redraw
        }
        (KeyCode::Char(character), modifiers)
            if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) =>
        {
            state.composer_mut().push(character);
            state.refresh_slash();
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
        PendingRequest::UserInput { .. } => true,
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
            user_input_response(state.composer(), &params, key.code)
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
        return match key.code {
            KeyCode::Backspace => {
                state.composer_mut().pop();
                InputAction::Redraw
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) =>
            {
                state.composer_mut().push(character);
                InputAction::Redraw
            }
            _ => InputAction::None,
        };
    };

    let request_id = request.request_id().clone();
    match state.pending_requests_mut().resolve(&request_id, response) {
        Ok(resolution) => {
            state.composer_mut().clear();
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

fn user_input_response(
    composer: &str,
    params: &codex_app_server_protocol::ToolRequestUserInputParams,
    key: KeyCode,
) -> Option<PendingRequestResponse> {
    if key == KeyCode::Esc {
        return Some(PendingRequestResponse::Reject {
            code: -32000,
            message: "user input cancelled".to_string(),
        });
    }
    if key != KeyCode::Enter || composer.trim().is_empty() {
        return None;
    }
    let values = composer.split('|').map(str::trim).collect::<Vec<_>>();
    let answers = params
        .questions
        .iter()
        .enumerate()
        .map(|(index, question)| {
            let value = values
                .get(index)
                .or_else(|| values.last())
                .copied()
                .unwrap_or_default();
            (
                question.id.clone(),
                ToolRequestUserInputAnswer {
                    answers: vec![value.to_string()],
                },
            )
        })
        .collect::<HashMap<_, _>>();
    Some(PendingRequestResponse::UserInput(
        ToolRequestUserInputResponse { answers },
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
