use codex_app_server_protocol::McpElicitationSchema;
use crossterm::event::KeyEvent;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;

use super::InputAction;
use super::resolve_request;
use crate::PendingRequest;
use crate::PendingRequestResponse;
use crate::SurfaceState;
use crate::mcp_form::McpFormEvent;

pub(super) fn handle_key(
    state: &mut SurfaceState,
    request: PendingRequest,
    schema: &McpElicitationSchema,
    key: KeyEvent,
) -> InputAction {
    let event = state.mcp_form_mut().handle_key(schema, key);
    finish(state, request, event)
}

pub(super) fn handle_mouse(
    state: &mut SurfaceState,
    request: PendingRequest,
    schema: &McpElicitationSchema,
    mouse: MouseEvent,
) -> InputAction {
    state.sync_request_states();
    let event = state.mcp_form_mut().handle_mouse(schema, mouse);
    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        state.focus_prompt();
    }
    finish(state, request, event)
}

fn finish(state: &mut SurfaceState, request: PendingRequest, event: McpFormEvent) -> InputAction {
    match event {
        McpFormEvent::None => InputAction::None,
        McpFormEvent::Redraw => InputAction::Redraw,
        McpFormEvent::Submit(response) => resolve_request(
            state,
            &request,
            PendingRequestResponse::McpElicitation(response),
        ),
    }
}
