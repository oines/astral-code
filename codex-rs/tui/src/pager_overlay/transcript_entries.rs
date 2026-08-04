use std::ops::Range;
use std::sync::Arc;

use crate::history_cell::HistoryCell;
use crate::history_transcript::HistoryEntryId;

#[derive(Clone)]
pub(super) struct TranscriptEntry {
    id: HistoryEntryId,
    cell: Arc<dyn HistoryCell>,
}

impl TranscriptEntry {
    pub(super) fn id(&self) -> HistoryEntryId {
        self.id
    }

    pub(super) fn cell(&self) -> &Arc<dyn HistoryCell> {
        &self.cell
    }
}

pub(super) struct TranscriptEntries {
    entries: Vec<TranscriptEntry>,
    highlighted: Option<HistoryEntryId>,
}

impl TranscriptEntries {
    pub(super) fn new(entries: Vec<(HistoryEntryId, Arc<dyn HistoryCell>)>) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|(id, cell)| TranscriptEntry { id, cell })
                .collect(),
            highlighted: None,
        }
    }

    pub(super) fn iter(&self) -> impl ExactSizeIterator<Item = &TranscriptEntry> {
        self.entries.iter()
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn highlighted(&self) -> Option<HistoryEntryId> {
        self.highlighted
    }

    pub(super) fn highlighted_index(&self) -> Option<usize> {
        self.highlighted
            .and_then(|id| self.entries.iter().position(|entry| entry.id == id))
    }

    pub(super) fn insert(&mut self, id: HistoryEntryId, cell: Arc<dyn HistoryCell>) {
        self.entries.push(TranscriptEntry { id, cell });
    }

    pub(super) fn replace(&mut self, entries: Vec<(HistoryEntryId, Arc<dyn HistoryCell>)>) {
        self.entries = entries
            .into_iter()
            .map(|(id, cell)| TranscriptEntry { id, cell })
            .collect();
        self.clear_missing_highlight();
    }

    /// Replace a canonicalized range while retaining its first entry's identity.
    pub(super) fn consolidate(&mut self, range: Range<usize>, cell: Arc<dyn HistoryCell>) -> bool {
        let end = range.end.min(self.entries.len());
        let start = range.start.min(end);
        if start == end {
            return false;
        }
        let Some(retained_id) = self.entries.get(start).map(TranscriptEntry::id) else {
            return false;
        };
        self.entries.splice(
            start..end,
            std::iter::once(TranscriptEntry {
                id: retained_id,
                cell,
            }),
        );
        self.clear_missing_highlight();
        true
    }

    pub(super) fn set_highlight_index(&mut self, index: Option<usize>) {
        self.highlighted = index.and_then(|index| self.entries.get(index).map(TranscriptEntry::id));
    }

    fn clear_missing_highlight(&mut self) {
        if self
            .highlighted
            .is_some_and(|id| !self.entries.iter().any(|entry| entry.id == id))
        {
            self.highlighted = None;
        }
    }
}
