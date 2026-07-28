//! Block-viewer access to Astral's canonical transcript projection.

use astral_tui_scrollback::PresentationBlock;

use super::SurfaceState;
use crate::block_viewer::BlockViewerState;

impl SurfaceState {
    pub(crate) fn block_viewer(&self) -> Option<&BlockViewerState> {
        self.block_viewer.as_ref()
    }

    pub(crate) fn block_viewer_mut(&mut self) -> Option<&mut BlockViewerState> {
        self.block_viewer.as_mut()
    }

    /// Apply Grok's Enter behavior to the selected transcript row.
    ///
    /// A group header owns Enter as its expand/collapse action. A normal
    /// foldable row opens a viewer that follows the current canonical block.
    pub(crate) fn open_selected_entry(&mut self) -> bool {
        if self.entry_display.selected_is_group_header() {
            self.toggle_selected_entry();
            return true;
        }
        let Some(entry_id) = self.entry_display.selected_id().map(str::to_string) else {
            return false;
        };
        if self.presentation_block(&entry_id).is_none() {
            return false;
        }
        self.block_viewer = Some(BlockViewerState::new(entry_id));
        true
    }

    pub(crate) fn close_block_viewer(&mut self) {
        self.block_viewer = None;
    }

    pub(super) fn current_block_viewer_block(&self) -> Option<PresentationBlock> {
        let entry_id = self.block_viewer.as_ref()?.entry_id();
        self.presentation_block(entry_id)
    }

    fn presentation_block(&self, entry_id: &str) -> Option<PresentationBlock> {
        let (turn_id, render_id) = entry_id.split_once('\0')?;
        self.conversation.presentation_block(turn_id, render_id)
    }
}
