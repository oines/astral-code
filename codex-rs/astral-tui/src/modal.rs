//! Shared state for Astral modal windows.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModalRow {
    pub(crate) label: String,
    pub(crate) value: String,
}

impl ModalRow {
    pub(crate) fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModalState {
    pub(crate) title: String,
    pub(crate) rows: Vec<ModalRow>,
    pub(crate) scroll_offset: usize,
}

impl ModalState {
    pub(crate) fn info(title: impl Into<String>, rows: Vec<ModalRow>) -> Self {
        Self {
            title: title.into(),
            rows,
            scroll_offset: 0,
        }
    }

    pub(crate) fn scroll_by(&mut self, delta: isize) {
        self.scroll_offset = self
            .scroll_offset
            .saturating_add_signed(delta)
            .min(self.rows.len().saturating_sub(1));
    }

    pub(crate) fn scroll_to_start(&mut self) {
        self.scroll_offset = 0;
    }

    pub(crate) fn scroll_to_end(&mut self) {
        self.scroll_offset = self.rows.len().saturating_sub(1);
    }
}
