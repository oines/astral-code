use codex_app_server_protocol::ToolRequestUserInputParams;
use crossterm::event::KeyEvent;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;

use super::InputAction;
use super::resolve_request;
use crate::PendingRequest;
use crate::PendingRequestResponse;
use crate::SurfaceState;
use crate::request_user_input::RequestUserInputEvent;

pub(super) fn handle_key(
    state: &mut SurfaceState,
    request: PendingRequest,
    params: &ToolRequestUserInputParams,
    key: KeyEvent,
) -> InputAction {
    let event = state.request_user_input_mut().handle_key(params, key);
    finish(state, request, event)
}

pub(super) fn handle_mouse(
    state: &mut SurfaceState,
    request: PendingRequest,
    params: &ToolRequestUserInputParams,
    mouse: MouseEvent,
) -> InputAction {
    state.sync_request_states();
    let event = state.request_user_input_mut().handle_mouse(params, mouse);
    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        state.focus_prompt();
    }
    finish(state, request, event)
}

fn finish(
    state: &mut SurfaceState,
    request: PendingRequest,
    event: RequestUserInputEvent,
) -> InputAction {
    let response = match event {
        RequestUserInputEvent::None => return InputAction::None,
        RequestUserInputEvent::Redraw => return InputAction::Redraw,
        RequestUserInputEvent::Submit(response) => PendingRequestResponse::UserInput(response),
        RequestUserInputEvent::Cancel => PendingRequestResponse::Reject {
            code: -32000,
            message: "user input cancelled".to_string(),
        },
    };
    resolve_request(state, &request, response)
}
