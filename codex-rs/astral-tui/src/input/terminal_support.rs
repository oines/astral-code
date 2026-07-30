//! Terminal-specific recovery for modified Enter.

use codex_terminal_detection::TerminalName;
use codex_terminal_detection::terminal_info;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use crate::macos_modifiers;
use crate::macos_modifiers::ModifierState;

pub(super) fn is_modified_enter(key: &KeyEvent) -> bool {
    let terminal = terminal_info().name;
    let needs_os_probe = key.code == KeyCode::Enter
        && terminal == TerminalName::AppleTerminal
        && !key
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::SHIFT);
    let modifiers = if needs_os_probe {
        macos_modifiers::snapshot()
    } else {
        ModifierState::default()
    };
    is_modified_enter_for(key, terminal, modifiers)
}

pub(super) fn is_modified_enter_for(
    key: &KeyEvent,
    terminal: TerminalName,
    modifiers: ModifierState,
) -> bool {
    key.code == KeyCode::Enter
        && (key
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::SHIFT)
            || (terminal == TerminalName::AppleTerminal && modifiers.any_newline_modifier()))
}
