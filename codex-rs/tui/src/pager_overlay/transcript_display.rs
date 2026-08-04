use std::collections::HashMap;

use astral_tui::DisplayMode;

use crate::history_cell::HistoryCell;
use crate::history_transcript::HistoryEntryId;

#[derive(Clone, Copy, Debug)]
pub(super) enum FoldAction {
    Toggle,
    Collapse,
    Expand,
}

#[derive(Default)]
pub(super) struct TranscriptDisplayState {
    modes: HashMap<HistoryEntryId, DisplayMode>,
}

impl TranscriptDisplayState {
    pub(super) fn mode_for(&mut self, id: HistoryEntryId, cell: &dyn HistoryCell) -> DisplayMode {
        let policy = cell.transcript_presentation();
        let mode = policy.normalize(self.modes.get(&id).copied());
        if policy.is_foldable() {
            self.modes.insert(id, mode);
        } else {
            self.modes.remove(&id);
        }
        mode
    }

    pub(super) fn apply(
        &mut self,
        id: HistoryEntryId,
        cell: &dyn HistoryCell,
        action: FoldAction,
    ) -> bool {
        let policy = cell.transcript_presentation();
        let current = policy.normalize(self.modes.get(&id).copied());
        let next = match action {
            FoldAction::Toggle => policy.toggle(current),
            FoldAction::Collapse => policy.collapse(),
            FoldAction::Expand => policy.expand(),
        };
        let Some(next) = next else {
            return false;
        };
        let changed = current != next;
        self.modes.insert(id, next);
        changed
    }

    pub(super) fn retain(&mut self, mut contains: impl FnMut(HistoryEntryId) -> bool) {
        self.modes.retain(|id, _| contains(*id));
    }
}
