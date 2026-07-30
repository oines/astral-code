//! Terminal-specific recovery for modifier flags dropped by the PTY.

use codex_terminal_detection::TerminalName;
use codex_terminal_detection::terminal_info;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use crate::macos_modifiers;
use crate::macos_modifiers::ModifierState;

#[derive(Clone, Copy, Debug, Default)]
struct DeletionModifierRescue {
    command: bool,
    option: bool,
}

pub(crate) fn normalize_key(key: KeyEvent) -> KeyEvent {
    if !key.modifiers.is_empty() || !matches!(key.code, KeyCode::Backspace | KeyCode::Delete) {
        return key;
    }
    let terminal = terminal_info().name;
    let rescue = deletion_modifier_rescue(terminal);
    let needs_os_probe = rescue.command || rescue.option;
    let modifiers = if needs_os_probe {
        macos_modifiers::snapshot()
    } else {
        ModifierState::default()
    };
    normalize_key_for(key, terminal, modifiers)
}

pub(super) fn normalize_key_for(
    mut key: KeyEvent,
    terminal: TerminalName,
    modifiers: ModifierState,
) -> KeyEvent {
    if !key.modifiers.is_empty() || !matches!(key.code, KeyCode::Backspace | KeyCode::Delete) {
        return key;
    }
    let rescue = deletion_modifier_rescue(terminal);
    let rescued = if modifiers.command && rescue.command {
        KeyModifiers::SUPER
    } else if modifiers.option && rescue.option {
        KeyModifiers::ALT
    } else {
        return key;
    };
    key.modifiers |= rescued;
    key
}

pub(super) fn is_modified_enter(key: &KeyEvent) -> bool {
    if key.code != KeyCode::Enter {
        return false;
    }
    if key
        .modifiers
        .intersects(KeyModifiers::ALT | KeyModifiers::SHIFT)
    {
        return true;
    }
    let terminal = terminal_info().name;
    let modifiers = if terminal == TerminalName::AppleTerminal {
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

fn deletion_modifier_rescue(terminal: TerminalName) -> DeletionModifierRescue {
    match terminal {
        TerminalName::WezTerm => DeletionModifierRescue {
            command: true,
            option: false,
        },
        TerminalName::Alacritty | TerminalName::AppleTerminal => DeletionModifierRescue {
            command: false,
            option: true,
        },
        TerminalName::WarpTerminal => DeletionModifierRescue {
            command: true,
            option: true,
        },
        TerminalName::Ghostty
        | TerminalName::Iterm2
        | TerminalName::VsCode
        | TerminalName::Kitty
        | TerminalName::Konsole
        | TerminalName::GnomeTerminal
        | TerminalName::Vte
        | TerminalName::WindowsTerminal
        | TerminalName::Dumb
        | TerminalName::Unknown => DeletionModifierRescue::default(),
    }
}
