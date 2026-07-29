use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;

use crate::InputAction;
use crate::PromptSubmission;
use crate::SlashInvocation;
use crate::SurfaceState;
use crate::command_palette::CommandPaletteCommand;
use crate::command_palette::CommandPaletteState;
use crate::modal::ModalPointerAction;

pub(super) fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    match (key.code, key.modifiers) {
        (KeyCode::Esc, _) => {
            state.close_command_palette();
            InputAction::Redraw
        }
        (KeyCode::Up, _) => {
            move_selection(state, -1);
            InputAction::Redraw
        }
        (KeyCode::Down, _) => {
            move_selection(state, 1);
            InputAction::Redraw
        }
        (KeyCode::PageUp, _) => {
            move_selection(state, -10);
            InputAction::Redraw
        }
        (KeyCode::PageDown, _) => {
            move_selection(state, 10);
            InputAction::Redraw
        }
        (KeyCode::Home, _) => {
            if let Some(palette) = state.command_palette_mut() {
                palette.select_start();
            }
            InputAction::Redraw
        }
        (KeyCode::End, _) => {
            if let Some(palette) = state.command_palette_mut() {
                palette.select_end();
            }
            InputAction::Redraw
        }
        (KeyCode::Enter, KeyModifiers::NONE) => activate_selected(state),
        (KeyCode::Backspace, _) => {
            if let Some(palette) = state.command_palette_mut() {
                palette.backspace_query();
            }
            InputAction::Redraw
        }
        (KeyCode::Char(character), modifiers)
            if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT =>
        {
            if let Some(palette) = state.command_palette_mut() {
                palette.insert_query(character);
            }
            InputAction::Redraw
        }
        _ => InputAction::None,
    }
}

pub(super) fn handle_paste(state: &mut SurfaceState, text: &str) -> InputAction {
    if let Some(palette) = state.command_palette_mut() {
        palette.paste_query(text);
        InputAction::Redraw
    } else {
        InputAction::None
    }
}

pub(super) fn handle_mouse(state: &mut SurfaceState, mouse: MouseEvent) -> InputAction {
    let action = state
        .command_palette_mut()
        .map(|palette| palette.pointer.handle_mouse(mouse))
        .unwrap_or(ModalPointerAction::Ignored);
    match action {
        ModalPointerAction::Ignored => InputAction::None,
        ModalPointerAction::Close => {
            state.close_command_palette();
            InputAction::Redraw
        }
        ModalPointerAction::Scroll(delta) => {
            move_selection(state, delta);
            InputAction::Redraw
        }
        ModalPointerAction::Hover(_) | ModalPointerAction::Redraw => InputAction::Redraw,
        ModalPointerAction::Activate(row) => {
            if let Some(palette) = state.command_palette_mut() {
                palette.select(row);
            }
            activate_selected(state)
        }
    }
}

fn move_selection(state: &mut SurfaceState, delta: isize) {
    if let Some(palette) = state.command_palette_mut() {
        palette.move_selection(delta);
    }
}

fn activate_selected(state: &mut SurfaceState) -> InputAction {
    let command = state
        .command_palette()
        .and_then(CommandPaletteState::selected_command);
    let Some(command) = command else {
        return InputAction::None;
    };
    match command {
        CommandPaletteCommand::CycleMode => {
            state.close_command_palette();
            InputAction::CycleMode
        }
        CommandPaletteCommand::ToggleMultiline => {
            state.close_command_palette();
            InputAction::ToggleMultiline
        }
        CommandPaletteCommand::OpenShortcuts => {
            state.close_command_palette();
            InputAction::OpenShortcuts
        }
        CommandPaletteCommand::ToggleQueue => {
            state.close_command_palette();
            if state.toggle_queue_focus() {
                InputAction::Redraw
            } else {
                InputAction::Notice("No follow-ups queued".to_string())
            }
        }
        CommandPaletteCommand::EditPrompt => {
            state.close_command_palette();
            if state.composer_has_structured_elements() {
                InputAction::Notice(
                    "External editing is unavailable while the draft has structured prompt items"
                        .to_string(),
                )
            } else {
                InputAction::OpenExternalEditor
            }
        }
        CommandPaletteCommand::CopyResponse => {
            state.close_command_palette();
            InputAction::CopyLastResponse
        }
        CommandPaletteCommand::Slash {
            command,
            name,
            insert_text,
            requires_input,
        } => {
            if requires_input {
                state.begin_palette_slash(insert_text);
                InputAction::Redraw
            } else {
                state.close_command_palette();
                state.record_slash(command);
                InputAction::Slash {
                    invocation: SlashInvocation {
                        command,
                        name,
                        args: String::new(),
                    },
                    submission: PromptSubmission::text_only(String::new()),
                }
            }
        }
    }
}
