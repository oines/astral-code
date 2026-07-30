use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use std::ops::Range;

use super::AstralTheme;
use super::prompt_elements::PromptElementOverlay;
use super::prompt_selection::PromptSelectionOverlay;
use crate::PromptInputMode;
use crate::composer::ComposerElement;

pub(super) const PROMPT_PREFIX_WIDTH: u16 = 2;

pub(crate) struct StatusBar<'a> {
    pub(crate) left: Line<'a>,
    pub(crate) right: Option<Line<'a>>,
}

impl StatusBar<'_> {
    pub(crate) fn render(self, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
        if area.is_empty() {
            return;
        }
        buffer.set_style(area, Style::default().bg(theme.bg_base));
        buffer.set_line(area.x, area.y, &self.left, area.width);
        if let Some(right) = self.right {
            let width = u16::try_from(right.width()).unwrap_or(u16::MAX);
            if width < area.width {
                buffer.set_line(area.right().saturating_sub(width), area.y, &right, width);
            }
        }
    }
}

pub(crate) struct PromptChrome<'a> {
    pub(crate) text: &'a str,
    pub(crate) cursor_byte: usize,
    pub(crate) title: Option<&'a str>,
    pub(crate) model: &'a str,
    pub(crate) flags: &'a [&'a str],
    pub(crate) ghost: Option<&'a str>,
    pub(crate) focused: bool,
    pub(crate) input_mode: PromptInputMode,
    pub(crate) selection: Option<Range<usize>>,
    pub(crate) elements: &'a [ComposerElement],
}

impl PromptChrome<'_> {
    pub(crate) fn render(
        self,
        area: Rect,
        buffer: &mut Buffer,
        theme: AstralTheme,
    ) -> Option<Position> {
        if area.width < 4 || area.height < 3 {
            return None;
        }
        let bg = theme.bg_base;
        let accent = match self.input_mode {
            PromptInputMode::Normal => None,
            PromptInputMode::Shell => Some(theme.command),
        };
        let border = accent.unwrap_or(if self.focused {
            theme.prompt_border_active
        } else {
            theme.prompt_border
        });
        let prefix_style = Style::default()
            .fg(accent.unwrap_or(theme.text_secondary))
            .bg(bg);
        buffer.set_style(area, Style::default().fg(theme.text_primary).bg(bg));
        render_border_row(area, area.y, '╭', '╮', border, bg, buffer);
        render_border_row(area, area.bottom() - 1, '╰', '╯', border, bg, buffer);
        for y in area.y + 1..area.bottom() - 1 {
            set_border_cell(buffer, area.x, y, '│', border, bg);
            set_border_cell(buffer, area.right() - 1, y, '│', border, bg);
        }

        if let Some(title) = self.title.map(str::trim).filter(|title| !title.is_empty()) {
            let available = usize::from(area.width.saturating_sub(6));
            let title = truncate(title, available);
            let label = format!(" {title} ");
            let width = u16::try_from(Line::from(label.as_str()).width()).unwrap_or(u16::MAX);
            if width + 4 < area.width {
                buffer.set_string(
                    area.right().saturating_sub(width + 3),
                    area.y,
                    label,
                    Style::default().fg(theme.gray).bg(bg),
                );
            }
        }

        let content_x = area.x + 2;
        let content_width = area.width.saturating_sub(4);
        let layout = prompt_layout(self.text, self.cursor_byte, content_width);
        let visible_rows = usize::from(area.height.saturating_sub(2));
        let first_visible = layout
            .cursor_row
            .saturating_sub(visible_rows.saturating_sub(1));
        for (visible_row, (row, text)) in layout
            .rows
            .iter()
            .enumerate()
            .skip(first_visible)
            .take(visible_rows)
            .enumerate()
        {
            let y = area.y + 1 + u16::try_from(visible_row).unwrap_or(u16::MAX);
            if row == 0 {
                buffer.set_string(content_x, y, self.input_mode.prefix(), prefix_style);
            }
            let x = if row == 0 {
                content_x + PROMPT_PREFIX_WIDTH
            } else {
                content_x
            };
            let available = usize::from(area.right().saturating_sub(2).saturating_sub(x));
            buffer.set_string(
                x,
                y,
                truncate(text, available),
                Style::default().fg(theme.text_primary).bg(bg),
            );
        }
        PromptElementOverlay {
            text: self.text,
            elements: self.elements,
            rows: &layout.ranges,
            first_visible,
            visible_rows,
            prefix_width: PROMPT_PREFIX_WIDTH,
        }
        .render(area, buffer, theme);
        PromptSelectionOverlay {
            text: self.text,
            selection: self.selection,
            rows: &layout.ranges,
            first_visible,
            visible_rows,
            prefix_width: PROMPT_PREFIX_WIDTH,
        }
        .render(area, buffer, theme);
        if layout.rows.len() == 1
            && self.cursor_byte == self.text.len()
            && let Some(ghost) = self.ghost
        {
            let text_width = u16::try_from(Line::from(self.text).width()).unwrap_or(u16::MAX);
            let x = content_x + PROMPT_PREFIX_WIDTH + text_width;
            let available = usize::from(area.right().saturating_sub(2).saturating_sub(x));
            buffer.set_string(
                x,
                area.y + 1,
                truncate(ghost, available),
                Style::default().fg(theme.gray_dim).bg(bg),
            );
        }
        render_prompt_info(
            area,
            buffer,
            theme,
            self.model,
            self.flags,
            self.focused,
            accent,
        );

        let cursor_visible_row = layout.cursor_row.saturating_sub(first_visible);
        let prefix = u16::from(layout.cursor_row == 0) * PROMPT_PREFIX_WIDTH;
        let cursor_width = u16::try_from(layout.cursor_column).unwrap_or(u16::MAX);
        Some(Position::new(
            (content_x + prefix + cursor_width).min(area.right().saturating_sub(2)),
            area.y + 1 + u16::try_from(cursor_visible_row).unwrap_or(u16::MAX),
        ))
    }
}

pub(crate) fn prompt_height(text: &str, cursor_byte: usize, width: u16) -> u16 {
    let rows = prompt_layout(text, cursor_byte, width.saturating_sub(4))
        .rows
        .len();
    u16::try_from(rows)
        .unwrap_or(u16::MAX)
        .saturating_add(2)
        .clamp(3, 8)
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PromptLayout {
    pub(super) rows: Vec<String>,
    pub(super) ranges: Vec<Range<usize>>,
    pub(super) cursor_row: usize,
    pub(super) cursor_column: usize,
}

pub(super) fn prompt_layout(text: &str, cursor_byte: usize, width: u16) -> PromptLayout {
    let cursor_byte = cursor_byte.min(text.len());
    let width = usize::from(width).max(1);
    let ranges = prompt_ranges(text, width);
    let mut cursor = None;
    let rows = ranges
        .iter()
        .enumerate()
        .map(|(index, range)| {
            if cursor.is_none() {
                if cursor_byte >= range.start && cursor_byte <= range.end {
                    cursor = Some((index, Line::from(&text[range.start..cursor_byte]).width()));
                } else if cursor_byte < range.start {
                    cursor = Some((index, 0));
                }
            }
            text[range.clone()].to_string()
        })
        .collect::<Vec<_>>();
    let (cursor_row, cursor_column) = cursor.unwrap_or_else(|| {
        let index = ranges.len().saturating_sub(1);
        let range = &ranges[index];
        (index, Line::from(&text[range.clone()]).width())
    });
    PromptLayout {
        rows,
        ranges,
        cursor_row,
        cursor_column,
    }
}

fn prompt_ranges(text: &str, width: usize) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut logical_start = 0;
    for (newline, _) in text.match_indices('\n') {
        wrap_logical_line(text, logical_start, newline, width, &mut ranges);
        logical_start = newline + 1;
    }
    wrap_logical_line(text, logical_start, text.len(), width, &mut ranges);
    ranges
}

fn wrap_logical_line(
    text: &str,
    start: usize,
    end: usize,
    width: usize,
    ranges: &mut Vec<Range<usize>>,
) {
    if start == end {
        ranges.push(start..end);
        return;
    }

    let mut row_start = start;
    while row_start < end {
        let capacity = if ranges.is_empty() {
            width
                .saturating_sub(usize::from(PROMPT_PREFIX_WIDTH))
                .max(1)
        } else {
            width
        };
        let mut used = 0_usize;
        let mut overflow = None;
        let mut last_whitespace = None;
        for (offset, character) in text[row_start..end].char_indices() {
            let byte = row_start + offset;
            let character_width = Line::from(character.to_string()).width();
            if used.saturating_add(character_width) > capacity {
                overflow = Some(byte);
                break;
            }
            used = used.saturating_add(character_width);
            if character.is_whitespace() {
                last_whitespace = Some(byte);
            }
        }

        let Some(overflow) = overflow else {
            ranges.push(row_start..end);
            break;
        };
        if overflow == row_start {
            let next = row_start
                + text[row_start..end]
                    .chars()
                    .next()
                    .map_or(1, char::len_utf8);
            ranges.push(row_start..next);
            row_start = next;
            continue;
        }
        let break_at = last_whitespace
            .filter(|whitespace| *whitespace > row_start)
            .unwrap_or(overflow);
        ranges.push(row_start..break_at);
        row_start = if break_at == overflow {
            overflow
        } else {
            text[break_at..end]
                .char_indices()
                .find(|(_, character)| !character.is_whitespace())
                .map_or(end, |(offset, _)| break_at + offset)
        };
    }
}

pub(crate) struct ShortcutsBar<'a> {
    pub(crate) hints: &'a [(&'a str, &'a str)],
    pub(crate) right: Option<&'a str>,
    pub(crate) pending_confirmation: Option<ShortcutConfirmation<'a>>,
}

pub(crate) struct ShortcutConfirmation<'a> {
    pub(crate) shortcut: &'a str,
    pub(crate) label: &'a str,
}

impl ShortcutsBar<'_> {
    pub(crate) fn render(self, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
        if area.is_empty() {
            return;
        }
        buffer.set_style(area, Style::default().bg(theme.bg_base));
        let key_style = Style::default()
            .fg(theme.text_secondary)
            .bg(theme.bg_base)
            .add_modifier(Modifier::BOLD);
        let label_style = Style::default().fg(theme.gray).bg(theme.bg_base);
        let separator_style = label_style.add_modifier(Modifier::DIM);
        if let Some(pending) = self.pending_confirmation {
            let key_width = u16::try_from(Line::from(pending.shortcut).width()).unwrap_or(u16::MAX);
            buffer.set_stringn(
                area.x,
                area.y,
                pending.shortcut,
                usize::from(area.width),
                key_style,
            );
            let label_x = area.x.saturating_add(key_width);
            if label_x < area.right() {
                buffer.set_stringn(
                    label_x,
                    area.y,
                    format!(":{}", pending.label),
                    usize::from(area.right().saturating_sub(label_x)),
                    label_style,
                );
            }
            return;
        }
        let mut x = area.x;
        for (index, (key, label)) in self.hints.iter().enumerate() {
            let separator_width = if index > 0 { 5 } else { 0 };
            let key_width = u16::try_from(Line::from(*key).width()).unwrap_or(u16::MAX);
            let label_width = u16::try_from(Line::from(*label).width()).unwrap_or(u16::MAX);
            let hint_width = key_width.saturating_add(label_width).saturating_add(1);
            if x.saturating_add(separator_width).saturating_add(hint_width) >= area.right() {
                break;
            }
            if separator_width > 0 {
                let separator = Span::styled("  │  ", separator_style);
                buffer.set_span(x, area.y, &separator, separator_width);
                x += separator_width;
            }
            buffer.set_string(x, area.y, *key, key_style);
            x += key_width;
            buffer.set_string(x, area.y, ":", label_style);
            x += 1;
            buffer.set_string(x, area.y, *label, label_style);
            x += label_width;
        }
        if let Some(right) = self.right {
            let right_width = u16::try_from(Line::from(right).width()).unwrap_or(u16::MAX);
            if right_width < area.width {
                let right_x = area.right().saturating_sub(right_width);
                if right_x > x + 1 {
                    buffer.set_string(right_x, area.y, right, label_style);
                }
            }
        }
    }
}

fn render_border_row(
    area: Rect,
    y: u16,
    left: char,
    right: char,
    color: ratatui::style::Color,
    background: ratatui::style::Color,
    buffer: &mut Buffer,
) {
    for x in area.x..area.right() {
        let character = if x == area.x {
            left
        } else if x == area.right() - 1 {
            right
        } else {
            '─'
        };
        set_border_cell(buffer, x, y, character, color, background);
    }
}

fn set_border_cell(
    buffer: &mut Buffer,
    x: u16,
    y: u16,
    character: char,
    foreground: ratatui::style::Color,
    background: ratatui::style::Color,
) {
    if let Some(cell) = buffer.cell_mut((x, y)) {
        cell.set_char(character);
        cell.set_style(Style::default().fg(foreground).bg(background));
    }
}

fn render_prompt_info(
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
    model: &str,
    flags: &[&str],
    focused: bool,
    accent: Option<ratatui::style::Color>,
) {
    let mut parts = vec![model];
    parts.extend(flags.iter().copied().filter(|flag| !flag.is_empty()));
    let label = format!(" {} ", parts.join(" · "));
    let available = usize::from(area.width.saturating_sub(4));
    let label = truncate(&label, available);
    let width = u16::try_from(Line::from(label.as_str()).width()).unwrap_or(u16::MAX);
    if width + 2 >= area.width {
        return;
    }
    let color = accent.unwrap_or(if focused { theme.gray } else { theme.gray_dim });
    buffer.set_string(
        area.right().saturating_sub(width + 2),
        area.bottom() - 1,
        label,
        Style::default().fg(color).bg(theme.bg_base),
    );
}

fn truncate(text: &str, max_width: usize) -> String {
    let width = Line::from(text).width();
    if width <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    let target = max_width.saturating_sub(1);
    let mut result = String::new();
    for character in text.chars() {
        let candidate = format!("{result}{character}");
        if Line::from(candidate.as_str()).width() > target {
            break;
        }
        result.push(character);
    }
    result.push('…');
    result
}
