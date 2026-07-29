//! Block-viewer access to Astral's canonical transcript projection.

use astral_tui_scrollback::PresentationBlock;

use super::SurfaceState;
use crate::block_viewer::BlockViewerState;
use crate::block_viewer::ViewerState;

impl SurfaceState {
    pub(crate) fn block_viewer(&self) -> Option<&ViewerState> {
        self.block_viewer.as_ref().map(BlockViewerState::viewer)
    }

    pub(crate) fn block_viewer_mut(&mut self) -> Option<&mut ViewerState> {
        self.block_viewer.as_mut().map(BlockViewerState::viewer_mut)
    }

    pub(crate) fn block_viewer_entry_id(&self) -> Option<&str> {
        self.block_viewer.as_ref().map(BlockViewerState::entry_id)
    }

    /// Apply Grok's Enter behavior to the selected transcript row.
    ///
    /// A group header owns Enter as its expand/collapse action. A normal
    /// selected row opens a viewer that follows the current canonical block.
    pub(crate) fn open_selected_entry(&mut self) -> bool {
        if self.scrollback.selected_is_group_header() {
            self.toggle_selected_entry();
            return true;
        }
        let Some(entry_id) = self.scrollback.selected_id().map(str::to_string) else {
            return false;
        };
        self.open_entry(entry_id)
    }

    pub(crate) fn open_entry(&mut self, entry_id: String) -> bool {
        let Some((_, running)) = self.presentation_block_state(&entry_id) else {
            return false;
        };
        self.block_viewer = Some(BlockViewerState::new(entry_id, running));
        true
    }

    pub(crate) fn close_block_viewer(&mut self) {
        self.block_viewer = None;
    }

    pub(super) fn current_block_viewer_entry(&self) -> Option<(PresentationBlock, bool)> {
        let entry_id = self.block_viewer_entry_id()?;
        self.presentation_block_state(entry_id)
    }

    pub(super) fn presentation_block(&self, entry_id: &str) -> Option<PresentationBlock> {
        self.presentation_block_state(entry_id)
            .map(|(block, _)| block)
    }

    fn presentation_block_state(&self, entry_id: &str) -> Option<(PresentationBlock, bool)> {
        let (turn_id, render_id) = entry_id.split_once('\0')?;
        self.conversation
            .presentation_block_state(turn_id, render_id)
    }
}
