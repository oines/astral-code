use std::collections::VecDeque;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use unicode_width::UnicodeWidthChar;

use super::AstralTheme;
use crate::prompt_queue::QueuedPrompt;

const MAX_QUEUE_HEIGHT: u16 = 3;

pub(crate) struct QueuePane<'a> {
    pub(crate) entries: &'a VecDeque<QueuedPrompt>,
    pub(crate) selected_id: Option<u64>,
    pub(crate) focused: bool,
}

impl QueuePane<'_> {
    pub(crate) fn height(&self) -> u16 {
        u16::try_from(self.entries.len())
            .unwrap_or(u16::MAX)
            .min(MAX_QUEUE_HEIGHT)
    }

    pub(crate) fn render(self, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
        if area.is_empty() {
            return;
        }
        buffer.set_style(area, Style::default().bg(theme.bg_base));
        let visible_rows = usize::from(area.height);
        let selected = self
            .selected_id
            .and_then(|id| self.entries.iter().position(|entry| entry.id() == id))
            .unwrap_or_default();
        let first = selected
            .saturating_sub(visible_rows.saturating_sub(1))
            .min(self.entries.len().saturating_sub(visible_rows));
        for (row, (position, entry)) in self.entries.iter().enumerate().skip(first).enumerate() {
            let prefix = format!("#{} ", position + 1);
            let suffix = multiline_suffix(entry.text());
            let available = usize::from(area.width)
                .saturating_sub(Line::from(prefix.as_str()).width())
                .saturating_sub(Line::from(suffix.as_str()).width());
            let text = truncate(
                entry
                    .text()
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or(""),
                available,
            );
            let mut line: Line<'static> = vec![
                prefix.fg(theme.gray),
                text.fg(theme.text_secondary),
                suffix.fg(theme.gray_dim),
            ]
            .into();
            if self.focused && self.selected_id == Some(entry.id()) {
                line = line.patch_style(Style::default().bg(theme.panel_selected));
            }
            buffer.set_line(
                area.x,
                area.y + u16::try_from(row).unwrap_or(u16::MAX),
                &line,
                area.width,
            );
        }
    }
}

fn multiline_suffix(text: &str) -> String {
    let extra = text.lines().count().saturating_sub(1);
    match extra {
        0 => String::new(),
        1 => " (+1 line)".to_string(),
        _ => format!(" (+{extra} lines)"),
    }
}

fn truncate(text: &str, max_width: usize) -> String {
    let text = text.trim();
    if text
        .chars()
        .filter_map(UnicodeWidthChar::width)
        .sum::<usize>()
        <= max_width
    {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    let target = max_width.saturating_sub(1);
    let mut width = 0;
    let mut result = String::new();
    for character in text.chars() {
        let character_width = character.width().unwrap_or_default();
        if width + character_width > target {
            break;
        }
        result.push(character);
        width += character_width;
    }
    result.push('…');
    result
}
