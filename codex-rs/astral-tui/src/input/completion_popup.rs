use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;

use crate::InputAction;
use crate::SurfaceState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionKind {
    Slash,
    Mention,
}

pub(super) fn handle_mouse(state: &mut SurfaceState, mouse: MouseEvent) -> InputAction {
    let Some((kind, total_items)) = active_completion(state) else {
        return InputAction::None;
    };
    match mouse.kind {
        MouseEventKind::Moved => {
            if state.update_completion_hover(mouse) {
                InputAction::Redraw
            } else {
                InputAction::None
            }
        }
        MouseEventKind::ScrollUp if state.completion_contains(mouse) => {
            move_selection(state, kind, -1);
            InputAction::Redraw
        }
        MouseEventKind::ScrollDown if state.completion_contains(mouse) => {
            move_selection(state, kind, 1);
            InputAction::Redraw
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(target) = state.completion_scrollbar_target(mouse, total_items) {
                select(state, kind, target);
                state.set_completion_scrollbar_dragging(true);
                return InputAction::Redraw;
            }
            if let Some(row) = state.completion_row_at(mouse) {
                select(state, kind, row);
                state.focus_prompt();
                accept(state, kind);
                return InputAction::Redraw;
            }
            if state.prompt_contains(mouse) {
                state.focus_prompt();
                state.place_composer_cursor(mouse);
                return InputAction::Redraw;
            }
            InputAction::None
        }
        MouseEventKind::Drag(MouseButton::Left) if state.completion_scrollbar_dragging() => {
            if let Some(target) = state.completion_scrollbar_target(mouse, total_items) {
                select(state, kind, target);
            }
            InputAction::Redraw
        }
        MouseEventKind::Up(MouseButton::Left) if state.completion_scrollbar_dragging() => {
            state.set_completion_scrollbar_dragging(false);
            InputAction::Redraw
        }
        MouseEventKind::Down(MouseButton::Right | MouseButton::Middle)
        | MouseEventKind::Up(MouseButton::Left | MouseButton::Right | MouseButton::Middle)
        | MouseEventKind::Drag(MouseButton::Left | MouseButton::Right | MouseButton::Middle)
        | MouseEventKind::ScrollLeft
        | MouseEventKind::ScrollRight
        | MouseEventKind::ScrollUp
        | MouseEventKind::ScrollDown => InputAction::None,
    }
}

fn active_completion(state: &SurfaceState) -> Option<(CompletionKind, usize)> {
    if state.mentions().open {
        Some((CompletionKind::Mention, state.mentions().matches.len()))
    } else if state.slash().open {
        Some((CompletionKind::Slash, state.slash().matches.len()))
    } else {
        None
    }
}

fn move_selection(state: &mut SurfaceState, kind: CompletionKind, delta: isize) {
    match kind {
        CompletionKind::Slash => state.move_slash_selection(delta),
        CompletionKind::Mention => state.move_mention_selection(delta),
    }
}

fn select(state: &mut SurfaceState, kind: CompletionKind, index: usize) {
    match kind {
        CompletionKind::Slash => state.select_slash(index),
        CompletionKind::Mention => state.select_mention(index),
    }
}

fn accept(state: &mut SurfaceState, kind: CompletionKind) {
    match kind {
        CompletionKind::Slash => {
            state.accept_slash_selection();
        }
        CompletionKind::Mention => {
            state.accept_mention_selection();
        }
    }
}
