//! Shared Astral modal chrome.
//!
//! The frame hierarchy is ported from Grok Build's `views/modal_window.rs` at
//! commit `47348d13ec4508dcfe440e34c6d511bb02998fb2` (Apache-2.0).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;

use crate::modal::ModalState;

use super::AstralTheme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalHeight {
    Adaptive,
    MinimumContent(u16),
    FullViewport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModalFrame {
    pub(crate) popup: Rect,
    pub(crate) content: Rect,
    pub(crate) close_button: Rect,
}

pub(crate) struct InfoModal<'a> {
    pub(crate) state: &'a ModalState,
}

impl InfoModal<'_> {
    pub(crate) fn render(self, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
        let footer = if self.state.rows.len() > 10 {
            "↑/↓ scroll · Esc close"
        } else {
            "Esc close"
        };
        let Some(content) = render_modal_frame(
            area,
            buffer,
            theme,
            &self.state.title,
            footer,
            ModalHeight::Adaptive,
        ) else {
            return;
        };
        let label_width = self
            .state
            .rows
            .iter()
            .map(|row| Line::from(row.label.as_str()).width())
            .max()
            .unwrap_or_default();
        for (index, row) in self
            .state
            .rows
            .iter()
            .skip(self.state.scroll_offset)
            .take(usize::from(content.height))
            .enumerate()
        {
            let y = content.y + u16::try_from(index).unwrap_or(u16::MAX);
            buffer.set_stringn(
                content.x,
                y,
                &row.label,
                usize::from(content.width),
                Style::default().fg(theme.gray).bg(theme.bg_base),
            );
            let value_x = content.x + u16::try_from(label_width).unwrap_or(u16::MAX) + 2;
            if value_x < content.right() {
                buffer.set_stringn(
                    value_x,
                    y,
                    &row.value,
                    usize::from(content.right().saturating_sub(value_x)),
                    Style::default().fg(theme.text_primary).bg(theme.bg_base),
                );
            }
        }
    }
}

pub(crate) fn render_modal_frame(
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
    title: &str,
    footer: &str,
    height: ModalHeight,
) -> Option<Rect> {
    render_modal_frame_with_geometry(area, buffer, theme, title, footer, height)
        .map(|frame| frame.content)
}

pub(crate) fn render_modal_frame_with_geometry(
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
    title: &str,
    footer: &str,
    height: ModalHeight,
) -> Option<ModalFrame> {
    let popup = popup_area(area, height)?;
    Clear.render(popup, buffer);
    buffer.set_style(popup, Style::default().bg(theme.bg_base));
    let border = Style::default().fg(theme.gray_dim).bg(theme.bg_base);
    let title = Line::from(vec![
        "─ ".fg(theme.gray_dim),
        title.to_string().bold(),
        " ─".fg(theme.gray_dim),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border)
        .style(Style::default().fg(theme.text_primary).bg(theme.bg_base))
        .title(title);
    let inner = block.inner(popup);
    block.render(popup, buffer);

    let close_button = Rect::new(popup.right().saturating_sub(5), popup.y, 3, 1);
    buffer.set_string(
        close_button.x,
        close_button.y,
        "[×]",
        Style::default()
            .fg(theme.text_secondary)
            .bg(theme.bg_base)
            .add_modifier(Modifier::BOLD),
    );
    let footer_width = u16::try_from(Line::from(footer).width()).unwrap_or(u16::MAX);
    if footer_width < inner.width {
        buffer.set_string(
            inner.x + (inner.width - footer_width) / 2,
            inner.bottom().saturating_sub(1),
            footer,
            Style::default().fg(theme.gray).bg(theme.bg_base),
        );
    }
    Some(ModalFrame {
        popup,
        content: Rect::new(
            inner.x + 2,
            inner.y + 1,
            inner.width.saturating_sub(4),
            inner.height.saturating_sub(3),
        ),
        close_button,
    })
}

/// Shared selected-row treatment for list choices inside Astral modal frames.
///
/// Grok's modal picker keeps the selected row on the visual panel color and
/// emphasizes its primary label. Keeping this in the modal layer prevents the
/// resume, theme, and permission pickers from drifting onto prompt-border or
/// menu-specific colors.
pub(crate) fn modal_choice_style(theme: AstralTheme, selected: bool) -> Style {
    if selected {
        Style::default()
            .fg(theme.text_primary)
            .bg(theme.panel_selected)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text_primary).bg(theme.bg_base)
    }
}

fn popup_area(area: Rect, height: ModalHeight) -> Option<Rect> {
    if area.width < 20 || area.height < 8 {
        return None;
    }
    if height == ModalHeight::FullViewport {
        let width = (area.width.saturating_mul(95) / 100)
            .max(60.min(area.width))
            .min(area.width);
        let height = (area.height.saturating_mul(92) / 100)
            .max(12.min(area.height))
            .min(area.height);
        return Some(Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        ));
    }
    let width = (area.width.saturating_mul(3) / 5).clamp(44.min(area.width), 120.min(area.width));
    let minimum = match height {
        ModalHeight::Adaptive => 8,
        ModalHeight::MinimumContent(content_height) => content_height.saturating_add(5),
        ModalHeight::FullViewport => unreachable!("handled above"),
    };
    let height = area.height.saturating_sub(8).max(minimum).min(area.height);
    Some(Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    ))
}
