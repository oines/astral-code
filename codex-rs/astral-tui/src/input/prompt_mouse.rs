use std::time::Instant;

use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;

use super::InputAction;
use crate::SurfaceState;
use crate::composer::ComposerMouseAction;

pub(super) fn handle(state: &mut SurfaceState, mouse: MouseEvent) -> InputAction {
    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        state.focus_prompt();
    }
    match state.handle_composer_mouse(mouse, Instant::now()) {
        ComposerMouseAction::Nothing => InputAction::None,
        ComposerMouseAction::Redraw => InputAction::Redraw,
        ComposerMouseAction::Copy(text) => InputAction::CopyText {
            text,
            notice: "Copied prompt selection".to_string(),
        },
        ComposerMouseAction::OpenImage(image) => {
            state.open_local_image(image);
            InputAction::Redraw
        }
    }
}
