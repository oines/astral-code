use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;

use crate::InputAction;
use crate::SurfaceState;
use crate::actions;
use crate::actions::ActionId;
use crate::view::QueuePaneAction;

pub(super) fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    if actions::matches(ActionId::InterjectPrompt, &key) {
        return state
            .selected_follow_up_id()
            .map_or(InputAction::None, |id| InputAction::SteerQueuedPrompt {
                id,
            });
    }
    match key {
        KeyEvent {
            code: KeyCode::Esc | KeyCode::Tab,
            modifiers: KeyModifiers::NONE,
            ..
        } => {
            state.blur_queue();
            InputAction::Redraw
        }
        KeyEvent {
            code: KeyCode::Up | KeyCode::Char('k'),
            modifiers: KeyModifiers::NONE,
            ..
        }
        | KeyEvent {
            code: KeyCode::Char('p'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => {
            state.move_queue_selection(-1);
            InputAction::Redraw
        }
        KeyEvent {
            code: KeyCode::Down | KeyCode::Char('j'),
            modifiers: KeyModifiers::NONE,
            ..
        }
        | KeyEvent {
            code: KeyCode::Char('n'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => {
            state.move_queue_selection(1);
            InputAction::Redraw
        }
        KeyEvent {
            code: KeyCode::Char('e') | KeyCode::Enter,
            modifiers: KeyModifiers::NONE,
            ..
        } => {
            if state.begin_queue_edit() {
                InputAction::Redraw
            } else {
                InputAction::None
            }
        }
        KeyEvent {
            code: KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace,
            modifiers: KeyModifiers::NONE,
            ..
        } => {
            if state.remove_selected_follow_up() {
                InputAction::DrainQueue
            } else {
                InputAction::None
            }
        }
        KeyEvent {
            code: KeyCode::Char('J'),
            modifiers,
            ..
        } if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT => {
            state.reorder_selected_follow_up(1);
            InputAction::Redraw
        }
        KeyEvent {
            code: KeyCode::Char('K'),
            modifiers,
            ..
        } if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT => {
            state.reorder_selected_follow_up(-1);
            InputAction::Redraw
        }
        KeyEvent {
            code: KeyCode::Char('y'),
            modifiers: KeyModifiers::NONE,
            ..
        } => state
            .selected_follow_up_text()
            .map_or(InputAction::None, |text| InputAction::CopyText {
                text: text.to_string(),
                notice: "Copied queued follow-up".to_string(),
            }),
        _ => InputAction::None,
    }
}

pub(super) fn handle_mouse(state: &mut SurfaceState, mouse: MouseEvent) -> InputAction {
    match mouse.kind {
        MouseEventKind::Moved => {
            if state.update_queue_hover(mouse) {
                InputAction::Redraw
            } else {
                InputAction::None
            }
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            let Some(hit) = state.queue_hit(mouse) else {
                return InputAction::None;
            };
            state.select_follow_up(hit.id);
            let delta = if mouse.kind == MouseEventKind::ScrollUp {
                -1
            } else {
                1
            };
            state.move_queue_selection(delta);
            InputAction::Redraw
        }
        MouseEventKind::Down(MouseButton::Left) if !state.queue_editing() => {
            let Some(hit) = state.queue_hit(mouse) else {
                return InputAction::None;
            };
            state.select_follow_up(hit.id);
            match hit.action {
                Some(QueuePaneAction::SendNow) => InputAction::SteerQueuedPrompt { id: hit.id },
                Some(QueuePaneAction::Edit) => {
                    if state.begin_queue_edit() {
                        InputAction::Redraw
                    } else {
                        InputAction::None
                    }
                }
                Some(QueuePaneAction::Delete) => {
                    if state.remove_follow_up(hit.id).is_some() {
                        InputAction::DrainQueue
                    } else {
                        InputAction::None
                    }
                }
                None => InputAction::Redraw,
            }
        }
        MouseEventKind::Down(MouseButton::Left)
        | MouseEventKind::Down(MouseButton::Right | MouseButton::Middle)
        | MouseEventKind::Up(_)
        | MouseEventKind::Drag(_)
        | MouseEventKind::ScrollLeft
        | MouseEventKind::ScrollRight => InputAction::None,
    }
}
