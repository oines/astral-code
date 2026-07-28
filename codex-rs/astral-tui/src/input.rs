use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::Thread;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;

use crate::PendingRequest;
use crate::PendingRequestResponse;
use crate::PromptSubmission;
use crate::RequestResolution;
use crate::SlashInvocation;
use crate::SurfaceActivity;
use crate::SurfaceState;
use crate::ThreadPickerAction;
use crate::mcp_form::McpFormEvent;
use crate::permission_picker::PermissionPickerInput;
use crate::permission_picker::PermissionSelection;
use crate::permission_picker::handle_key as handle_permission_picker_key;
use crate::request_choice::RequestChoiceEvent;
use crate::request_choice::cancel_response;
use crate::request_choice::is_simple_request;
use crate::request_choice::response_for;
use crate::theme_picker::ThemePickerInput;
use crate::theme_picker::handle_key as handle_theme_picker_key;
use crate::thread_picker::PickerInput;
use crate::thread_picker::handle_key as handle_thread_picker_key;

mod mention_popup;
mod plan_review;
mod scrollback;
mod user_input;

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
    Slash {
        invocation: SlashInvocation,
        submission: PromptSubmission,
    },
    ThreadPickerLoadNext,
    ThreadPickerSelect {
        action: ThreadPickerAction,
        thread: Box<Thread>,
    },
    SelectTheme(String),
    SelectPermission(PermissionSelection),
    Plan(crate::plan_review::PlanReviewAction),
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
        state.sync_request_states();
        if is_simple_request(&request) && state.scrollback_focused() {
            return scrollback::handle_key(state, key);
        }
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
    if state.plan_review().is_some() {
        return plan_review::handle_key(state, key);
    }
    if key.code == KeyCode::Esc && state.clear_scrollback_selection() {
        return InputAction::Redraw;
    }
    if state.scrollback_focused() {
        return scrollback::handle_key(state, key);
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
    let mcp_schema = state
        .pending_requests()
        .front()
        .and_then(|request| match request {
            PendingRequest::McpElicitation { params, .. } => match &params.request {
                McpServerElicitationRequest::Form {
                    requested_schema, ..
                } => Some(requested_schema.clone()),
                McpServerElicitationRequest::Url { .. } => None,
            },
            _ => None,
        });
    if let Some(schema) = mcp_schema {
        return if state.mcp_form_mut().handle_paste(&schema, text) {
            InputAction::Redraw
        } else {
            InputAction::None
        };
    }
    if state.plan_review().is_some() {
        return plan_review::handle_paste(state, text);
    }
    state.composer_state_mut().insert_text(text);
    state.refresh_composer_completions();
    InputAction::Redraw
}

pub(crate) fn handle_mouse(state: &mut SurfaceState, mouse: MouseEvent) -> InputAction {
    if let Some(request) = state.pending_requests().front().cloned()
        && is_simple_request(&request)
    {
        state.sync_request_states();
        let event = state.request_choice_mut().handle_mouse(mouse);
        if matches!(
            mouse.kind,
            crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left)
        ) && event != RequestChoiceEvent::None
        {
            state.focus_prompt();
        }
        return handle_request_choice_event(state, request, event);
    }
    if let Some(request) = state.pending_requests().front().cloned()
        && let PendingRequest::UserInput { params, .. } = &request
    {
        let params = params.clone();
        return user_input::handle_mouse(state, request, &params, mouse);
    }
    if state.plan_review().is_some() {
        return plan_review::handle_mouse(state, mouse);
    }
    InputAction::None
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
    if key.code == KeyCode::Tab && key.modifiers == KeyModifiers::NONE && state.focus_scrollback() {
        return InputAction::Redraw;
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
                        let submission = state.take_submission();
                        state.record_slash(invocation.command);
                        InputAction::Slash {
                            invocation,
                            submission,
                        }
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
    let response = match request.clone() {
        PendingRequest::CommandExecution { .. }
        | PendingRequest::FileChange { .. }
        | PendingRequest::Permissions { .. } => {
            let event = state.request_choice_mut().handle_key(key);
            return handle_request_choice_event(state, request, event);
        }
        PendingRequest::UserInput { params, .. } => {
            return user_input::handle_key(state, request, &params, key);
        }
        PendingRequest::McpElicitation { params, .. } => match &params.request {
            McpServerElicitationRequest::Form {
                requested_schema, ..
            } => match state.mcp_form_mut().handle_key(requested_schema, key) {
                McpFormEvent::None => return InputAction::None,
                McpFormEvent::Redraw => return InputAction::Redraw,
                McpFormEvent::Submit(response) => {
                    Some(PendingRequestResponse::McpElicitation(response))
                }
            },
            McpServerElicitationRequest::Url { .. } => {
                let event = state.request_choice_mut().handle_key(key);
                return handle_request_choice_event(state, request, event);
            }
        },
        PendingRequest::DynamicTool { .. } | PendingRequest::Attestation { .. } => None,
        PendingRequest::LegacyApplyPatch { .. } | PendingRequest::LegacyExecCommand { .. } => {
            Some(PendingRequestResponse::Reject {
                code: -32601,
                message: "Astral TUI accepts app-server v2 requests only".to_string(),
            })
        }
    };

    response.map_or(InputAction::None, |response| {
        resolve_request(state, &request, response)
    })
}

fn handle_request_choice_event(
    state: &mut SurfaceState,
    request: PendingRequest,
    event: RequestChoiceEvent,
) -> InputAction {
    match event {
        RequestChoiceEvent::None => InputAction::None,
        RequestChoiceEvent::Redraw => InputAction::Redraw,
        RequestChoiceEvent::FocusScrollback => {
            if state.focus_scrollback() {
                InputAction::Redraw
            } else {
                InputAction::None
            }
        }
        RequestChoiceEvent::Activate(choice) => response_for(&request, choice)
            .map_or(InputAction::None, |response| {
                resolve_request(state, &request, response)
            }),
        RequestChoiceEvent::Cancel => cancel_response(&request)
            .map_or(InputAction::None, |response| {
                resolve_request(state, &request, response)
            }),
    }
}

fn resolve_request(
    state: &SurfaceState,
    request: &PendingRequest,
    response: PendingRequestResponse,
) -> InputAction {
    let request_id = request.request_id().clone();
    match state
        .pending_requests()
        .prepare_resolution(&request_id, response)
    {
        Ok(resolution) => InputAction::Resolve(resolution),
        Err(error) => InputAction::Notice(error.to_string()),
    }
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "input_editor_tests.rs"]
mod editor_tests;
