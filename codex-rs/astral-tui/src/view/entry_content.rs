use std::collections::HashMap;
use std::collections::HashSet;

use astral_tui_scrollback::PresentationBlock;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct EntryContentCapabilities {
    supports_raw: bool,
    supports_copy: bool,
    supports_viewer: bool,
    double_click_opens: bool,
    copy_meta_label: Option<&'static str>,
}

impl EntryContentCapabilities {
    pub(super) fn for_block(block: &PresentationBlock) -> Self {
        Self {
            supports_raw: block.supports_raw(),
            supports_copy: block.supports_copy(),
            supports_viewer: block.supports_viewer(),
            double_click_opens: block.double_click_opens(),
            copy_meta_label: block.copy_meta_label(),
        }
    }

    pub(super) fn supports_raw(self) -> bool {
        self.supports_raw
    }

    pub(super) fn supports_copy(self) -> bool {
        self.supports_copy
    }

    pub(super) fn supports_viewer(self) -> bool {
        self.supports_viewer
    }

    pub(super) fn double_click_opens(self) -> bool {
        self.double_click_opens
    }

    pub(super) fn copy_meta_label(self) -> Option<&'static str> {
        self.copy_meta_label
    }
}

#[derive(Debug, Default)]
pub(super) struct EntryContentState {
    capabilities: HashMap<String, EntryContentCapabilities>,
    raw_entries: HashSet<String>,
}

impl EntryContentState {
    pub(super) fn observe(&mut self, entry_id: String, block: &PresentationBlock) {
        self.capabilities
            .insert(entry_id, EntryContentCapabilities::for_block(block));
    }

    pub(super) fn retain(&mut self, known_ids: &HashSet<String>) {
        self.capabilities
            .retain(|entry_id, _| known_ids.contains(entry_id));
        self.raw_entries
            .retain(|entry_id| known_ids.contains(entry_id));
    }

    pub(super) fn is_raw(&self, entry_id: &str) -> bool {
        self.raw_entries.contains(entry_id)
    }

    pub(super) fn supports_copy(&self, entry_id: &str) -> bool {
        self.capabilities
            .get(entry_id)
            .is_some_and(|capabilities| capabilities.supports_copy())
    }

    pub(super) fn supports_viewer(&self, entry_id: &str) -> bool {
        self.capabilities
            .get(entry_id)
            .is_some_and(|capabilities| capabilities.supports_viewer())
    }

    pub(super) fn double_click_opens(&self, entry_id: &str) -> bool {
        self.capabilities
            .get(entry_id)
            .is_some_and(|capabilities| capabilities.double_click_opens())
    }

    pub(super) fn copy_meta_label(&self, entry_id: &str) -> Option<&'static str> {
        self.capabilities.get(entry_id)?.copy_meta_label()
    }

    pub(super) fn toggle_raw(&mut self, entry_id: &str) -> bool {
        if !self
            .capabilities
            .get(entry_id)
            .is_some_and(|capabilities| capabilities.supports_raw())
        {
            return false;
        }
        if !self.raw_entries.remove(entry_id) {
            self.raw_entries.insert(entry_id.to_string());
        }
        true
    }
}
