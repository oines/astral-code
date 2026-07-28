use std::collections::HashMap;
use std::collections::HashSet;

use astral_tui_scrollback::DisplayMode;
use astral_tui_scrollback::PresentationBlock;

use crate::conversation::TranscriptTurn;

#[derive(Debug, Clone, PartialEq, Eq)]
struct EntryDescriptor {
    id: String,
    default_mode: DisplayMode,
}

/// Fold and focus state owned by the TUI presentation layer.
///
/// Entries are keyed by Astral's stable local transcript ids rather than
/// provider item ids, so provider id reuse cannot move a manual fold.
#[derive(Debug, Default)]
pub(crate) struct EntryDisplayState {
    focused: bool,
    selected: Option<String>,
    entries: Vec<EntryDescriptor>,
    manual_modes: HashMap<String, DisplayMode>,
}

impl EntryDisplayState {
    pub(crate) fn observe(&mut self, turns: &[TranscriptTurn]) {
        self.entries = turns
            .iter()
            .flat_map(|turn| {
                turn.blocks
                    .iter()
                    .filter(|&block| block.block.is_foldable())
                    .map(|block| EntryDescriptor {
                        id: entry_id(&turn.id, &block.item_id),
                        default_mode: block.block.default_display_mode(),
                    })
            })
            .collect();

        let visible_ids = self
            .entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<HashSet<_>>();
        self.manual_modes
            .retain(|entry_id, _| visible_ids.contains(entry_id.as_str()));
        if self
            .selected
            .as_ref()
            .is_some_and(|selected| !visible_ids.contains(selected.as_str()))
        {
            self.selected = None;
        }
        if self.focused && self.selected.is_none() {
            self.selected = self.entries.last().map(|entry| entry.id.clone());
        }
        if self.entries.is_empty() {
            self.focused = false;
        }
    }

    pub(crate) fn mode_for(
        &self,
        turn_id: &str,
        item_id: &str,
        block: &PresentationBlock,
    ) -> DisplayMode {
        self.manual_modes
            .get(&entry_id(turn_id, item_id))
            .copied()
            .unwrap_or_else(|| block.default_display_mode())
    }

    pub(crate) fn focus_scrollback(&mut self) -> bool {
        let Some(last) = self.entries.last() else {
            return false;
        };
        self.focused = true;
        if self.selected.is_none() {
            self.selected = Some(last.id.clone());
        }
        true
    }

    pub(crate) fn focus_prompt(&mut self) {
        self.focused = false;
    }

    pub(crate) fn is_focused(&self) -> bool {
        self.focused
    }

    pub(crate) fn selected_id(&self) -> Option<&str> {
        if self.focused {
            self.selected.as_deref()
        } else {
            None
        }
    }

    pub(crate) fn move_selection(&mut self, delta: isize) -> Option<String> {
        let selected = self.selected.as_deref();
        let current = selected
            .and_then(|selected| self.entries.iter().position(|entry| entry.id == selected))
            .unwrap_or_else(|| self.entries.len().saturating_sub(1));
        let next = current
            .saturating_add_signed(delta)
            .min(self.entries.len().saturating_sub(1));
        let entry = self.entries.get(next)?;
        self.selected = Some(entry.id.clone());
        Some(entry.id.clone())
    }

    pub(crate) fn toggle_selected(&mut self) -> Option<String> {
        let entry = self.selected_entry()?.clone();
        let current = self
            .manual_modes
            .get(&entry.id)
            .copied()
            .unwrap_or(entry.default_mode);
        let target = match current {
            DisplayMode::Collapsed | DisplayMode::Truncated => DisplayMode::Expanded,
            DisplayMode::Expanded => DisplayMode::Collapsed,
        };
        self.manual_modes.insert(entry.id.clone(), target);
        Some(entry.id)
    }

    pub(crate) fn expand_selected(&mut self) -> Option<String> {
        self.set_selected_mode(DisplayMode::Expanded)
    }

    pub(crate) fn collapse_selected(&mut self) -> Option<String> {
        self.set_selected_mode(DisplayMode::Collapsed)
    }

    fn set_selected_mode(&mut self, mode: DisplayMode) -> Option<String> {
        let entry = self.selected_entry()?.clone();
        self.manual_modes.insert(entry.id.clone(), mode);
        Some(entry.id)
    }

    fn selected_entry(&self) -> Option<&EntryDescriptor> {
        let selected = self.selected.as_deref()?;
        self.entries.iter().find(|entry| entry.id == selected)
    }
}

pub(crate) fn entry_id(turn_id: &str, item_id: &str) -> String {
    format!("{turn_id}\0{item_id}")
}

#[cfg(test)]
#[path = "entry_state_tests.rs"]
mod tests;
