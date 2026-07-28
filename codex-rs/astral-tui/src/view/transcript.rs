// Derived from Grok Build's user, thinking, and turn-completion block
// presentation at commit 47348d13ec4508dcfe440e34c6d511bb02998fb2
// (Apache-2.0). Modified for Astral app-server turn and item metadata.

use astral_tui_scrollback::DisplayMode;
use astral_tui_scrollback::LineJoiner;
use astral_tui_scrollback::MarkdownStyle;
use astral_tui_scrollback::MarkdownSyntaxTheme;
use astral_tui_scrollback::PresentationBlock;
use astral_tui_scrollback::RenderOptions;
use astral_tui_scrollback::render_block;
use astral_tui_scrollback::render_markdown_with_metadata;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use std::ops::Range;

use crate::CommittedBlock;
use crate::conversation::TranscriptBlock;
use crate::conversation::TranscriptTurn;

use super::AstralTheme;
use super::AstralThemeId;
use super::EntryDisplayState;
use super::EntryGroupSpan;
use super::entry_group::scan_turn;
use super::entry_state::entry_id;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptSection {
    pub(crate) item_id: String,
    pub(crate) lines: Range<usize>,
    pub(crate) kind: TranscriptSectionKind,
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
}

pub(crate) fn render_transcript(
    turns: &[TranscriptTurn],
    width: u16,
    theme: AstralTheme,
    display: &EntryDisplayState,
) -> TranscriptLayout {
    let mut lines = Vec::new();
    let mut sections = Vec::new();
    let mut selectable_ranges = Vec::new();
    for turn in turns {
        let groups = scan_turn(turn, display);
        for (index, block) in turn.blocks.iter().enumerate() {
            if let Some(group) = groups.iter().find(|group| group.range.start == index) {
                render_group_section(
                    &mut lines,
                    &mut sections,
                    &mut selectable_ranges,
                    group,
                    width,
                    theme,
                    display,
                );
            }
            if groups.iter().any(|group| group.hides(index)) {
                continue;
            }
            let start = lines.len();
            let mut selectable_lines = Vec::new();
            let item_id = entry_id(&turn.id, &block.item_id);
            let mode = display.mode_for(&turn.id, &block.item_id, &block.block);
            render_turn_block(&mut lines, &mut selectable_lines, block, width, theme, mode);
            if display.selected_id() == Some(item_id.as_str()) {
                highlight_selected_header(&mut lines, start, width, theme, mode);
            }
            sections.push(TranscriptSection {
                item_id,
                lines: start..lines.len(),
                kind: TranscriptSectionKind::Entry,
            });
            selectable_ranges.push(TranscriptSelectableRange {
                lines: selectable_lines,
            });
        }
        if let Some(duration_ms) = turn_duration_ms(turn) {
            let line = Line::from(format!("Worked for {}", format_duration(duration_ms)).dim());
            let columns = selectable_columns(&line, width);
            let line_index = lines.len();
            lines.push(line);
            if let Some(section) = sections.last_mut() {
                section.lines.end = lines.len();
            }
            if let Some(selectable_range) = selectable_ranges.last_mut() {
                selectable_range.lines.push(TranscriptSelectableLine {
                    line: line_index,
                    columns,
                    joiner_to_previous: LineJoiner::HardBreak,
                });
            }
        }
    }
    TranscriptLayout {
        lines,
        sections,
        selectable_ranges,
    }
}

fn render_group_section(
    lines: &mut Vec<Line<'static>>,
    sections: &mut Vec<TranscriptSection>,
    selectable_ranges: &mut Vec<TranscriptSelectableRange>,
    group: &EntryGroupSpan,
    width: u16,
    theme: AstralTheme,
    display: &EntryDisplayState,
) {
    let start = lines.len();
    let color = if group.failed {
        theme.accent_error
    } else if group.running {
        theme.accent_running
    } else {
        theme.gray
    };
    let line: Line<'static> = vec!["◈ ".fg(color), group.label.clone().bold().fg(color)].into();
    let columns = selectable_columns(&line, width);
    lines.push(line);
    if group.header_owns_selection() && display.selected_id() == Some(group.id.as_str()) {
        let mode = if group.expanded {
            DisplayMode::Expanded
        } else {
            DisplayMode::Collapsed
        };
        highlight_selected_header(lines, start, width, theme, mode);
    }
    sections.push(TranscriptSection {
        item_id: group.id.clone(),
        lines: start..lines.len(),
        kind: TranscriptSectionKind::GroupHeader,
    });
    selectable_ranges.push(TranscriptSelectableRange {
        lines: vec![TranscriptSelectableLine {
            line: start,
            columns,
            joiner_to_previous: LineJoiner::HardBreak,
        }],
    });
}

pub(crate) fn render_committed_block(
    committed: &CommittedBlock,
    width: u16,
    theme: AstralTheme,
) -> Vec<Line<'static>> {
    let turn = TranscriptTurn {
        id: committed.turn_id.clone(),
        blocks: vec![TranscriptBlock {
            item_id: committed.item_id.clone(),
            block: committed.block.clone(),
            started_at_ms: committed.started_at_ms,
            completed_at_ms: committed.completed_at_ms,
        }],
        started_at_ms: committed.turn_started_at_ms,
        completed_at_ms: committed.turn_completed_at_ms,
        duration_ms: committed.turn_duration_ms,
    };
    let mut lines = Vec::new();
    let mut selectable_lines = Vec::new();
    let mode = turn.blocks[0].block.default_display_mode();
    render_turn_block(
        &mut lines,
        &mut selectable_lines,
        &turn.blocks[0],
        width,
        theme,
        mode,
    );
    if committed.ends_turn
        && let Some(duration_ms) = turn_duration_ms(&turn)
    {
        lines.push(
            format!("Worked for {}", format_duration(duration_ms))
                .dim()
                .into(),
        );
    }
    lines
}

fn render_turn_block(
    lines: &mut Vec<Line<'static>>,
    selectable_lines: &mut Vec<TranscriptSelectableLine>,
    block: &TranscriptBlock,
    width: u16,
    theme: AstralTheme,
    mode: DisplayMode,
) {
    match &block.block {
        PresentationBlock::User { .. } => {
            let rendered = render_block(&block.block, RenderOptions::for_mode(width, mode));
            push_transcript_line(
                lines,
                selectable_lines,
                band_line(Line::default(), width, theme.panel_selected),
                0..0,
                LineJoiner::HardBreak,
            );
            for line in rendered.lines {
                let columns = selectable_columns(&line, width);
                let line = band_line(line, width, theme.panel_selected);
                push_transcript_line(
                    lines,
                    selectable_lines,
                    line,
                    columns,
                    LineJoiner::HardBreak,
                );
            }
            push_transcript_line(
                lines,
                selectable_lines,
                band_line(Line::default(), width, theme.panel_selected),
                0..0,
                LineJoiner::HardBreak,
            );
        }
        PresentationBlock::Thinking { running, .. } => {
            let duration_ms = item_duration_ms(block);
            let label = if *running {
                "Thinking…".to_string()
            } else {
                duration_ms.map_or_else(
                    || "Thought".to_string(),
                    |duration_ms| format!("Thought for {}", format_duration(duration_ms)),
                )
            };
            let marker = if *running { "◇ " } else { "◆ " };
            let color = if *running {
                theme.accent_running
            } else {
                theme.gray
            };
            let line = vec![marker.fg(color), label.bold().fg(color)].into();
            let columns = selectable_columns(&line, width);
            push_transcript_line(
                lines,
                selectable_lines,
                line,
                columns,
                LineJoiner::HardBreak,
            );
            if mode != DisplayMode::Collapsed {
                let rendered = render_block(&block.block, RenderOptions::for_mode(width, mode));
                for line in rendered.lines.into_iter().skip(1) {
                    let columns = selectable_columns(&line, width);
                    push_transcript_line(
                        lines,
                        selectable_lines,
                        line,
                        columns,
                        LineJoiner::HardBreak,
                    );
                }
            }
        }
        PresentationBlock::Assistant { text } => {
            let rendered = render_markdown_with_metadata(text, width, markdown_style(theme));
            for rendered_line in rendered {
                let line = rendered_line.line;
                let columns = selectable_columns(&line, width);
                push_transcript_line(
                    lines,
                    selectable_lines,
                    line,
                    columns,
                    rendered_line.joiner_to_previous,
                );
            }
        }
        _ => {
            for line in render_block(&block.block, RenderOptions::for_mode(width, mode)).lines {
                let columns = selectable_columns(&line, width);
                push_transcript_line(
                    lines,
                    selectable_lines,
                    line,
                    columns,
                    LineJoiner::HardBreak,
                );
            }
        }
    }
}

fn highlight_selected_header(
    lines: &mut [Line<'static>],
    start: usize,
    width: u16,
    theme: AstralTheme,
    mode: DisplayMode,
) {
    let Some(line) = lines.get_mut(start) else {
        return;
    };
    if let Some(marker) = line.spans.first_mut() {
        let content = marker.content.to_string();
        let mut chars = content.chars();
        if chars.next().is_some() {
            let indicator = if mode == DisplayMode::Collapsed {
                "›"
            } else {
                "⌄"
            };
            marker.content = format!("{indicator}{}", chars.as_str()).into();
        }
    }
    let line = std::mem::take(line);
    lines[start] = band_line(line, width, theme.panel_selected);
}

fn selectable_columns(line: &Line<'_>, width: u16) -> Range<u16> {
    let text = line.to_string();
    0..u16::try_from(Line::from(text.trim_end()).width())
        .unwrap_or(u16::MAX)
        .min(width)
}

fn push_transcript_line(
    lines: &mut Vec<Line<'static>>,
    selectable_lines: &mut Vec<TranscriptSelectableLine>,
    line: Line<'static>,
    columns: Range<u16>,
    joiner_to_previous: LineJoiner,
) {
    selectable_lines.push(TranscriptSelectableLine {
        line: lines.len(),
        columns,
        joiner_to_previous,
    });
    lines.push(line);
}

fn markdown_style(theme: AstralTheme) -> MarkdownStyle {
    let primary = Style::default().fg(theme.text_primary);
    let secondary = Style::default().fg(theme.text_secondary);
    let gray = Style::default().fg(theme.gray);
    MarkdownStyle {
        text: primary,
        headings: [
            primary.add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            primary.add_modifier(Modifier::BOLD),
            primary.add_modifier(Modifier::BOLD | Modifier::ITALIC),
            secondary.add_modifier(Modifier::BOLD),
            secondary.add_modifier(Modifier::ITALIC),
            secondary.add_modifier(Modifier::ITALIC),
        ],
        strong: primary.add_modifier(Modifier::BOLD),
        emphasis: primary.add_modifier(Modifier::ITALIC),
        strikethrough: secondary.add_modifier(Modifier::CROSSED_OUT),
        inline_code: Style::default()
            .fg(theme.accent_running)
            .add_modifier(Modifier::BOLD),
        blockquote: gray,
        list_marker: gray,
        task_checked: Style::default().fg(theme.accent_running),
        task_unchecked: gray,
        rule: Style::default().fg(theme.gray_dim),
        link_text: Style::default()
            .fg(theme.accent_running)
            .add_modifier(Modifier::UNDERLINED),
        link_url: gray,
        code: secondary,
        code_background: Style::default().bg(theme.panel_background),
        syntax_theme: if theme == AstralTheme::for_id(AstralThemeId::Day) {
            MarkdownSyntaxTheme::Day
        } else if theme == AstralTheme::for_id(AstralThemeId::Terminal) {
            MarkdownSyntaxTheme::Terminal
        } else {
            MarkdownSyntaxTheme::Night
        },
        table_border: Style::default().fg(theme.gray_dim),
        table_header: primary.add_modifier(Modifier::BOLD),
    }
}

fn band_line(
    mut line: Line<'static>,
    width: u16,
    background: ratatui::style::Color,
) -> Line<'static> {
    for span in &mut line.spans {
        span.style = span.style.bg(background);
    }
    let padding = usize::from(width).saturating_sub(line.width());
    line.spans.push(styled_padding(padding, Some(background)));
    line
}

fn styled_padding(width: usize, background: Option<ratatui::style::Color>) -> Span<'static> {
    let mut style = Style::default();
    if let Some(background) = background {
        style = style.bg(background);
    }
    Span::styled(" ".repeat(width), style)
}

fn item_duration_ms(block: &TranscriptBlock) -> Option<i64> {
    block
        .completed_at_ms
        .zip(block.started_at_ms)
        .map(|(completed, started)| completed.saturating_sub(started))
        .filter(|duration| *duration >= 0)
}

fn turn_duration_ms(turn: &TranscriptTurn) -> Option<i64> {
    turn.duration_ms.or_else(|| {
        turn.completed_at_ms
            .zip(turn.started_at_ms)
            .map(|(completed, started)| completed.saturating_sub(started))
            .filter(|duration| *duration >= 0)
    })
}

fn format_duration(duration_ms: i64) -> String {
    let seconds = duration_ms.max(0) as f64 / 1_000.0;
    if seconds < 60.0 {
        format!("{seconds:.1}s")
    } else {
        let minutes = (seconds / 60.0).floor() as u64;
        let remaining = seconds - minutes as f64 * 60.0;
        format!("{minutes}m{remaining:.0}s")
    }
}

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;
