//! Prompt queue presentation and last-frame pointer geometry.
//!
//! The row/action model follows Grok Build's `views/queue_pane.rs` at
//! `47348d13ec4508dcfe440e34c6d511bb02998fb2`. Queue ownership and mutations
//! remain in Astral's TUI state; this module only renders rows and reports hits.

use std::collections::VecDeque;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use unicode_width::UnicodeWidthChar;

use super::AstralTheme;
use crate::prompt_queue::QueuedPrompt;

const MAX_QUEUE_HEIGHT: u16 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueuePaneAction {
    SendNow,
    Edit,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QueuePaneHover {
    pub(crate) id: u64,
    pub(crate) action: Option<QueuePaneAction>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct QueuePaneFrame {
    area: Rect,
    rows: Vec<(u64, Rect)>,
    actions: Vec<(u64, QueuePaneAction, Rect)>,
}

impl QueuePaneFrame {
    pub(crate) fn contains(&self, column: u16, row: u16) -> bool {
        self.area.contains((column, row).into())
    }

    pub(crate) fn hit(&self, column: u16, row: u16) -> Option<QueuePaneHover> {
        self.actions
            .iter()
            .find(|(_, _, rect)| rect.contains((column, row).into()))
            .map(|(id, action, _)| QueuePaneHover {
                id: *id,
                action: Some(*action),
            })
            .or_else(|| {
                self.rows
                    .iter()
                    .find(|(_, rect)| rect.contains((column, row).into()))
                    .map(|(id, _)| QueuePaneHover {
                        id: *id,
                        action: None,
                    })
            })
    }

    pub(crate) fn contains_id(&self, id: u64) -> bool {
        self.rows.iter().any(|(row_id, _)| *row_id == id)
    }

    fn observe_row(&mut self, id: u64, rect: Rect) {
        self.rows.push((id, rect));
    }

    fn observe_action(&mut self, id: u64, action: QueuePaneAction, rect: Rect) {
        self.actions.push((id, action, rect));
    }
}

pub(crate) struct QueuePane<'a> {
    pub(crate) entries: &'a VecDeque<QueuedPrompt>,
    pub(crate) selected_id: Option<u64>,
    pub(crate) focused: bool,
    pub(crate) hovered: Option<QueuePaneHover>,
    pub(crate) turn_running: bool,
}

impl QueuePane<'_> {
    pub(crate) fn height(&self) -> u16 {
        u16::try_from(self.entries.len())
            .unwrap_or(u16::MAX)
            .min(MAX_QUEUE_HEIGHT)
    }

    pub(crate) fn render(
        self,
        area: Rect,
        buffer: &mut Buffer,
        theme: AstralTheme,
    ) -> QueuePaneFrame {
        let mut frame = QueuePaneFrame {
            area,
            ..QueuePaneFrame::default()
        };
        if area.is_empty() {
            return frame;
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
            let y = area
                .y
                .saturating_add(u16::try_from(row).unwrap_or(u16::MAX));
            if y >= area.bottom() {
                break;
            }
            let row_rect = Rect::new(area.x, y, area.width, 1);
            frame.observe_row(entry.id(), row_rect);
            let selected = self.focused && self.selected_id == Some(entry.id());
            let hovered = self.hovered.is_some_and(|hover| hover.id == entry.id());
            let row_background = if selected {
                theme.panel_selected
            } else if hovered {
                theme.panel_background
            } else {
                theme.bg_base
            };
            let mut buttons = Vec::new();
            if selected || hovered {
                let mut right = area.right();
                let actions = [
                    (QueuePaneAction::Delete, "[cancel]", true),
                    (QueuePaneAction::Edit, "[edit]", true),
                    (QueuePaneAction::SendNow, "[Send now]", self.turn_running),
                ];
                for (action, label, visible) in actions {
                    if !visible {
                        continue;
                    }
                    let width = u16::try_from(label.len()).unwrap_or(u16::MAX);
                    let Some(x) = right.checked_sub(width).filter(|x| *x >= area.x) else {
                        break;
                    };
                    buttons.push((action, label, Rect::new(x, y, width, 1)));
                    right = x;
                }
            }
            let reserved = buttons.iter().map(|(_, _, rect)| rect.width).sum::<u16>();
            let prefix = format!("#{} ", position + 1);
            let suffix = multiline_suffix(entry.text());
            let available = usize::from(area.width.saturating_sub(reserved))
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
            let line: Line<'static> = vec![
                prefix.fg(theme.gray),
                text.fg(theme.text_secondary),
                suffix.fg(theme.gray_dim),
            ]
            .into();
            buffer.set_line(area.x, y, &line, area.width.saturating_sub(reserved));
            buffer.set_style(row_rect, Style::default().bg(row_background));
            for (action, label, rect) in buttons {
                let hovered_action = self
                    .hovered
                    .is_some_and(|hover| hover.id == entry.id() && hover.action == Some(action));
                let foreground = action_color(action, hovered_action, theme);
                buffer.set_string(
                    rect.x,
                    rect.y,
                    label,
                    Style::default().fg(foreground).bg(row_background),
                );
                frame.observe_action(entry.id(), action, rect);
            }
        }
        frame
    }
}

fn action_color(action: QueuePaneAction, hovered: bool, theme: AstralTheme) -> Color {
    if !hovered {
        return theme.gray;
    }
    match action {
        QueuePaneAction::Delete => theme.accent_error,
        QueuePaneAction::SendNow | QueuePaneAction::Edit => theme.text_primary,
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
