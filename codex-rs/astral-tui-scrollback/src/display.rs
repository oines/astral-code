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
            Self::User { .. }
            | Self::Assistant { .. }
            | Self::Plan { running: false, .. }
            | Self::Todo(_) => DisplayMode::Expanded,
            Self::Thinking { running: true, .. } | Self::Plan { running: true, .. } => {
                DisplayMode::Truncated
            }
            Self::Thinking { running: false, .. } | Self::Subagent(_) | Self::System { .. } => {
                DisplayMode::Collapsed
            }
            Self::Tool(tool) if tool.kind == ToolKind::Edit && !tool.changes.is_empty() => {
                DisplayMode::Expanded
            }
            Self::Tool(tool) if tool.is_user_shell() => match tool.status {
                crate::ToolStatus::Running => DisplayMode::Truncated,
                crate::ToolStatus::Success
                | crate::ToolStatus::Failed
                | crate::ToolStatus::Declined
                | crate::ToolStatus::Interrupted => DisplayMode::Expanded,
            },
            Self::Tool(_) => DisplayMode::Collapsed,
        }
    }

    pub fn is_foldable(&self) -> bool {
        match self {
            // Grok's ThinkingBlock is always foldable, including the empty
            // streaming state. This keeps a Thought row on the inline fold
            // path instead of treating it as a generic viewer target.
            Self::Thinking { .. } => true,
            Self::Plan { text, .. } => !text.trim().is_empty(),
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

    /// Return the block-specific display mode used by a fold toggle.
    ///
    /// Grok's transcript does not give every block the same two-state fold:
    /// command and read entries open to a compact preview, while streaming
    /// thoughts and generic tools never toggle directly from fully expanded
    /// to fully hidden. Keep that presentation policy beside the block rather
    /// than teaching mouse and keyboard handlers about individual tool kinds.
    pub fn next_fold_mode(&self, current: DisplayMode) -> DisplayMode {
        match self {
            Self::Thinking { running, .. } | Self::Plan { running, .. } if *running => {
                match current {
                    DisplayMode::Collapsed | DisplayMode::Truncated => DisplayMode::Expanded,
                    DisplayMode::Expanded => DisplayMode::Truncated,
                }
            }
            Self::Tool(tool) if tool.is_user_shell() => match current {
                DisplayMode::Collapsed => DisplayMode::Expanded,
                DisplayMode::Truncated | DisplayMode::Expanded => DisplayMode::Collapsed,
            },
            Self::Tool(tool)
                if matches!(
                    tool.kind,
                    ToolKind::Execute | ToolKind::Background | ToolKind::Read
                ) =>
            {
                match current {
                    DisplayMode::Collapsed => DisplayMode::Truncated,
                    DisplayMode::Truncated | DisplayMode::Expanded => DisplayMode::Collapsed,
                }
            }
            Self::Tool(tool)
                if tool.kind == ToolKind::Other && tool.status == crate::ToolStatus::Running =>
            {
                match current {
                    DisplayMode::Collapsed => DisplayMode::Truncated,
                    DisplayMode::Truncated => DisplayMode::Expanded,
                    DisplayMode::Expanded => DisplayMode::Truncated,
                }
            }
            Self::User { .. }
            | Self::Assistant { .. }
            | Self::Thinking { .. }
            | Self::Plan { .. }
            | Self::Todo(_)
            | Self::Tool(_)
            | Self::Subagent(_)
            | Self::System { .. } => match current {
                DisplayMode::Collapsed => DisplayMode::Expanded,
                DisplayMode::Truncated | DisplayMode::Expanded => DisplayMode::Collapsed,
            },
        }
    }

    /// Return the minimum display mode used by an explicit collapse action.
    ///
    /// Running thoughts and streaming terminal-like entries retain a compact
    /// live preview, matching Grok's left-arrow behavior.
    pub fn collapse_mode(&self) -> DisplayMode {
        match self {
            Self::Thinking { running: true, .. } | Self::Plan { running: true, .. } => {
                DisplayMode::Truncated
            }
            Self::Tool(tool)
                if tool.status == crate::ToolStatus::Running
                    && (tool.is_user_shell() || tool.kind == ToolKind::Other) =>
            {
                DisplayMode::Truncated
            }
            Self::User { .. }
            | Self::Assistant { .. }
            | Self::Thinking { running: false, .. }
            | Self::Plan { running: false, .. }
            | Self::Todo(_)
            | Self::Tool(_)
            | Self::Subagent(_)
            | Self::System { .. } => DisplayMode::Collapsed,
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
