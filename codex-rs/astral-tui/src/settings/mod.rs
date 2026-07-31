//! Unified Astral settings surface.
//!
//! Settings reads the effective app-server configuration, but every mutation
//! targets only the base user config layer. Project and managed layers remain
//! visible as provenance and are never edited by the TUI.

mod input;
mod input_editor;
mod pages;
mod registry;
mod render;
mod render_editor;
mod render_row;
mod state;
mod state_actions;
mod state_rows;
mod store;

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;

use crate::view::AstralThemeId;

pub(crate) use input::handle_key;
pub(crate) use input::handle_mouse;
pub(crate) use input::handle_paste;
pub(crate) use registry::Category;
pub(crate) use registry::SettingDefinition;
pub(crate) use registry::SettingKind;
pub(crate) use registry::SettingOption;
pub(crate) use registry::Subpage;
pub(crate) use registry::categories;
pub(crate) use registry::definitions;
pub(crate) use render::render;
pub(crate) use state::SettingsConfirmAction;
pub(crate) use state::SettingsEditor;
pub(crate) use state::SettingsPage;
pub(crate) use state::SettingsRow;
pub(crate) use state::SettingsState;
pub(crate) use store::SettingsData;
pub(crate) use store::SettingsFocus;
pub(crate) use store::SettingsStore;
pub(crate) use store::SettingsWrite;

pub(super) const SEARCH_ROW_ID: usize = usize::MAX;
pub(super) const BACK_ROW_ID: usize = usize::MAX - 1;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SettingsInput {
    None,
    Redraw,
    Close,
    Write {
        write: SettingsWrite,
        selected_theme: Option<AstralThemeId>,
    },
    PreviewTheme(AstralThemeId),
    RestoreTheme(AstralThemeId),
    Notice(String),
}
