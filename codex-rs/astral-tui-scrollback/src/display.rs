use crate::PresentationBlock;
use crate::ToolKind;

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
            Self::Tool(tool) if tool.kind == ToolKind::Edit && !tool.changes.is_empty() => {
                DisplayMode::Expanded
            }
            Self::Tool(_) => DisplayMode::Collapsed,
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

    /// Whether the block participates in transcript navigation.
    ///
    /// Grok treats navigation and folding as separate capabilities: user and
    /// assistant messages are selectable even though they have nothing to
    /// collapse. Keeping those concepts separate prevents focus from jumping
    /// backwards to the last foldable tool when the visible tail is plain
    /// conversation text.
    pub fn is_selectable(&self) -> bool {
        !matches!(self, Self::Todo(_) | Self::System { .. })
    }
}
