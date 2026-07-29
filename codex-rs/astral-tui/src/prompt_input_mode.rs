//! Mutually exclusive semantic modes for the main prompt.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PromptInputMode {
    #[default]
    Normal,
    Shell,
}

impl PromptInputMode {
    pub(crate) const fn prefix(self) -> &'static str {
        match self {
            Self::Normal => "❯ ",
            Self::Shell => "! ",
        }
    }

    pub(crate) const fn info(self) -> Option<&'static str> {
        match self {
            Self::Normal => None,
            Self::Shell => Some("Run shell command"),
        }
    }

    pub(crate) const fn is_shell(self) -> bool {
        matches!(self, Self::Shell)
    }
}
