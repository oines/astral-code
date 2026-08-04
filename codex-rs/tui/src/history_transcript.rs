//! Stable local identity for the authoritative `HistoryCell` transcript.
//!
//! `ChatWidget` remains responsible for projecting app-server events into
//! `HistoryCell`s. This container only adds presentation identity around that
//! already-ordered stream so selection, folding, and resize anchors do not
//! depend on vector indices.

use std::ops::Deref;
use std::ops::Range;
use std::sync::Arc;

use crate::history_cell::HistoryCell;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct HistoryEntryId(u64);

#[derive(Debug, Default)]
pub(crate) struct HistoryTranscript {
    cells: Vec<Arc<dyn HistoryCell>>,
    ids: Vec<HistoryEntryId>,
    next_id: u64,
}

impl HistoryTranscript {
    pub(crate) fn push(&mut self, cell: Arc<dyn HistoryCell>) -> HistoryEntryId {
        let id = self.allocate_id();
        self.cells.push(cell);
        self.ids.push(id);
        self.assert_aligned();
        id
    }

    pub(crate) fn remove(&mut self, index: usize) -> Arc<dyn HistoryCell> {
        self.ids.remove(index);
        let cell = self.cells.remove(index);
        self.assert_aligned();
        cell
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        self.cells.truncate(len);
        self.ids.truncate(len);
        self.assert_aligned();
    }

    pub(crate) fn clear(&mut self) {
        self.cells.clear();
        self.ids.clear();
    }

    /// Replace a canonicalized run while retaining the first source entry's
    /// identity. Streaming agent/plan cells use this when their finalized,
    /// source-backed cell replaces the provisional tail.
    pub(crate) fn consolidate(
        &mut self,
        range: Range<usize>,
        cell: Arc<dyn HistoryCell>,
    ) -> HistoryEntryId {
        assert!(
            range.start < range.end,
            "consolidation range must not be empty"
        );
        assert!(
            range.end <= self.cells.len(),
            "consolidation range must be in bounds"
        );
        let retained_id = self.ids[range.start];
        self.cells.splice(range.clone(), std::iter::once(cell));
        self.ids.splice(range, std::iter::once(retained_id));
        self.assert_aligned();
        retained_id
    }

    pub(crate) fn entries(
        &self,
    ) -> impl ExactSizeIterator<Item = (HistoryEntryId, &Arc<dyn HistoryCell>)> {
        self.ids.iter().copied().zip(&self.cells)
    }

    pub(crate) fn clone_entries(&self) -> Vec<(HistoryEntryId, Arc<dyn HistoryCell>)> {
        self.entries()
            .map(|(id, cell)| (id, cell.clone()))
            .collect()
    }

    fn allocate_id(&mut self) -> HistoryEntryId {
        assert_ne!(
            self.next_id,
            u64::MAX,
            "history entry identity space exhausted"
        );
        let id = HistoryEntryId(self.next_id);
        self.next_id += 1;
        id
    }

    fn assert_aligned(&self) {
        debug_assert_eq!(self.cells.len(), self.ids.len());
    }
}

impl Deref for HistoryTranscript {
    type Target = [Arc<dyn HistoryCell>];

    fn deref(&self) -> &Self::Target {
        &self.cells
    }
}

impl From<Vec<Arc<dyn HistoryCell>>> for HistoryTranscript {
    fn from(cells: Vec<Arc<dyn HistoryCell>>) -> Self {
        cells.into_iter().collect()
    }
}

impl FromIterator<Arc<dyn HistoryCell>> for HistoryTranscript {
    fn from_iter<T: IntoIterator<Item = Arc<dyn HistoryCell>>>(iter: T) -> Self {
        let mut transcript = Self::default();
        for cell in iter {
            transcript.push(cell);
        }
        transcript
    }
}

#[cfg(test)]
#[path = "history_transcript_tests.rs"]
mod tests;
