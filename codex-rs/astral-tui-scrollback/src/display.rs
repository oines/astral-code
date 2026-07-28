use crate::PresentationBlock;
use crate::ToolStatus;

/// Presentation-only visibility for one transcript entry.
///
/// The app-server item remains authoritative; this mode controls only how
/// much of that item the TUI exposes in the scrollback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    Collapsed,
    Truncated,
    Expanded,
}

impl PresentationBlock {
    pub fn default_display_mode(&self) -> DisplayMode {
        match self {
            Self::User { .. } | Self::Assistant { .. } | Self::Todo(_) => DisplayMode::Expanded,
            Self::Thinking { running: true, .. } | Self::Plan { running: true, .. } => {
                DisplayMode::Truncated
            }
            Self::Thinking { running: false, .. }
            | Self::Plan { running: false, .. }
            | Self::Subagent(_)
            | Self::System { .. } => DisplayMode::Collapsed,
            Self::Tool(tool) => match tool.status {
                ToolStatus::Running
                | ToolStatus::Failed
                | ToolStatus::Declined
                | ToolStatus::Interrupted => DisplayMode::Truncated,
                ToolStatus::Success => DisplayMode::Collapsed,
            },
        }
    }

    pub fn is_foldable(&self) -> bool {
        match self {
            Self::Thinking { text, .. } | Self::Plan { text, .. } => !text.trim().is_empty(),
            Self::Tool(tool) => {
                !tool.details.is_empty()
                    || tool
                        .output
                        .as_deref()
                        .is_some_and(|output| !output.trim().is_empty())
                    || !tool.changes.is_empty()
            }
            Self::Subagent(subagent) => {
                subagent
                    .prompt
                    .as_deref()
                    .is_some_and(|prompt| !prompt.trim().is_empty())
                    || !subagent.agents.is_empty()
            }
            Self::System { detail, .. } => detail
                .as_deref()
                .is_some_and(|detail| !detail.trim().is_empty()),
            Self::User { .. } | Self::Assistant { .. } | Self::Todo(_) => false,
        }
    }
}
