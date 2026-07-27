use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;

use super::AstralTheme;

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
    pub(crate) title: Option<&'a str>,
    pub(crate) model: &'a str,
    pub(crate) flags: &'a [&'a str],
    pub(crate) focused: bool,
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
        let border = if self.focused {
            theme.prompt_border_active
        } else {
            theme.prompt_border
        };
        buffer.set_style(area, Style::default().fg(theme.text_primary).bg(bg));
        render_border_row(area, area.y, '╭', '╮', border, buffer);
        render_border_row(area, area.bottom() - 1, '╰', '╯', border, buffer);
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
        let lines = self.text.split('\n').collect::<Vec<_>>();
        let visible_rows = usize::from(area.height.saturating_sub(2));
        for (row, text) in lines.iter().take(visible_rows).enumerate() {
            let y = area.y + 1 + u16::try_from(row).unwrap_or(u16::MAX);
            if row == 0 {
                buffer.set_string(
                    content_x,
                    y,
                    "❯ ",
                    Style::default().fg(theme.text_secondary).bg(bg),
                );
            }
            let x = if row == 0 { content_x + 2 } else { content_x };
            let available = usize::from(area.right().saturating_sub(2).saturating_sub(x));
            buffer.set_string(
                x,
                y,
                truncate(text, available),
                Style::default().fg(theme.text_primary).bg(bg),
            );
        }
        render_prompt_info(area, buffer, theme, self.model, self.flags, self.focused);

        let cursor_row = lines
            .len()
            .saturating_sub(1)
            .min(visible_rows.saturating_sub(1));
        let cursor_text = lines.get(cursor_row).copied().unwrap_or_default();
        let prefix = u16::from(cursor_row == 0) * 2;
        let cursor_width = u16::try_from(Line::from(cursor_text).width()).unwrap_or(u16::MAX);
        Some(Position::new(
            (content_x + prefix + cursor_width).min(area.right().saturating_sub(2)),
            area.y + 1 + u16::try_from(cursor_row).unwrap_or(u16::MAX),
        ))
    }
}

pub(crate) struct ShortcutsBar<'a> {
    pub(crate) hints: &'a [(&'a str, &'a str)],
    pub(crate) right: Option<&'a str>,
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
        let mut x = area.x;
        for (index, (key, label)) in self.hints.iter().enumerate() {
            if index > 0 {
                let separator = Span::styled("  │  ", separator_style);
                if x + 5 >= area.right() {
                    break;
                }
                buffer.set_span(x, area.y, &separator, 5);
                x += 5;
            }
            let key_width = u16::try_from(Line::from(*key).width()).unwrap_or(u16::MAX);
            let label_width = u16::try_from(Line::from(*label).width()).unwrap_or(u16::MAX);
            if x + key_width + label_width + 1 >= area.right() {
                break;
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
    buffer: &mut Buffer,
) {
    let background = AstralTheme::default().bg_base;
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
    let color = if focused { theme.gray } else { theme.gray_dim };
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
