use std::collections::HashMap;
use std::collections::HashSet;

use astral_tui_scrollback::DisplayMode;
use astral_tui_scrollback::PresentationBlock;

use crate::conversation::TranscriptTurn;

use super::entry_content::EntryContentState;
use super::entry_group::EntryGroupKind;
use super::entry_group::EntryGroupSpan;
use super::entry_group::scan_turn;

#[derive(Debug, Clone, PartialEq, Eq)]
struct EntryDescriptor {
    id: String,
    default_mode: DisplayMode,
    parent_group: Option<String>,
    group_header: bool,
    foldable: bool,
    thinking: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GroupDescriptor {
    kind: EntryGroupKind,
    turn_id: String,
    range: std::ops::Range<usize>,
}

/// TUI fold and focus state keyed by Astral's stable local transcript ids.
#[derive(Debug, Default)]
pub(crate) struct EntryDisplayState {
    focused: bool,
    selected: Option<String>,
    render_revision: u64,
    entries: Vec<EntryDescriptor>,
    manual_modes: HashMap<String, DisplayMode>,
    groups: HashMap<String, GroupDescriptor>,
    entry_groups: HashMap<String, Vec<String>>,
    expanded_groups: HashSet<String>,
    content_state: EntryContentState,
    thinking_mode: Option<DisplayMode>,
    preserve_empty_selection: bool,
    pending_verb_rekey: Option<String>,
}

impl EntryDisplayState {
    pub(crate) fn render_revision(&self) -> u64 {
        self.render_revision
    }

    pub(crate) fn observe(&mut self, turns: &[TranscriptTurn]) {
        let mut groups_by_turn = turns
            .iter()
            .map(|turn| scan_turn(turn, self))
            .collect::<Vec<_>>();
        if self.rekey_expanded_verb_group(turns, &groups_by_turn) {
            groups_by_turn = turns.iter().map(|turn| scan_turn(turn, self)).collect();
        }

        let mut entries = Vec::new();
        let mut known_ids = HashSet::new();
        let mut groups_seen = HashMap::new();
        let mut entry_groups_seen = HashMap::new();
        for (turn, groups) in turns.iter().zip(groups_by_turn) {
            for (index, block) in turn.blocks.iter().enumerate() {
                if block.block.is_selectable() {
                    let id = entry_id(&turn.id, &block.item_id);
                    known_ids.insert(id.clone());
                    entry_groups_seen.insert(
                        id,
                        groups
                            .iter()
                            .filter(|group| group.range.contains(&index))
                            .map(|group| group.id.clone())
                            .collect(),
                    );
                }
            }
            for group in &groups {
                known_ids.insert(group.id.clone());
                groups_seen.insert(
                    group.id.clone(),
                    GroupDescriptor {
                        kind: group.kind,
                        turn_id: turn.id.clone(),
                        range: group.range.clone(),
                    },
                );
            }
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
                        foldable: true,
                        thinking: false,
                    });
                }
                let parent_group = groups
                    .iter()
                    .find(|group| group.expanded && group.contains_member(index))
                    .map(|group| group.id.clone());
                if groups.iter().any(|group| group.hides(index)) || !block.block.is_selectable() {
                    continue;
                }
                let thinking = matches!(&block.block, PresentationBlock::Thinking { .. });
                let id = entry_id(&turn.id, &block.item_id);
                self.content_state.observe(id.clone(), &block.block);
                entries.push(EntryDescriptor {
                    id,
                    default_mode: if thinking {
                        self.thinking_mode
                            .unwrap_or_else(|| block.block.default_display_mode())
                    } else {
                        block.block.default_display_mode()
                    },
                    parent_group,
                    group_header: false,
                    foldable: block.block.is_foldable(),
                    thinking,
                });
            }
        }
        self.entries = entries;
        self.groups = groups_seen;
        self.entry_groups = entry_groups_seen;

        self.manual_modes
            .retain(|entry_id, _| known_ids.contains(entry_id.as_str()));
        self.expanded_groups
            .retain(|entry_id| known_ids.contains(entry_id.as_str()));
        self.content_state.retain(&known_ids);
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
        if self.focused && self.selected.is_none() && !self.preserve_empty_selection {
            self.selected = self.entries.last().map(|entry| entry.id.clone());
        }
        if self.entries.is_empty() {
            self.focused = false;
            self.preserve_empty_selection = false;
        }
    }

    /// Move a verb group's expansion key when opening or closing its anchor
    /// makes the same run re-anchor on another member.
    ///
    /// This preserves Grok Build's `rekey_verb_group_expansion` invariant
    /// at commit `47348d13ec4508dcfe440e34c6d511bb02998fb2`
    /// (Apache-2.0) without leaking its index-based state into Astral's view.
    fn rekey_expanded_verb_group(
        &mut self,
        turns: &[TranscriptTurn],
        groups_by_turn: &[Vec<EntryGroupSpan>],
    ) -> bool {
        let Some(pending_entry_id) = self.pending_verb_rekey.take() else {
            return false;
        };
        let Some((turn_index, block_index)) =
            turns.iter().enumerate().find_map(|(turn_index, turn)| {
                turn.blocks
                    .iter()
                    .position(|block| entry_id(&turn.id, &block.item_id) == pending_entry_id)
                    .map(|block_index| (turn_index, block_index))
            })
        else {
            return false;
        };
        let turn = &turns[turn_index];
        let next_groups = &groups_by_turn[turn_index];
        let migration = self.expanded_groups.iter().find_map(|old_id| {
            let old = self.groups.get(old_id)?;
            if old.kind != EntryGroupKind::VerbRun || old.turn_id != turn.id {
                return None;
            }
            let next = next_groups
                .iter()
                .filter(|next| {
                    let overlaps =
                        old.range.start < next.range.end && next.range.start < old.range.end;
                    next.kind == EntryGroupKind::VerbRun
                        && next.id != *old_id
                        && overlaps
                        && (old.range.contains(&block_index) || next.range.contains(&block_index))
                })
                .max_by_key(|next| {
                    old.range
                        .end
                        .min(next.range.end)
                        .saturating_sub(old.range.start.max(next.range.start))
                })?;
            Some((old_id.clone(), next.id.clone()))
        });
        let Some((old_id, new_id)) = migration else {
            return false;
        };
        self.expanded_groups.remove(&old_id);
        self.expanded_groups.insert(new_id);
        true
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
            .unwrap_or_else(|| match block {
                PresentationBlock::Thinking { .. } => self
                    .thinking_mode
                    .unwrap_or_else(|| block.default_display_mode()),
                _ => block.default_display_mode(),
            })
    }

    pub(crate) fn focus_scrollback(&mut self) -> bool {
        let Some(last) = self.entries.last() else {
            return false;
        };
        self.focused = true;
        self.preserve_empty_selection = false;
        if self.selected.is_none() {
            self.selected = Some(last.id.clone());
        }
        self.bump_render_revision();
        true
    }

    pub(crate) fn focus_prompt(&mut self) {
        let changed = self.focused;
        self.focused = false;
        self.preserve_empty_selection = false;
        if changed {
            self.bump_render_revision();
        }
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
        let changed = !self.focused || self.selected.as_deref() != Some(entry_id);
        self.focused = true;
        self.preserve_empty_selection = false;
        self.selected = Some(entry_id.to_string());
        if changed {
            self.bump_render_revision();
        }
        true
    }

    pub(crate) fn contains(&self, entry_id: &str) -> bool {
        self.entries.iter().any(|entry| entry.id == entry_id)
    }

    pub(crate) fn reveal(&mut self, entry_id: &str) -> bool {
        let Some(groups) = self.entry_groups.get(entry_id) else {
            return false;
        };
        self.expanded_groups.extend(groups.iter().cloned());
        self.manual_modes
            .insert(entry_id.to_string(), DisplayMode::Expanded);
        self.focused = true;
        self.preserve_empty_selection = false;
        self.selected = Some(entry_id.to_string());
        self.bump_render_revision();
        true
    }

    pub(crate) fn selected_mode(&self) -> Option<DisplayMode> {
        self.selected_id().and_then(|entry_id| self.mode(entry_id))
    }

    pub(crate) fn mode(&self, entry_id: &str) -> Option<DisplayMode> {
        let entry = self.entries.iter().find(|entry| entry.id == entry_id)?;
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

    pub(crate) fn is_foldable(&self, entry_id: &str) -> bool {
        self.entries
            .iter()
            .find(|entry| entry.id == entry_id)
            .is_some_and(|entry| entry.foldable)
    }

    pub(crate) fn selected_is_group_header(&self) -> bool {
        self.selected_entry()
            .is_some_and(|entry| entry.group_header)
    }

    pub(crate) fn selected_is_foldable(&self) -> bool {
        self.selected_entry().is_some_and(|entry| entry.foldable)
    }

    pub(crate) fn selected_is_raw(&self) -> bool {
        self.selected_id()
            .is_some_and(|entry_id| self.content_state.is_raw(entry_id))
    }

    pub(crate) fn is_raw_entry(&self, entry_id: &str) -> bool {
        self.content_state.is_raw(entry_id)
    }

    pub(crate) fn selected_supports_copy(&self) -> bool {
        self.selected_id()
            .is_some_and(|entry_id| self.content_state.supports_copy(entry_id))
    }

    pub(crate) fn selected_copy_meta_label(&self) -> Option<&'static str> {
        self.content_state.copy_meta_label(self.selected_id()?)
    }

    pub(crate) fn is_raw(&self, turn_id: &str, item_id: &str) -> bool {
        self.content_state.is_raw(&entry_id(turn_id, item_id))
    }

    pub(crate) fn group_is_expanded(&self, group_id: &str) -> bool {
        self.expanded_groups.contains(group_id)
    }

    pub(crate) fn move_selection(&mut self, delta: isize) -> Option<String> {
        let last = self.entries.len().checked_sub(1)?;
        let previous = self.selected.clone();
        let next = self
            .selected
            .as_deref()
            .and_then(|selected| self.entries.iter().position(|entry| entry.id == selected))
            .map_or_else(
                || if delta < 0 { last } else { 0 },
                |current| current.saturating_add_signed(delta).min(last),
            );
        let entry_id = self.entries.get(next)?.id.clone();
        self.preserve_empty_selection = false;
        self.selected = Some(entry_id.clone());
        if self.selected != previous {
            self.bump_render_revision();
        }
        Some(entry_id)
    }

    pub(crate) fn select_first(&mut self) -> Option<String> {
        let entry_id = self.entries.first()?.id.clone();
        self.select(&entry_id).then_some(entry_id)
    }

    pub(crate) fn select_last(&mut self) -> Option<String> {
        let entry_id = self.entries.last()?.id.clone();
        self.select(&entry_id).then_some(entry_id)
    }

    pub(crate) fn toggle_selected(&mut self) -> Option<String> {
        let entry = self.selected_entry()?.clone();
        if entry.group_header {
            return self.toggle_group(&entry.id);
        }
        if !entry.foldable {
            return None;
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
        self.pending_verb_rekey = Some(entry.id.clone());
        self.bump_render_revision();
        Some(entry.id)
    }

    pub(crate) fn toggle_selected_raw(&mut self) -> Option<String> {
        let entry = self.selected_entry()?;
        let entry_id = entry.id.clone();
        self.toggle_raw(&entry_id).then_some(entry_id)
    }

    pub(crate) fn toggle_raw(&mut self, entry_id: &str) -> bool {
        let toggled = self.content_state.toggle_raw(entry_id);
        if toggled {
            self.bump_render_revision();
        }
        toggled
    }

    pub(crate) fn expand_selected(&mut self) -> Option<String> {
        let entry = self.selected_entry()?.clone();
        if entry.group_header {
            if self.expanded_groups.contains(&entry.id) {
                return Some(entry.id);
            }
            return self.toggle_group(&entry.id);
        }
        if !entry.foldable {
            return None;
        }
        self.manual_modes
            .insert(entry.id.clone(), DisplayMode::Expanded);
        self.pending_verb_rekey = Some(entry.id.clone());
        self.bump_render_revision();
        Some(entry.id)
    }

    pub(crate) fn collapse_selected(&mut self) -> Option<String> {
        let entry = self.selected_entry()?.clone();
        if entry.group_header {
            if self.expanded_groups.remove(&entry.id) {
                self.preserve_empty_selection = false;
                self.bump_render_revision();
                return Some(entry.id);
            }
            return None;
        }
        if !entry.foldable {
            return None;
        }
        let current = self
            .manual_modes
            .get(&entry.id)
            .copied()
            .unwrap_or(entry.default_mode);
        if current != DisplayMode::Collapsed {
            self.manual_modes
                .insert(entry.id.clone(), DisplayMode::Collapsed);
            self.pending_verb_rekey = Some(entry.id.clone());
            self.bump_render_revision();
            return Some(entry.id);
        }
        let parent = entry.parent_group?;
        self.expanded_groups.remove(&parent);
        self.preserve_empty_selection = false;
        self.selected = Some(parent.clone());
        self.bump_render_revision();
        Some(parent)
    }

    pub(crate) fn toggle_all(&mut self) {
        let any_collapsed = self.entries.iter().any(|entry| {
            !entry.group_header
                && entry.foldable
                && self.mode(&entry.id) == Some(DisplayMode::Collapsed)
        });
        let target = if any_collapsed {
            DisplayMode::Expanded
        } else {
            DisplayMode::Collapsed
        };
        for entry in &self.entries {
            if !entry.group_header && entry.foldable {
                self.manual_modes.insert(entry.id.clone(), target);
            }
        }
        self.expanded_groups.clear();
        self.pending_verb_rekey = None;
        self.bump_render_revision();
    }

    pub(crate) fn toggle_all_thinking(&mut self) {
        let any_collapsed = self.entries.iter().any(|entry| {
            entry.thinking && entry.foldable && self.mode(&entry.id) == Some(DisplayMode::Collapsed)
        });
        let target = if any_collapsed {
            DisplayMode::Expanded
        } else {
            DisplayMode::Collapsed
        };
        self.thinking_mode = Some(target);
        for entry in &self.entries {
            if entry.thinking && entry.foldable {
                self.manual_modes.insert(entry.id.clone(), target);
            }
        }
        if target == DisplayMode::Expanded {
            self.expanded_groups.extend(self.groups.keys().cloned());
        } else {
            self.expanded_groups.clear();
        }
        self.pending_verb_rekey = None;
        self.bump_render_revision();
    }

    pub(crate) fn thinking_fold_label(&self) -> &'static str {
        if self.entries.iter().any(|entry| {
            entry.thinking && entry.foldable && self.mode(&entry.id) == Some(DisplayMode::Collapsed)
        }) {
            "expand thinking"
        } else {
            "collapse thinking"
        }
    }

    pub(crate) fn toggle_group(&mut self, group_id: &str) -> Option<String> {
        let kind = self.groups.get(group_id)?.kind;
        let expanding = !self.expanded_groups.remove(group_id);
        if expanding {
            self.expanded_groups.insert(group_id.to_string());
            if kind == EntryGroupKind::Truncation {
                self.selected = None;
                self.preserve_empty_selection = true;
            }
        } else {
            self.preserve_empty_selection = false;
        }
        self.bump_render_revision();
        Some(group_id.to_string())
    }

    fn bump_render_revision(&mut self) {
        self.render_revision = self.render_revision.wrapping_add(1);
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
