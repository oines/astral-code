//! Canonical key representation for Astral TUI actions.
//!
//! This keeps case normalization and terminal-specific Shift+Tab encodings in
//! one place so input dispatch and visible shortcut descriptions cannot drift.

use std::fmt;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct KeyShortcut {
    code: KeyCode,
    modifiers: KeyModifiers,
    modifier_match: ModifierMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ModifierMatch {
    Exact,
    Contains,
    Any,
}

impl KeyShortcut {
    pub(super) fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self {
            code,
            modifiers,
            modifier_match: ModifierMatch::Exact,
        }
        .normalize_case()
    }

    pub(super) fn plain(code: KeyCode) -> Self {
        Self::new(code, KeyModifiers::NONE)
    }

    pub(super) fn character(character: char) -> Self {
        Self::plain(KeyCode::Char(character))
    }

    pub(super) fn control(character: char) -> Self {
        Self::new(KeyCode::Char(character), KeyModifiers::CONTROL)
    }

    pub(super) fn with_required_modifiers(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self {
            code,
            modifiers,
            modifier_match: ModifierMatch::Contains,
        }
        .normalize_case()
    }

    pub(super) fn with_any_modifiers(code: KeyCode) -> Self {
        Self {
            code,
            modifiers: KeyModifiers::NONE,
            modifier_match: ModifierMatch::Any,
        }
        .normalize_case()
    }

    pub(super) fn shift(code: KeyCode) -> Self {
        Self::new(code, KeyModifiers::SHIFT)
    }

    pub(super) fn matches(self, event: &KeyEvent) -> bool {
        if event.kind == KeyEventKind::Release {
            return false;
        }
        let normalized = Self::new(event.code, event.modifiers);
        self.code == normalized.code
            && match self.modifier_match {
                ModifierMatch::Exact => self.modifiers == normalized.modifiers,
                ModifierMatch::Contains => normalized.modifiers.contains(self.modifiers),
                ModifierMatch::Any => true,
            }
    }

    pub(super) fn display_pretty(self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.contains(KeyModifiers::SUPER) {
            parts.push(if cfg!(target_os = "macos") {
                "Cmd".to_string()
            } else {
                "Super".to_string()
            });
        }
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            parts.push("Ctrl".to_string());
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            parts.push(if cfg!(target_os = "macos") {
                "Opt".to_string()
            } else {
                "Alt".to_string()
            });
        }
        let has_shift = self.modifiers.contains(KeyModifiers::SHIFT);
        if has_shift {
            parts.push("Shift".to_string());
        }
        if self.code == KeyCode::BackTab && !has_shift {
            parts.push("Shift".to_string());
        }
        parts.push(match self.code {
            KeyCode::Char(' ') => "Space".to_string(),
            KeyCode::Char(character) => character.to_ascii_lowercase().to_string(),
            KeyCode::Enter => "Enter".to_string(),
            KeyCode::Esc => "Esc".to_string(),
            KeyCode::Tab | KeyCode::BackTab => "Tab".to_string(),
            KeyCode::Backspace => "Backspace".to_string(),
            KeyCode::Delete => "Delete".to_string(),
            KeyCode::Up => "↑".to_string(),
            KeyCode::Down => "↓".to_string(),
            KeyCode::Left => "←".to_string(),
            KeyCode::Right => "→".to_string(),
            KeyCode::Home => "Home".to_string(),
            KeyCode::End => "End".to_string(),
            KeyCode::PageUp => "Page Up".to_string(),
            KeyCode::PageDown => "Page Down".to_string(),
            KeyCode::F(number) => format!("F{number}"),
            other => format!("{other:?}"),
        });
        parts.join("+")
    }

    fn normalize_case(mut self) -> Self {
        let KeyCode::Char(mut character) = self.code else {
            return self;
        };
        if self.modifiers.is_empty()
            && let Some(control) = c0_control_char_to_ctrl_char(character)
        {
            character = control;
            self.code = KeyCode::Char(character);
            self.modifiers = KeyModifiers::CONTROL;
        }
        if character.is_ascii_uppercase() {
            self.modifiers.insert(KeyModifiers::SHIFT);
        } else if self.modifiers.contains(KeyModifiers::SHIFT) {
            self.code = KeyCode::Char(character.to_ascii_uppercase());
        }
        self
    }
}

fn c0_control_char_to_ctrl_char(character: char) -> Option<char> {
    let code = u32::from(character);
    match code {
        0x00 => Some(' '),
        0x01..=0x1a => char::from_u32(code - 0x01 + u32::from('a')),
        0x1c..=0x1f => char::from_u32(code - 0x1c + u32::from('4')),
        _ => None,
    }
}

impl fmt::Display for KeyShortcut {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.display_pretty())
    }
}

pub(super) fn shift_tab_keys() -> [KeyShortcut; 3] {
    [
        KeyShortcut::plain(KeyCode::BackTab),
        KeyShortcut::shift(KeyCode::BackTab),
        KeyShortcut::with_required_modifiers(KeyCode::Tab, KeyModifiers::SHIFT),
    ]
}
