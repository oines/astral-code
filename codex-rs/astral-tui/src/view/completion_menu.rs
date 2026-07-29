use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::AstralTheme;

/// Geometry produced by a completion menu render and consumed by pointer
/// input on the next terminal frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CompletionMenuFrame {
    area: Rect,
    rows: Vec<(usize, Rect)>,
    scrollbar: Option<Rect>,
    window_start: usize,
}

impl CompletionMenuFrame {
    pub(crate) fn new(area: Rect, total_items: usize, selected_item: usize) -> Self {
        let items = Rect::new(
            area.x.saturating_add(1),
            area.y.saturating_add(1),
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );
        let visible_rows = usize::from(items.height);
        let window_start = selected_item.saturating_add(1).saturating_sub(visible_rows);
        let scrollbar = (total_items > visible_rows && items.width > 1)
            .then(|| Rect::new(items.right().saturating_sub(1), items.y, 1, items.height));
        Self {
            area,
            rows: Vec::with_capacity(visible_rows),
            scrollbar,
            window_start,
        }
    }

    pub(crate) fn window_start(&self) -> usize {
        self.window_start
    }

    pub(crate) fn visible_rows(&self) -> usize {
        usize::from(self.area.height.saturating_sub(2))
    }

    pub(crate) fn row_rect(&self, visible_row: usize) -> Rect {
        let scrollbar_width = u16::from(self.scrollbar.is_some());
        Rect::new(
            self.area.x.saturating_add(1),
            self.area
                .y
                .saturating_add(1)
                .saturating_add(u16::try_from(visible_row).unwrap_or(u16::MAX)),
            self.area
                .width
                .saturating_sub(2)
                .saturating_sub(scrollbar_width),
            1,
        )
    }

    pub(crate) fn observe_row(&mut self, item: usize, rect: Rect) {
        self.rows.push((item, rect));
    }

    pub(crate) fn contains(&self, column: u16, row: u16) -> bool {
        self.area.contains((column, row).into())
    }

    pub(crate) fn row_at(&self, column: u16, row: u16) -> Option<usize> {
        self.rows
            .iter()
            .find(|(_, rect)| rect.contains((column, row).into()))
            .map(|(item, _)| *item)
    }

    pub(crate) fn contains_item(&self, item: usize) -> bool {
        self.rows.iter().any(|(visible, _)| *visible == item)
    }

    pub(crate) fn scrollbar_target(
        &self,
        column: u16,
        row: u16,
        total_items: usize,
    ) -> Option<usize> {
        let scrollbar = self.scrollbar?;
        if !scrollbar.contains((column, row).into()) || total_items == 0 {
            return None;
        }
        let cell = usize::from(row.saturating_sub(scrollbar.y));
        let track_travel = usize::from(scrollbar.height).saturating_sub(1);
        if track_travel == 0 {
            return Some(0);
        }
        Some(
            cell.saturating_mul(total_items.saturating_sub(1))
                .checked_div(track_travel)
                .unwrap_or(0)
                .min(total_items.saturating_sub(1)),
        )
    }

    pub(crate) fn render_scrollbar(
        &self,
        buffer: &mut Buffer,
        theme: AstralTheme,
        total_items: usize,
        selected_item: usize,
    ) {
        let Some(scrollbar) = self.scrollbar else {
            return;
        };
        let track_height = usize::from(scrollbar.height);
        let visible_rows = self.visible_rows().min(total_items);
        let thumb_height = visible_rows
            .saturating_mul(track_height)
            .checked_div(total_items.max(1))
            .unwrap_or(1)
            .clamp(1, track_height);
        let thumb_travel = track_height.saturating_sub(thumb_height);
        let thumb_top = selected_item
            .min(total_items.saturating_sub(1))
            .saturating_mul(thumb_travel)
            .checked_div(total_items.saturating_sub(1).max(1))
            .unwrap_or(0);
        for offset in 0..track_height {
            let y = scrollbar
                .y
                .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX));
            let (symbol, color) = if (thumb_top..thumb_top + thumb_height).contains(&offset) {
                ("█", theme.gray)
            } else {
                ("│", theme.gray_dim)
            };
            if let Some(cell) = buffer.cell_mut((scrollbar.x, y)) {
                cell.set_symbol(symbol)
                    .set_style(Style::default().fg(color).bg(theme.bg_base));
            }
        }
    }
}
