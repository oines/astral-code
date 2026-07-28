use astral_tui_scrollback::BlockTextMode;
use astral_tui_scrollback::PresentationBlock;

use crate::block_viewer::BlockViewerState;

use super::SurfaceState;

impl SurfaceState {
    pub(crate) fn toggle_selected_raw(&mut self) -> bool {
        self.scrollback.toggle_selected_raw()
    }

    pub(crate) fn selected_supports_copy(&self) -> bool {
        self.scrollback.selected_supports_copy()
    }

    pub(crate) fn selected_copy_meta_label(&self) -> Option<&'static str> {
        self.scrollback.selected_copy_meta_label()
    }

    pub(crate) fn selected_copy_text(&self) -> Option<String> {
        let mode = if self.scrollback.selected_is_raw() {
            BlockTextMode::Raw
        } else {
            BlockTextMode::Rendered
        };
        non_empty(self.selected_presentation_block()?.copy_text(mode))
    }

    pub(crate) fn selected_copy_meta(&self) -> Option<String> {
        non_empty(self.selected_presentation_block()?.copy_meta())
    }

    pub(crate) fn toggle_block_viewer_raw(&mut self) -> bool {
        let Some(entry_id) = self
            .block_viewer()
            .map(|viewer| viewer.entry_id().to_string())
        else {
            return false;
        };
        self.scrollback.toggle_raw(&entry_id)
    }

    pub(crate) fn block_viewer_copy_text(&mut self) -> Option<String> {
        if self
            .block_viewer()
            .is_some_and(BlockViewerState::visual_selection_active)
        {
            return non_empty(self.block_viewer_mut()?.take_visual_selection_text());
        }
        let entry_id = self.block_viewer()?.entry_id();
        let mode = if self.scrollback.is_raw_entry(entry_id) {
            BlockTextMode::Raw
        } else {
            BlockTextMode::Rendered
        };
        non_empty(self.presentation_block(entry_id)?.copy_text(mode))
    }

    pub(crate) fn block_viewer_copy_meta(&self) -> Option<String> {
        let entry_id = self.block_viewer()?.entry_id();
        non_empty(self.presentation_block(entry_id)?.copy_meta())
    }

    pub(super) fn block_viewer_text_mode(&self) -> BlockTextMode {
        self.block_viewer()
            .filter(|viewer| self.scrollback.is_raw_entry(viewer.entry_id()))
            .map_or(BlockTextMode::Rendered, |_| BlockTextMode::Raw)
    }

    fn selected_presentation_block(&self) -> Option<PresentationBlock> {
        let entry_id = self.scrollback.selected_id()?;
        self.presentation_block(entry_id)
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}
