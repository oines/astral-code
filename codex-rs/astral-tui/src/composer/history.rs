//! Bounded prompt undo/redo history.
//!
//! The batching rules follow Grok Build's `xai-ratatui-textarea` at commit
//! 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0). Astral snapshots its
//! structured mention bindings alongside text so undo never silently changes
//! the next app-server submission.

use std::ops::Range;

use crate::mention::MentionBinding;

use super::ComposerState;

const MAX_DEPTH: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EditSnapshot {
    pub(super) text: String,
    pub(super) cursor: usize,
    pub(super) mention_bindings: Vec<MentionBinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MutationKind {
    Insert,
    Delete,
    Kill,
    Replace,
}

impl MutationKind {
    fn is_discrete(self) -> bool {
        matches!(self, Self::Kill | Self::Replace)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct EditHistory {
    undo: Vec<EditSnapshot>,
    redo: Vec<EditSnapshot>,
    last_kind: Option<MutationKind>,
    last_cursor: usize,
    last_insert_whitespace: bool,
}

impl EditHistory {
    pub(super) fn break_insert_batch_at(&mut self, text: &str) {
        let Some(first) = text.chars().next() else {
            return;
        };
        if self.last_kind == Some(MutationKind::Insert)
            && self.last_insert_whitespace != first.is_whitespace()
        {
            self.last_kind = None;
        }
    }

    pub(super) fn record_before(
        &mut self,
        kind: MutationKind,
        cursor: usize,
        snapshot: EditSnapshot,
    ) {
        let should_push = self.last_kind.is_none_or(|previous| {
            previous != kind || cursor != self.last_cursor || kind.is_discrete()
        });
        if should_push {
            self.undo.push(snapshot);
            if self.undo.len() > MAX_DEPTH {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last_kind = Some(kind);
    }

    pub(super) fn record_after(&mut self, cursor: usize) {
        self.last_cursor = cursor;
    }

    pub(super) fn record_inserted_text(&mut self, text: &str) {
        if let Some(last) = text.chars().next_back() {
            self.last_insert_whitespace = last.is_whitespace();
        }
    }

    pub(super) fn undo(&mut self, current: EditSnapshot) -> Option<EditSnapshot> {
        let snapshot = self.undo.pop()?;
        self.redo.push(current);
        self.reset_batch(snapshot.cursor);
        Some(snapshot)
    }

    pub(super) fn redo(&mut self, current: EditSnapshot) -> Option<EditSnapshot> {
        let snapshot = self.redo.pop()?;
        self.undo.push(current);
        if self.undo.len() > MAX_DEPTH {
            self.undo.remove(0);
        }
        self.reset_batch(snapshot.cursor);
        Some(snapshot)
    }

    fn reset_batch(&mut self, cursor: usize) {
        self.last_kind = None;
        self.last_cursor = cursor;
    }
}

impl ComposerState {
    pub(super) fn kill_range(&mut self, range: Range<usize>) {
        self.kill_buffer = self.text[range.clone()].to_string();
        self.replace_range(range, "", MutationKind::Kill);
    }

    pub(super) fn replace_range(
        &mut self,
        range: Range<usize>,
        replacement: &str,
        kind: MutationKind,
    ) {
        self.begin_mutation(kind);
        let removed_len = range.end.saturating_sub(range.start);
        let inserted_len = replacement.len();
        self.mention_bindings.retain_mut(|binding| {
            if range.is_empty() {
                return adjust_binding_for_insertion(binding, range.start, replacement);
            }
            if range.end <= binding.range.start {
                binding.range.start = shifted_index(binding.range.start, removed_len, inserted_len);
                binding.range.end = shifted_index(binding.range.end, removed_len, inserted_len);
                return true;
            }
            range.start >= binding.range.end
        });
        self.text.replace_range(range.clone(), replacement);
        self.cursor = range.start.saturating_add(inserted_len);
        self.preferred_column = None;
        self.finish_mutation();
        self.clear_selection_state();
    }

    pub(super) fn undo(&mut self) -> bool {
        let current = self.snapshot();
        let Some(snapshot) = self.history.undo(current) else {
            return false;
        };
        self.restore_snapshot(snapshot);
        true
    }

    pub(super) fn redo(&mut self) -> bool {
        let current = self.snapshot();
        let Some(snapshot) = self.history.redo(current) else {
            return false;
        };
        self.restore_snapshot(snapshot);
        true
    }

    pub(super) fn begin_mutation(&mut self, kind: MutationKind) {
        let snapshot = self.snapshot();
        self.history.record_before(kind, self.cursor, snapshot);
    }

    pub(super) fn finish_mutation(&mut self) {
        self.history.record_after(self.cursor);
    }

    fn snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            mention_bindings: self.mention_bindings.clone(),
        }
    }

    fn restore_snapshot(&mut self, snapshot: EditSnapshot) {
        self.text = snapshot.text;
        self.cursor = snapshot.cursor.min(self.text.len());
        self.mention_bindings = snapshot.mention_bindings;
        self.preferred_column = None;
        self.clear_selection_state();
    }
}

fn adjust_binding_for_insertion(
    binding: &mut MentionBinding,
    position: usize,
    inserted: &str,
) -> bool {
    if position < binding.range.start {
        binding.range.start = binding.range.start.saturating_add(inserted.len());
        binding.range.end = binding.range.end.saturating_add(inserted.len());
        return true;
    }
    if position == binding.range.start {
        if !inserted
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
        {
            return false;
        }
        binding.range.start = binding.range.start.saturating_add(inserted.len());
        binding.range.end = binding.range.end.saturating_add(inserted.len());
        return true;
    }
    if position < binding.range.end {
        return false;
    }
    position != binding.range.end || inserted.chars().next().is_some_and(char::is_whitespace)
}

fn shifted_index(index: usize, removed_len: usize, inserted_len: usize) -> usize {
    index
        .saturating_sub(removed_len)
        .saturating_add(inserted_len)
}
