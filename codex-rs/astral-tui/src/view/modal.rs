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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModalSizing {
    width_percent: u16,
    min_width: u16,
    max_width: u16,
    height: ModalHeight,
}

impl ModalSizing {
    pub(crate) const fn shortcuts() -> Self {
        Self {
            width_percent: 70,
            min_width: 44,
            max_width: 80,
            height: ModalHeight::Adaptive,
        }
    }

    pub(crate) const fn picker() -> Self {
        Self {
            width_percent: 50,
            min_width: 44,
            max_width: 80,
            height: ModalHeight::Adaptive,
        }
    }

    pub(crate) const fn settings() -> Self {
        Self {
            width_percent: 70,
            min_width: 44,
            max_width: 120,
            height: ModalHeight::FullViewport,
        }
    }

    const fn standard(height: ModalHeight) -> Self {
        match height {
            ModalHeight::FullViewport => Self {
                width_percent: 95,
                min_width: 60,
                max_width: u16::MAX,
                height,
            },
            ModalHeight::Adaptive | ModalHeight::MinimumContent(_) => Self {
                width_percent: 60,
                min_width: 44,
                max_width: 120,
                height,
            },
        }
    }
}

pub(crate) struct InfoModal<'a> {
    pub(crate) state: &'a mut ModalState,
}

impl InfoModal<'_> {
    pub(crate) fn render(self, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
        let footer = if self.state.open_target.is_some() && self.state.rows.len() > 10 {
            "Enter open · ↑/↓ scroll · Esc close"
        } else if self.state.open_target.is_some() {
            "Enter open · Esc close"
        } else if self.state.rows.len() > 10 {
            "↑/↓ scroll · Esc close"
        } else {
            "Esc close"
        };
        let Some(frame) = render_modal_frame_with_geometry(
            area,
            buffer,
            theme,
            &self.state.title,
            footer,
            ModalHeight::Adaptive,
        ) else {
            return;
        };
        render_modal_close_button(
            buffer,
            frame.close_button,
            theme,
            self.state.pointer.close_hovered(),
        );
        self.state
            .pointer
            .observe_frame(frame.popup, frame.close_button, Vec::new());
        let content = frame.content;
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

pub(crate) fn render_modal_frame_with_geometry(
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
    title: &str,
    footer: &str,
    height: ModalHeight,
) -> Option<ModalFrame> {
    render_modal_frame_with_sizing(
        area,
        buffer,
        theme,
        title,
        footer,
        ModalSizing::standard(height),
    )
}

pub(crate) fn render_modal_frame_with_sizing(
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
    title: &str,
    footer: &str,
    sizing: ModalSizing,
) -> Option<ModalFrame> {
    let popup = popup_area(area, sizing)?;
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
    render_modal_close_button(buffer, close_button, theme, /*hovered*/ false);
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

pub(crate) fn render_modal_close_button(
    buffer: &mut Buffer,
    area: Rect,
    theme: AstralTheme,
    hovered: bool,
) {
    buffer.set_string(
        area.x,
        area.y,
        "[×]",
        Style::default()
            .fg(if hovered {
                theme.text_primary
            } else {
                theme.text_secondary
            })
            .bg(theme.bg_base)
            .add_modifier(Modifier::BOLD),
    );
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

fn popup_area(area: Rect, sizing: ModalSizing) -> Option<Rect> {
    if area.width < 20 || area.height < 8 {
        return None;
    }
    if sizing.height == ModalHeight::FullViewport {
        let width = (area.width.saturating_mul(sizing.width_percent) / 100).clamp(
            sizing.min_width.min(area.width),
            sizing.max_width.min(area.width),
        );
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
    let width = (area.width.saturating_mul(sizing.width_percent) / 100).clamp(
        sizing.min_width.min(area.width),
        sizing.max_width.min(area.width),
    );
    let minimum = match sizing.height {
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
