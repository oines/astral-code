// Derived from Grok Build's scrollback layout and entry chrome rules at
// commit 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Adapted to Astral's provider-neutral PresentationBlock stream.

use std::ops::Range;

use astral_tui_scrollback::DisplayMode;
use astral_tui_scrollback::LineJoiner;
use astral_tui_scrollback::PresentationBlock;
use astral_tui_scrollback::ToolKind;
use astral_tui_scrollback::ToolStatus;
use ratatui::style::Color;
use ratatui::text::Line;

use super::AstralTheme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptSection {
    pub(crate) item_id: String,
    pub(crate) lines: Range<usize>,
    pub(crate) kind: TranscriptSectionKind,
    pub(crate) accent: Option<TranscriptAccent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TranscriptAccent {
    Full(Color),
    Collapsed(Color),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TranscriptSectionKind {
    Entry,
    GroupHeader,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptSelectableLine {
    pub(crate) line: usize,
    pub(crate) columns: Range<u16>,
    pub(crate) joiner_to_previous: LineJoiner,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TranscriptSelectableRange {
    pub(crate) lines: Vec<TranscriptSelectableLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptGroup {
    pub(crate) lines: Range<usize>,
    pub(crate) member_ids: Vec<String>,
    pub(crate) expanded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptAnchor {
    pub(crate) item_id: String,
    pub(crate) line_offset: usize,
    pub(crate) section_height: usize,
}

impl TranscriptAnchor {
    pub(crate) fn at(
        sections: &[TranscriptSection],
        total_lines: usize,
        line: usize,
    ) -> Option<Self> {
        let line = line.min(total_lines.checked_sub(1)?);
        let section = sections
            .iter()
            .find(|section| section.lines.contains(&line))
            .or_else(|| {
                sections
                    .iter()
                    .rev()
                    .find(|section| section.lines.start <= line)
            })
            .or_else(|| sections.first())?;
        Some(Self {
            item_id: section.item_id.clone(),
            line_offset: line.saturating_sub(section.lines.start),
            section_height: section.lines.len().max(1),
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TranscriptLayout {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) sections: Vec<TranscriptSection>,
    pub(crate) groups: Vec<TranscriptGroup>,
    pub(crate) selectable_ranges: Vec<TranscriptSelectableRange>,
}

impl TranscriptLayout {
    pub(crate) fn section(&self, item_id: &str) -> Option<&TranscriptSection> {
        self.sections
            .iter()
            .find(|section| {
                section.item_id == item_id && section.kind == TranscriptSectionKind::Entry
            })
            .or_else(|| {
                self.sections
                    .iter()
                    .find(|section| section.item_id == item_id)
            })
    }

    pub(crate) fn selection_lines(&self, item_id: &str) -> Option<Range<usize>> {
        if let Some(group) = self
            .groups
            .iter()
            .find(|group| group.expanded && group.member_ids.iter().any(|id| id == item_id))
        {
            return Some(group.lines.clone());
        }
        self.section(item_id).map(|section| section.lines.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EntrySpacing {
    Dense,
    Separated,
}

pub(super) fn entry_spacing(block: &PresentationBlock, mode: DisplayMode) -> EntrySpacing {
    let groupable = !matches!(
        block,
        PresentationBlock::User { .. }
            | PresentationBlock::Assistant { .. }
            | PresentationBlock::Todo(_)
    );
    if groupable && mode == DisplayMode::Collapsed {
        EntrySpacing::Dense
    } else {
        EntrySpacing::Separated
    }
}

pub(super) fn begin_entry(
    lines: &mut Vec<Line<'static>>,
    previous: &mut Option<EntrySpacing>,
    current: EntrySpacing,
) {
    if previous.is_some_and(|previous| {
        previous == EntrySpacing::Separated || current == EntrySpacing::Separated
    }) {
        lines.push(Line::default());
    }
    *previous = Some(current);
}

pub(super) fn entry_accent(
    block: &PresentationBlock,
    mode: DisplayMode,
    theme: AstralTheme,
) -> Option<TranscriptAccent> {
    let color = match block {
        PresentationBlock::Thinking { .. } if mode != DisplayMode::Collapsed => theme.gray_dim,
        PresentationBlock::Tool(tool) if tool.kind == ToolKind::Execute => {
            status_accent(tool.status, theme)
        }
        PresentationBlock::Tool(tool)
            if matches!(
                tool.kind,
                ToolKind::Background
                    | ToolKind::BackgroundPoll
                    | ToolKind::BackgroundInput
                    | ToolKind::BackgroundList
                    | ToolKind::BackgroundStop
            ) && tool.status == ToolStatus::Running =>
        {
            theme.accent_running
        }
        PresentationBlock::Tool(tool)
            if mode != DisplayMode::Collapsed
                && !matches!(
                    tool.kind,
                    ToolKind::Read
                        | ToolKind::Edit
                        | ToolKind::List
                        | ToolKind::Search
                        | ToolKind::Todo
                ) =>
        {
            status_accent(tool.status, theme)
        }
        PresentationBlock::Subagent(subagent) if subagent.status == ToolStatus::Running => {
            theme.accent_running
        }
        PresentationBlock::User { .. }
        | PresentationBlock::Assistant { .. }
        | PresentationBlock::Thinking { .. }
        | PresentationBlock::Plan { .. }
        | PresentationBlock::Todo(_)
        | PresentationBlock::Tool(_)
        | PresentationBlock::Subagent(_)
        | PresentationBlock::System { .. } => return None,
    };
    Some(TranscriptAccent::Full(color))
}

fn status_accent(status: ToolStatus, theme: AstralTheme) -> Color {
    match status {
        ToolStatus::Running => theme.accent_running,
        ToolStatus::Success => Color::Green,
        ToolStatus::Failed => theme.accent_error,
        ToolStatus::Declined => Color::Yellow,
        ToolStatus::Interrupted => theme.gray_dim,
    }
}
