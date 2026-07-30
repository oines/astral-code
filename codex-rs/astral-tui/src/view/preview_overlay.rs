//! Multiline prompt-element preview.
//!
//! The geometry and first/last-line truncation follow Grok Build's
//! `preview_overlay.rs` at commit `47348d13ec4508dcfe440e34c6d511bb02998fb2`
//! (Apache-2.0). Astral supplies its own theme and composer state.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::AstralTheme;

const PREVIEW_LINES: usize = 3;
const MIN_WIDTH: u16 = 20;
const MIN_HEIGHT: u16 = 5;

pub(crate) struct PreviewOverlay<'a> {
    pub(crate) content: &'a str,
    pub(crate) hint: Option<Line<'static>>,
}

impl PreviewOverlay<'_> {
    pub(crate) fn render(
        self,
        area: Rect,
        buffer: &mut Buffer,
        theme: AstralTheme,
    ) -> Option<Rect> {
        let lines = self.content.lines().collect::<Vec<_>>();
        if lines.is_empty() || area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            return None;
        }

        let needs_separator = lines.len() > PREVIEW_LINES * 2;
        let content_height = if needs_separator {
            PREVIEW_LINES * 2 + 1
        } else {
            lines.len()
        };
        let height = u16::try_from(content_height)
            .unwrap_or(u16::MAX)
            .saturating_add(2)
            .min(area.height);
        let width = ((area.width as f32) * 0.75) as u16;
        let overlay = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.bottom().saturating_sub(height),
            width,
            height,
        );

        Clear.render(overlay, buffer);
        buffer.set_style(overlay, Style::default().bg(theme.panel_selected));
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.gray).bg(theme.panel_selected))
            .style(Style::default().bg(theme.panel_selected));
        let inner = block.inner(overlay);
        block.render(overlay, buffer);

        render_content(
            buffer,
            inner,
            &lines,
            needs_separator,
            Style::default()
                .fg(theme.text_primary)
                .bg(theme.panel_selected),
            Style::default().fg(theme.gray).bg(theme.panel_selected),
        );
        if let Some(hint) = self.hint {
            render_hint(buffer, overlay, hint, theme.panel_selected);
        }
        Some(overlay)
    }
}

fn render_content(
    buffer: &mut Buffer,
    area: Rect,
    lines: &[&str],
    needs_separator: bool,
    text_style: Style,
    separator_style: Style,
) {
    let mut row = 0;
    let mut render_line = |text: &str, style: Style| {
        if row >= area.height {
            return;
        }
        let text = truncate(text, usize::from(area.width));
        buffer.set_line(
            area.x,
            area.y + row,
            &Line::from(Span::styled(text, style)),
            area.width,
        );
        row = row.saturating_add(1);
    };

    if needs_separator {
        for line in lines.iter().take(PREVIEW_LINES) {
            render_line(line, text_style);
        }
        let omitted = lines.len().saturating_sub(PREVIEW_LINES * 2);
        render_line(&format!("⋮ ({omitted} more lines)"), separator_style);
        for line in lines.iter().skip(lines.len().saturating_sub(PREVIEW_LINES)) {
            render_line(line, text_style);
        }
    } else {
        for line in lines {
            render_line(line, text_style);
        }
    }
}

fn render_hint(
    buffer: &mut Buffer,
    area: Rect,
    hint: Line<'static>,
    background: ratatui::style::Color,
) {
    const CHROME: u16 = 6;
    const MIN_TEXT_WIDTH: u16 = 8;
    let width = area.width.saturating_sub(CHROME);
    if width < MIN_TEXT_WIDTH {
        return;
    }

    let mut hint = truncate_line(hint, usize::from(width));
    for span in &mut hint.spans {
        span.style = span.style.bg(background);
    }
    let pad = Span::styled(" ", Style::default().bg(background));
    let mut spans = vec![pad.clone()];
    spans.append(&mut hint.spans);
    spans.push(pad);
    buffer.set_line(
        area.x + 2,
        area.bottom().saturating_sub(1),
        &Line::from(spans),
        area.width.saturating_sub(4),
    );
}

fn truncate(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }
    let budget = max_width.saturating_sub(1);
    let mut output = String::new();
    let mut width = 0usize;
    for grapheme in text.graphemes(true) {
        let next = width.saturating_add(UnicodeWidthStr::width(grapheme));
        if next > budget {
            break;
        }
        output.push_str(grapheme);
        width = next;
    }
    output.push('…');
    output
}

fn truncate_line(line: Line<'static>, max_width: usize) -> Line<'static> {
    if line.width() <= max_width {
        return line;
    }
    let budget = max_width.saturating_sub(1);
    let mut width = 0usize;
    let mut output = Vec::new();
    for span in line.spans {
        let remaining = budget.saturating_sub(width);
        if remaining == 0 {
            break;
        }
        let text = truncate_without_ellipsis(span.content.as_ref(), remaining);
        width = width.saturating_add(UnicodeWidthStr::width(text.as_str()));
        if !text.is_empty() {
            output.push(Span::styled(text, span.style));
        }
        if width >= budget {
            break;
        }
    }
    let style = output.last().map_or_else(Style::default, |span| span.style);
    output.push(Span::styled("…", style));
    Line::from(output)
}

fn truncate_without_ellipsis(text: &str, max_width: usize) -> String {
    let mut output = String::new();
    let mut width = 0usize;
    for grapheme in text.graphemes(true) {
        let next = width.saturating_add(UnicodeWidthStr::width(grapheme));
        if next > max_width {
            break;
        }
        output.push_str(grapheme);
        width = next;
    }
    output
}
