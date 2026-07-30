use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;

use super::InputAction;
use crate::PromptSubmission;
use crate::SlashCommandId;
use crate::SlashInvocation;
use crate::SurfaceState;
use crate::modal::ModalPointerAction;
use crate::model_picker::ModelPickerInput;
use crate::model_picker::handle_key as handle_model_key;
use crate::model_picker::handle_mouse as handle_model_mouse_event;
use crate::model_picker::handle_paste as handle_model_paste;
use crate::permission_picker::PermissionPickerInput;
use crate::permission_picker::handle_key as handle_permission_key;
use crate::permission_picker::handle_mouse as handle_permission_mouse_event;
use crate::theme_picker::ThemePickerInput;
use crate::theme_picker::ThemePickerState;
use crate::theme_picker::handle_key as handle_theme_key;
use crate::theme_picker::handle_mouse as handle_theme_mouse_event;
use crate::thread_picker::PickerInput;
use crate::thread_picker::PickerState;
use crate::thread_picker::handle_key as handle_thread_key;
use crate::thread_picker::handle_mouse as handle_thread_mouse_event;

pub(super) fn handle_info_modal_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    if key.code == KeyCode::Esc
        || (key.code == KeyCode::Char('.') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        state.close_modal();
        return InputAction::Redraw;
    }
    if key.code == KeyCode::Enter {
        let target = state.modal().and_then(|modal| modal.open_target.clone());
        if let Some(target) = target {
            state.close_modal();
            return InputAction::OpenLink(target);
        }
    }
    let Some(modal) = state.modal_mut() else {
        return InputAction::None;
    };
    match key.code {
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
    }
}

pub(super) fn handle_theme_picker_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    let original = state.theme_picker().map(ThemePickerState::original);
    let input = state
        .theme_picker_mut()
        .map(|picker| handle_theme_key(picker, key))
        .unwrap_or(ThemePickerInput::None);
    apply_theme_input(state, original, input)
}

pub(super) fn handle_theme_picker_mouse(
    state: &mut SurfaceState,
    mouse: MouseEvent,
) -> InputAction {
    let original = state.theme_picker().map(ThemePickerState::original);
    let input = state
        .theme_picker_mut()
        .map(|picker| handle_theme_mouse_event(picker, mouse))
        .unwrap_or(ThemePickerInput::None);
    apply_theme_input(state, original, input)
}

fn apply_theme_input(
    state: &mut SurfaceState,
    original: Option<crate::view::AstralThemeId>,
    input: ThemePickerInput,
) -> InputAction {
    match input {
        ThemePickerInput::None => InputAction::None,
        ThemePickerInput::Redraw => InputAction::Redraw,
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

pub(super) fn handle_permission_picker_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    let input = state
        .permission_picker_mut()
        .map(|picker| handle_permission_key(picker, key))
        .unwrap_or(PermissionPickerInput::None);
    apply_permission_input(state, input)
}

pub(super) fn handle_permission_picker_mouse(
    state: &mut SurfaceState,
    mouse: MouseEvent,
) -> InputAction {
    let input = state
        .permission_picker_mut()
        .map(|picker| handle_permission_mouse_event(picker, mouse))
        .unwrap_or(PermissionPickerInput::None);
    apply_permission_input(state, input)
}

fn apply_permission_input(state: &mut SurfaceState, input: PermissionPickerInput) -> InputAction {
    match input {
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

pub(super) fn handle_thread_picker_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    let input = state
        .thread_picker_mut()
        .map(|picker| handle_thread_key(picker, key, /*terminal_height*/ 24))
        .unwrap_or(PickerInput::None);
    apply_thread_input(state, input)
}

pub(super) fn handle_thread_picker_mouse(
    state: &mut SurfaceState,
    mouse: MouseEvent,
) -> InputAction {
    let input = state
        .thread_picker_mut()
        .map(|picker| handle_thread_mouse_event(picker, mouse))
        .unwrap_or(PickerInput::None);
    apply_thread_input(state, input)
}

fn apply_thread_input(state: &mut SurfaceState, input: PickerInput) -> InputAction {
    match input {
        PickerInput::None => InputAction::None,
        PickerInput::Redraw => InputAction::Redraw,
        PickerInput::LoadNext => InputAction::ThreadPickerLoadNext,
        PickerInput::Select(thread) => {
            let Some(action) = state.thread_picker().map(PickerState::action) else {
                return InputAction::None;
            };
            state.close_thread_picker();
            InputAction::ThreadPickerSelect { action, thread }
        }
        PickerInput::Cancel => {
            state.close_thread_picker();
            InputAction::Redraw
        }
    }
}

pub(super) fn handle_model_picker_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    let input = state
        .model_picker_mut()
        .map(|picker| handle_model_key(picker, key))
        .unwrap_or(ModelPickerInput::None);
    apply_model_input(state, input)
}

pub(super) fn handle_model_picker_paste(state: &mut SurfaceState, text: &str) -> InputAction {
    let input = state
        .model_picker_mut()
        .map(|picker| handle_model_paste(picker, text))
        .unwrap_or(ModelPickerInput::None);
    apply_model_input(state, input)
}

pub(super) fn handle_model_picker_mouse(
    state: &mut SurfaceState,
    mouse: MouseEvent,
) -> InputAction {
    let input = state
        .model_picker_mut()
        .map(|picker| handle_model_mouse_event(picker, mouse))
        .unwrap_or(ModelPickerInput::None);
    apply_model_input(state, input)
}

fn apply_model_input(state: &mut SurfaceState, input: ModelPickerInput) -> InputAction {
    match input {
        ModelPickerInput::None => InputAction::None,
        ModelPickerInput::Redraw => InputAction::Redraw,
        ModelPickerInput::Select(args) => {
            state.close_model_picker();
            state.record_slash(SlashCommandId::Model);
            InputAction::Slash {
                invocation: SlashInvocation {
                    command: SlashCommandId::Model,
                    name: "model",
                    args,
                },
                submission: PromptSubmission::text_only(String::new()),
            }
        }
        ModelPickerInput::Cancel => {
            state.close_model_picker();
            InputAction::Redraw
        }
    }
}

pub(super) fn handle_info_modal_mouse(state: &mut SurfaceState, mouse: MouseEvent) -> InputAction {
    let action = state
        .modal_mut()
        .map(|modal| modal.pointer.handle_mouse(mouse))
        .unwrap_or(ModalPointerAction::Ignored);
    match action {
        ModalPointerAction::Ignored => InputAction::None,
        ModalPointerAction::Close => {
            state.close_modal();
            InputAction::Redraw
        }
        ModalPointerAction::Scroll(delta) => {
            if let Some(modal) = state.modal_mut() {
                modal.scroll_by(delta);
            }
            InputAction::Redraw
        }
        ModalPointerAction::Redraw
        | ModalPointerAction::Hover(_)
        | ModalPointerAction::Activate(_) => InputAction::Redraw,
    }
}
