use crossterm::event::KeyEvent;
use crossterm::event::MouseEvent;

use super::InputAction;
use crate::SurfaceState;
use crate::settings::SettingsInput;

pub(super) fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    let input = state
        .settings_mut()
        .map(|settings| crate::settings::handle_key(settings, key))
        .unwrap_or(SettingsInput::None);
    apply_input(state, input)
}

pub(super) fn handle_paste(state: &mut SurfaceState, text: &str) -> InputAction {
    let input = state
        .settings_mut()
        .map(|settings| crate::settings::handle_paste(settings, text))
        .unwrap_or(SettingsInput::None);
    apply_input(state, input)
}

pub(super) fn handle_mouse(state: &mut SurfaceState, mouse: MouseEvent) -> InputAction {
    let input = state
        .settings_mut()
        .map(|settings| crate::settings::handle_mouse(settings, mouse))
        .unwrap_or(SettingsInput::None);
    apply_input(state, input)
}

fn apply_input(state: &mut SurfaceState, input: SettingsInput) -> InputAction {
    match input {
        SettingsInput::None => InputAction::None,
        SettingsInput::Redraw => InputAction::Redraw,
        SettingsInput::Close => {
            state.close_settings();
            InputAction::Redraw
        }
        SettingsInput::Write {
            write,
            selected_theme,
        } => InputAction::SettingsConfigWrite {
            focus: write.focus.token(),
            params: write.params,
            selected_theme: selected_theme.map(|theme| theme.config_name().to_string()),
        },
        SettingsInput::PreviewTheme(theme) => {
            state.set_theme(theme);
            InputAction::Redraw
        }
        SettingsInput::RestoreTheme(theme) => {
            state.set_theme(theme);
            InputAction::Redraw
        }
        SettingsInput::Notice(message) => {
            if let Some(settings) = state.settings_mut() {
                settings.set_notice(message);
            }
            InputAction::Redraw
        }
    }
}
