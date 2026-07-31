use crossterm::event::KeyEvent;
use crossterm::event::MouseEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::modal::ModalPointerAction;
use crate::view::AstralTheme;

use super::BACK_ROW_ID;
use super::SettingsInput;
use super::SettingsPage;
use super::SettingsState;

pub(super) mod models;
pub(super) mod search;
mod search_editor;
pub(super) mod search_render;
mod search_write;
pub(super) mod session_memory;
mod session_memory_config;
pub(super) mod session_memory_render;

pub(super) use search::SearchPageState;
pub(super) use session_memory::SessionMemoryPageState;

pub(super) fn handle_key(state: &mut SettingsState, key: KeyEvent) -> SettingsInput {
    state.clear_notice();
    let store = state.store.clone();
    let input = match state.page {
        SettingsPage::Search => state.search.handle_key(&store, key),
        SettingsPage::SessionMemoryTemplates => state.session_memory.handle_key(&store, key),
        SettingsPage::Root | SettingsPage::Category(_) | SettingsPage::Models => {
            return SettingsInput::None;
        }
    };
    if input == SettingsInput::Close {
        state.request_back()
    } else {
        input
    }
}

pub(super) fn handle_paste(state: &mut SettingsState, text: &str) -> SettingsInput {
    match state.page {
        SettingsPage::Search => state.search.handle_paste(text),
        SettingsPage::SessionMemoryTemplates => state.session_memory.handle_paste(text),
        SettingsPage::Root | SettingsPage::Category(_) | SettingsPage::Models => {
            SettingsInput::None
        }
    }
}

pub(super) fn handle_mouse(state: &mut SettingsState, mouse: MouseEvent) -> SettingsInput {
    match state.pointer.handle_mouse(mouse) {
        ModalPointerAction::Ignored => SettingsInput::None,
        ModalPointerAction::Redraw | ModalPointerAction::Hover(None) => SettingsInput::Redraw,
        ModalPointerAction::Close => {
            state.clear_notice();
            let cancelled = match state.page {
                SettingsPage::Search => state.search.cancel_editor(),
                SettingsPage::SessionMemoryTemplates => state.session_memory.cancel_editor(),
                SettingsPage::Root | SettingsPage::Category(_) | SettingsPage::Models => false,
            };
            if cancelled {
                SettingsInput::Redraw
            } else {
                state.request_close()
            }
        }
        ModalPointerAction::Hover(Some(_)) => SettingsInput::Redraw,
        ModalPointerAction::Activate(BACK_ROW_ID) => {
            state.clear_notice();
            state.request_back()
        }
        ModalPointerAction::Activate(index) => {
            state.clear_notice();
            let store = state.store.clone();
            match state.page {
                SettingsPage::Search => state.search.activate_row(&store, index),
                SettingsPage::SessionMemoryTemplates => {
                    state.session_memory.activate_row(&store, index)
                }
                SettingsPage::Root | SettingsPage::Category(_) | SettingsPage::Models => {
                    SettingsInput::None
                }
            }
        }
        ModalPointerAction::Scroll(delta) => {
            state.clear_notice();
            match state.page {
                SettingsPage::Search => state.search.handle_scroll(delta),
                SettingsPage::SessionMemoryTemplates => {
                    state.session_memory.handle_scroll(delta);
                }
                SettingsPage::Root | SettingsPage::Category(_) | SettingsPage::Models => {}
            }
            SettingsInput::Redraw
        }
    }
}

pub(super) fn render(
    state: &mut SettingsState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    match state.page {
        SettingsPage::Search => search_render::render(state, area, buffer, theme),
        SettingsPage::SessionMemoryTemplates => {
            session_memory_render::render(state, area, buffer, theme);
        }
        SettingsPage::Root | SettingsPage::Category(_) | SettingsPage::Models => {}
    }
}
