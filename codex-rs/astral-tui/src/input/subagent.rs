//! Input ownership for Grok-style read-only child-thread views.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;

use crate::InputAction;
use crate::SurfaceState;
use crate::modal::ModalPointerAction;
use crate::view::ScrollbackMouseAction;

pub(super) fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    let child_has_overlay = state
        .subagent_surface_mut()
        .is_some_and(|child| child.active_overlay().is_some());
    if child_has_overlay {
        let Some(child) = state.subagent_surface_mut() else {
            return InputAction::None;
        };
        let action = super::handle_key(child, key);
        return normalize_child_action(child, action);
    }
    if key.code == KeyCode::Esc
        || (key.code == KeyCode::Char('q') && key.modifiers == KeyModifiers::NONE)
    {
        state.close_subagent_view();
        return InputAction::Redraw;
    }
    let Some(child) = state.subagent_surface_mut() else {
        return InputAction::None;
    };
    child.focus_scrollback();
    let action = super::scrollback::handle_key(child, key);
    normalize_child_action(child, action)
}

pub(super) fn handle_paste(_state: &mut SurfaceState, _text: &str) -> InputAction {
    InputAction::None
}

pub(super) fn handle_mouse(state: &mut SurfaceState, mouse: MouseEvent) -> InputAction {
    let frame_action = state.handle_subagent_frame_mouse(mouse);
    if frame_action == ModalPointerAction::Close {
        state.close_subagent_view();
        return InputAction::Redraw;
    }

    let Some(child) = state.subagent_surface_mut() else {
        return InputAction::None;
    };
    let action = super::handle_mouse(child, mouse);
    if action != InputAction::None {
        return normalize_child_action(child, action);
    }
    if !child.scrollback_contains(mouse) {
        return if matches!(
            frame_action,
            ModalPointerAction::Redraw | ModalPointerAction::Hover(_)
        ) {
            InputAction::Redraw
        } else {
            InputAction::None
        };
    }
    child.focus_scrollback();
    match child.handle_scrollback_mouse(mouse) {
        ScrollbackMouseAction::Ignored => InputAction::Redraw,
        ScrollbackMouseAction::ActivateEntry(entry_id) => {
            if let Some(thread_id) = child.subagent_thread_id_for_entry(&entry_id) {
                InputAction::OpenSubagent { thread_id }
            } else if child.open_entry(entry_id) {
                InputAction::Redraw
            } else {
                InputAction::None
            }
        }
        ScrollbackMouseAction::Copy(text) => InputAction::CopyText {
            text,
            notice: "Copied selection".to_string(),
        },
        ScrollbackMouseAction::Open(target) => InputAction::OpenLink(target),
    }
}

fn normalize_child_action(child: &mut SurfaceState, action: InputAction) -> InputAction {
    match action {
        InputAction::None
        | InputAction::Redraw
        | InputAction::CopyText { .. }
        | InputAction::OpenLink(_)
        | InputAction::OpenSubagent { .. } => action,
        InputAction::ScrollUp => {
            child.page_up();
            InputAction::Redraw
        }
        InputAction::ScrollDown => {
            child.page_down();
            InputAction::Redraw
        }
        InputAction::OpenShortcuts => {
            child.open_shortcut_help();
            InputAction::Redraw
        }
        InputAction::Notice(message) => {
            child.set_notice(message);
            InputAction::Redraw
        }
        InputAction::CopyLastResponse => {
            child
                .last_agent_response()
                .map_or(InputAction::None, |text| InputAction::CopyText {
                    text: text.to_string(),
                    notice: "Copied last subagent response".to_string(),
                })
        }
        InputAction::Submit(_)
        | InputAction::Interrupt
        | InputAction::Exit
        | InputAction::Slash { .. }
        | InputAction::ThreadPickerLoadNext
        | InputAction::ThreadPickerSelect { .. }
        | InputAction::SelectTheme(_)
        | InputAction::SelectPermission(_)
        | InputAction::Plan(_)
        | InputAction::CycleMode
        | InputAction::Resolve(_) => {
            child.set_notice("Subagent view is read-only");
            InputAction::Redraw
        }
    }
}
