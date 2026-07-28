use std::collections::HashMap;
use std::collections::HashSet;

use astral_tui_scrollback::DisplayMode;
use astral_tui_scrollback::PresentationBlock;

use crate::conversation::TranscriptTurn;

use super::entry_group::scan_turn;

#[derive(Debug, Clone, PartialEq, Eq)]
struct EntryDescriptor {
    id: String,
    default_mode: DisplayMode,
    parent_group: Option<String>,
    group_header: bool,
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
    groups: HashSet<String>,
    expanded_groups: HashSet<String>,
}

impl EntryDisplayState {
    pub(crate) fn observe(&mut self, turns: &[TranscriptTurn]) {
        let mut entries = Vec::new();
        let mut known_ids = HashSet::new();
        let mut groups_seen = HashSet::new();
        for turn in turns {
            for block in &turn.blocks {
                if block.block.is_foldable() {
                    known_ids.insert(entry_id(&turn.id, &block.item_id));
                }
            }
            let groups = scan_turn(turn, self);
            groups_seen.extend(groups.iter().map(|group| group.id.clone()));
            for (index, block) in turn.blocks.iter().enumerate() {
                if let Some(group) = groups
                    .iter()
                    .find(|group| group.range.start == index && group.header_owns_selection())
                {
                    entries.push(EntryDescriptor {
                        id: group.id.clone(),
                        default_mode: DisplayMode::Collapsed,
                        parent_group: None,
                        group_header: true,
                    });
                }
                let parent_group = groups
                    .iter()
                    .find(|group| group.expanded && group.contains_member(index))
                    .map(|group| group.id.clone());
                if groups.iter().any(|group| group.hides(index)) || !block.block.is_foldable() {
                    continue;
                }
                entries.push(EntryDescriptor {
                    id: entry_id(&turn.id, &block.item_id),
                    default_mode: block.block.default_display_mode(),
                    parent_group,
                    group_header: false,
                });
            }
        }
        self.entries = entries;
        self.groups = groups_seen;

        self.manual_modes
            .retain(|entry_id, _| known_ids.contains(entry_id.as_str()));
        self.expanded_groups
            .retain(|entry_id| known_ids.contains(entry_id.as_str()));
        let visible_ids = self
            .entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<HashSet<_>>();
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

    pub(crate) fn select(&mut self, entry_id: &str) -> bool {
        if !self.entries.iter().any(|entry| entry.id == entry_id) {
            return false;
        }
        self.focused = true;
        self.selected = Some(entry_id.to_string());
        true
    }

    pub(crate) fn selected_mode(&self) -> Option<DisplayMode> {
        let entry = self.selected_entry()?;
        if entry.group_header {
            return Some(if self.expanded_groups.contains(&entry.id) {
                DisplayMode::Expanded
            } else {
                DisplayMode::Collapsed
            });
        }
        Some(
            self.manual_modes
                .get(&entry.id)
                .copied()
                .unwrap_or(entry.default_mode),
        )
    }

    pub(crate) fn group_is_expanded(&self, group_id: &str) -> bool {
        self.expanded_groups.contains(group_id)
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
        if entry.group_header {
            return self.toggle_group(&entry.id);
        }
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
        let entry = self.selected_entry()?.clone();
        if entry.group_header {
            self.expanded_groups.insert(entry.id.clone());
            return Some(entry.id);
        }
        self.manual_modes
            .insert(entry.id.clone(), DisplayMode::Expanded);
        Some(entry.id)
    }

    pub(crate) fn collapse_selected(&mut self) -> Option<String> {
        let entry = self.selected_entry()?.clone();
        if entry.group_header {
            return self.expanded_groups.remove(&entry.id).then_some(entry.id);
        }
        let current = self
            .manual_modes
            .get(&entry.id)
            .copied()
            .unwrap_or(entry.default_mode);
        if current != DisplayMode::Collapsed {
            self.manual_modes
                .insert(entry.id.clone(), DisplayMode::Collapsed);
            return Some(entry.id);
        }
        let parent = entry.parent_group?;
        self.expanded_groups.remove(&parent);
        self.selected = Some(parent.clone());
        Some(parent)
    }

    pub(crate) fn toggle_group(&mut self, group_id: &str) -> Option<String> {
        if !self.groups.contains(group_id) {
            return None;
        }
        if !self.expanded_groups.remove(group_id) {
            self.expanded_groups.insert(group_id.to_string());
        }
        Some(group_id.to_string())
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
