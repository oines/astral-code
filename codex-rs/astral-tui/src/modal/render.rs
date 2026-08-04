use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;

use super::MIN_MODAL_HEIGHT;
use super::MIN_MODAL_WIDTH;
use super::ModalContentArea;
use super::ModalPresentation;
use super::ModalShortcut;
use super::ModalSizing;
use super::ModalWindow;
use super::ModalWindowConfig;
use super::SHORTCUT_SEPARATOR;
use super::ShortcutHit;

impl ModalWindow {
    /// Paint chrome and return the only area the domain presenter should use.
    pub fn render(
        &mut self,
        buffer: &mut Buffer,
        area: Rect,
        config: &ModalWindowConfig<'_>,
    ) -> Option<ModalContentArea> {
        self.clear_geometry();
        let modal_area = modal_area(area, config.presentation, config.sizing)?;
        self.rendered_area = Some(modal_area);
        Clear.render(modal_area, buffer);
        let inner = match config.presentation {
            ModalPresentation::Popup => self.render_popup(buffer, modal_area, config.title),
            ModalPresentation::Embedded => {
                self.render_embedded_title(buffer, modal_area, config.title)
            }
        };
        if config.tabs.is_empty() {
            self.active_tab = 0;
        } else {
            self.active_tab = self.active_tab.min(config.tabs.len().saturating_sub(1));
        }
        let tab_rows = self.render_tabs(buffer, inner, config.tabs);
        let divider_rows = u16::from(!config.tabs.is_empty() && tab_rows < inner.height);
        if divider_rows == 1 {
            let y = inner.y.saturating_add(tab_rows);
            buffer.set_line(
                inner.x,
                y,
                &Line::from("─".repeat(usize::from(inner.width))).style(self.style.border),
                inner.width,
            );
        }
        let footer_width = inner
            .width
            .saturating_sub(config.sizing.horizontal_padding.saturating_mul(2));
        let footer_rows = config
            .sizing
            .footer_rows
            .max(shortcut_rows_needed(config.shortcuts, footer_width))
            .min(inner.height);
        let footer = Rect::new(
            padded_x(inner, config.sizing.horizontal_padding),
            inner.bottom().saturating_sub(footer_rows),
            footer_width,
            footer_rows,
        );
        self.render_shortcuts(buffer, footer, config.shortcuts);
        let content_top = inner
            .y
            .saturating_add(tab_rows)
            .saturating_add(divider_rows)
            .saturating_add(if config.tabs.is_empty() {
                config.sizing.vertical_padding
            } else {
                0
            })
            .min(footer.y);
        Some(ModalContentArea {
            content: Rect::new(
                padded_x(inner, config.sizing.horizontal_padding),
                content_top,
                footer_width,
                footer.y.saturating_sub(content_top),
            ),
            footer,
            inner,
        })
    }
    fn render_popup(&mut self, buffer: &mut Buffer, area: Rect, title: &str) -> Rect {
        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(self.style.border);
        if !title.is_empty() {
            block = block.title(Line::from(vec![
                Span::styled("─ ", self.style.border),
                Span::styled(title, Style::default().bold()),
                Span::styled(" ─", self.style.border),
            ]));
        }
        let inner = block.inner(area);
        block.render(area, buffer);
        let close_rect = Rect::new(area.right().saturating_sub(7), area.y, 5, 1);
        let style = if self.close_hovered {
            Style::default().bold().patch(self.style.hover)
        } else {
            Style::default().bold()
        };
        buffer.set_line(
            close_rect.x,
            close_rect.y,
            &Line::from(" [×] ").style(style),
            5,
        );
        self.close_rect = Some(close_rect);
        inner
    }
    fn render_embedded_title(&self, buffer: &mut Buffer, area: Rect, title: &str) -> Rect {
        if title.is_empty() {
            return area;
        }
        buffer.set_line(area.x, area.y, &Line::from(title).bold(), area.width);
        Rect::new(
            area.x,
            area.y.saturating_add(1),
            area.width,
            area.height.saturating_sub(1),
        )
    }
    fn render_tabs(&mut self, buffer: &mut Buffer, inner: Rect, tabs: &[&str]) -> u16 {
        self.tab_rects = vec![None; tabs.len()];
        if tabs.is_empty() || inner.width <= 2 {
            return 0;
        }
        let rows = wrapped_indices(tabs.iter().map(|tab| text_width(tab)), inner.width - 2, 2);
        for (row_index, row) in rows.iter().enumerate() {
            let y = inner.y.saturating_add(row_index as u16);
            if y >= inner.bottom() {
                break;
            }
            let mut x = inner.x.saturating_add(2);
            for (position, tab_index) in row.iter().copied().enumerate() {
                let available = inner.right().saturating_sub(x);
                if available == 0 {
                    continue;
                }
                let style = if tab_index == self.active_tab {
                    self.style.selection
                } else {
                    Style::default().dim()
                };
                let width = text_width(tabs[tab_index]).min(available);
                buffer.set_line(x, y, &Line::from(tabs[tab_index]).style(style), available);
                self.tab_rects[tab_index] = Some(Rect::new(x, y, width, 1));
                x = x.saturating_add(width);
                if position + 1 < row.len() {
                    buffer.set_line(x, y, &Line::from("  "), inner.right().saturating_sub(x));
                    x = x.saturating_add(2);
                }
            }
        }
        rows.len().min(usize::from(inner.height)) as u16
    }
    fn render_shortcuts(
        &mut self,
        buffer: &mut Buffer,
        area: Rect,
        shortcuts: &[ModalShortcut<'_>],
    ) {
        if area.is_empty() || shortcuts.is_empty() {
            return;
        }
        let rows = wrapped_indices(
            shortcuts.iter().map(|shortcut| text_width(shortcut.label)),
            area.width,
            text_width(SHORTCUT_SEPARATOR),
        );
        let visible_rows = rows.len().min(usize::from(area.height));
        for (row_index, row) in rows.iter().take(visible_rows).enumerate() {
            let y = area.bottom().saturating_sub(visible_rows as u16) + row_index as u16;
            let total_width = row.iter().enumerate().fold(0, |width, (position, index)| {
                width
                    + text_width(shortcuts[*index].label)
                    + u16::from(position > 0) * text_width(SHORTCUT_SEPARATOR)
            });
            let mut x = area
                .x
                .saturating_add(area.width.saturating_sub(total_width) / 2);
            for (position, shortcut_index) in row.iter().copied().enumerate() {
                if position > 0 {
                    let available = area.right().saturating_sub(x);
                    buffer.set_line(x, y, &Line::from(SHORTCUT_SEPARATOR).dim(), available);
                    x = x.saturating_add(text_width(SHORTCUT_SEPARATOR).min(available));
                }
                let shortcut = shortcuts[shortcut_index];
                let available = area.right().saturating_sub(x);
                if available == 0 {
                    break;
                }
                let width = text_width(shortcut.label).min(available);
                let hover = self.hovered_shortcut == Some(shortcut_index);
                let (key, label) = split_shortcut(shortcut.label);
                let key_style = hovered_style(Style::default().bold(), self.style.hover, hover);
                let label_style = hovered_style(Style::default().dim(), self.style.hover, hover);
                buffer.set_line(
                    x,
                    y,
                    &Line::from(vec![
                        Span::styled(key, key_style),
                        Span::styled(label, label_style),
                    ]),
                    available,
                );
                self.shortcut_hits.push(ShortcutHit {
                    rect: Rect::new(x, y, width, 1),
                    index: shortcut_index,
                    action: shortcut.action,
                });
                x = x.saturating_add(width);
            }
        }
    }
}
fn modal_area(area: Rect, presentation: ModalPresentation, sizing: ModalSizing) -> Option<Rect> {
    let resolved = match presentation {
        ModalPresentation::Embedded => area,
        ModalPresentation::Popup => {
            let preferred_width = (f32::from(area.width) * sizing.width_fraction) as u16;
            let maximum_width = area.width.saturating_sub(4).min(sizing.maximum_width);
            let width = preferred_width
                .min(maximum_width)
                .max(sizing.minimum_width)
                .min(area.width);
            let height = area
                .height
                .saturating_sub(sizing.vertical_margin.saturating_mul(2));
            Rect::new(
                area.x + area.width.saturating_sub(width) / 2,
                area.y + area.height.saturating_sub(height) / 2,
                width,
                height,
            )
        }
    };
    (resolved.width >= MIN_MODAL_WIDTH && resolved.height >= MIN_MODAL_HEIGHT).then_some(resolved)
}
fn shortcut_rows_needed(shortcuts: &[ModalShortcut<'_>], width: u16) -> u16 {
    wrapped_indices(
        shortcuts.iter().map(|shortcut| text_width(shortcut.label)),
        width,
        text_width(SHORTCUT_SEPARATOR),
    )
    .len() as u16
}
fn wrapped_indices(widths: impl IntoIterator<Item = u16>, width: u16, gap: u16) -> Vec<Vec<usize>> {
    if width == 0 {
        return Vec::new();
    }
    let mut rows = Vec::<Vec<usize>>::new();
    let mut row_width = 0u16;
    for (index, item_width) in widths.into_iter().enumerate() {
        let gap_width = if row_width == 0 { 0 } else { gap };
        if row_width > 0
            && row_width
                .saturating_add(gap_width)
                .saturating_add(item_width)
                > width
        {
            rows.push(Vec::new());
            row_width = 0;
        }
        match rows.last_mut() {
            Some(row) => row.push(index),
            None => rows.push(vec![index]),
        }
        row_width = item_width.saturating_add(if row_width == 0 {
            0
        } else {
            row_width.saturating_add(gap)
        });
    }
    rows
}

fn split_shortcut(label: &str) -> (&str, &str) {
    label
        .find(' ')
        .map_or((label, ""), |index| label.split_at(index))
}

fn text_width(text: &str) -> u16 {
    Line::from(text).width().min(usize::from(u16::MAX)) as u16
}

fn padded_x(area: Rect, padding: u16) -> u16 {
    area.x.saturating_add(padding.min(area.width))
}

fn hovered_style(base: Style, hover: Style, hovered: bool) -> Style {
    if hovered { base.patch(hover) } else { base }
}
