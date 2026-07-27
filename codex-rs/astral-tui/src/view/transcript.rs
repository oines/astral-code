// Derived from Grok Build's user, thinking, and turn-completion block
// presentation at commit 47348d13ec4508dcfe440e34c6d511bb02998fb2
// (Apache-2.0). Modified for Astral app-server turn and item metadata.

use astral_tui_scrollback::MarkdownStyle;
use astral_tui_scrollback::MarkdownSyntaxTheme;
use astral_tui_scrollback::PresentationBlock;
use astral_tui_scrollback::RenderOptions;
use astral_tui_scrollback::render_block;
use astral_tui_scrollback::render_markdown;
use chrono::Local;
use chrono::TimeZone;
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

const TIMESTAMP_WIDTH: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptSection {
    pub(crate) item_id: String,
    pub(crate) lines: Range<usize>,
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
}

impl TranscriptLayout {
    pub(crate) fn section(&self, item_id: &str) -> Option<&TranscriptSection> {
        self.sections
            .iter()
            .find(|section| section.item_id == item_id)
    }
}

pub(crate) fn render_transcript(
    turns: &[TranscriptTurn],
    width: u16,
    theme: AstralTheme,
) -> TranscriptLayout {
    let mut lines = Vec::new();
    let mut sections = Vec::new();
    for turn in turns {
        for (index, block) in turn.blocks.iter().enumerate() {
            let start = lines.len();
            if index == 0 && !lines.is_empty() {
                lines.push(Line::default());
            }
            render_turn_block(&mut lines, block, turn, width, theme);
            sections.push(TranscriptSection {
                item_id: section_id(&turn.id, &block.item_id),
                lines: start..lines.len(),
            });
        }
        if let Some(duration_ms) = turn_duration_ms(turn) {
            lines.push(
                format!("Worked for {}", format_duration(duration_ms))
                    .dim()
                    .into(),
            );
            if let Some(section) = sections.last_mut() {
                section.lines.end = lines.len();
            }
        }
    }
    TranscriptLayout { lines, sections }
}

fn section_id(turn_id: &str, item_id: &str) -> String {
    format!("{turn_id}\0{item_id}")
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
    render_turn_block(&mut lines, &turn.blocks[0], &turn, width, theme);
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
    block: &TranscriptBlock,
    turn: &TranscriptTurn,
    width: u16,
    theme: AstralTheme,
) {
    match &block.block {
        PresentationBlock::User { .. } => {
            if !lines.is_empty() {
                lines.push(Line::default());
            }
            let timestamp = turn.started_at_ms.and_then(format_timestamp);
            let content_width = reserved_content_width(width, timestamp.as_deref());
            let rendered = render_block(&block.block, RenderOptions::compact(content_width));
            lines.push(band_line(Line::default(), width, theme.panel_selected));
            lines.extend(rendered.lines.into_iter().map(|line| {
                timestamped_line(
                    line,
                    timestamp.as_deref(),
                    width,
                    Some(theme.panel_selected),
                )
            }));
            lines.push(band_line(Line::default(), width, theme.panel_selected));
        }
        PresentationBlock::Thinking { running, .. } => {
            if !lines.is_empty() {
                lines.push(Line::default());
            }
            let duration_ms = item_duration_ms(block);
            let label = if *running {
                "Thinking…".to_string()
            } else {
                duration_ms.map_or_else(
                    || "Thought".to_string(),
                    |duration_ms| format!("Thought for {}", format_duration(duration_ms)),
                )
            };
            lines.push(vec!["◆ ".fg(theme.gray), label.bold().fg(theme.gray)].into());
            if *running {
                let rendered = render_block(&block.block, RenderOptions::compact(width));
                lines.extend(rendered.lines.into_iter().skip(1));
            }
        }
        PresentationBlock::Assistant { text } => {
            if !lines.is_empty() {
                lines.push(Line::default());
            }
            let timestamp = block
                .completed_at_ms
                .or(turn.completed_at_ms)
                .and_then(format_timestamp);
            let content_width = reserved_content_width(width, timestamp.as_deref());
            let rendered = render_markdown(text, content_width, markdown_style(theme));
            for (index, line) in rendered.into_iter().enumerate() {
                lines.push(timestamped_line(
                    line,
                    if index == 0 {
                        timestamp.as_deref()
                    } else {
                        None
                    },
                    width,
                    None,
                ));
            }
        }
        _ => {
            if !lines.is_empty() {
                lines.push(Line::default());
            }
            lines.extend(render_block(&block.block, RenderOptions::compact(width)).lines);
        }
    }
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

fn timestamped_line(
    mut line: Line<'static>,
    timestamp: Option<&str>,
    width: u16,
    background: Option<ratatui::style::Color>,
) -> Line<'static> {
    let width = usize::from(width);
    let timestamp_width = timestamp.map_or(0, |timestamp| Line::from(timestamp).width());
    let padding = width
        .saturating_sub(line.width())
        .saturating_sub(timestamp_width);
    line.spans.push(styled_padding(padding, background));
    if let Some(timestamp) = timestamp {
        let mut span = timestamp.to_string().dim();
        if let Some(background) = background {
            span = span.bg(background);
        }
        line.spans.push(span);
    }
    if let Some(background) = background {
        band_line(line, width as u16, background)
    } else {
        line
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

fn reserved_content_width(width: u16, timestamp: Option<&str>) -> u16 {
    if timestamp.is_some() {
        width.saturating_sub(TIMESTAMP_WIDTH as u16 + 2).max(1)
    } else {
        width.max(1)
    }
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

fn format_timestamp(timestamp_ms: i64) -> Option<String> {
    Local
        .timestamp_millis_opt(timestamp_ms)
        .single()
        .map(|timestamp| timestamp.format("%-I:%M %p").to_string())
}

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;
