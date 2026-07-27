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
}

impl ModalState {
    pub(crate) fn info(title: impl Into<String>, rows: Vec<ModalRow>) -> Self {
        Self {
            title: title.into(),
            rows,
        }
    }
}
