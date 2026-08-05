use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::ModalShortcut;
use crate::ModalSizing;
use crate::ModalWindowConfig;

use super::BlockViewerDocument;
use super::BlockViewerHost;
use super::BlockViewerSource;
use super::COPY_SHORTCUT;
use super::RAW_SHORTCUT;

impl BlockViewerHost {
    /// Paint the Grok-style modal from the latest canonical entry.
    /// Returns `false` when the entry disappeared or the terminal is too small.
    pub fn render(
        &mut self,
        buffer: &mut Buffer,
        area: Rect,
        source: &(impl BlockViewerSource + ?Sized),
    ) -> bool {
        self.content_area = None;
        self.scrollbar_area = None;
        if !self.reconcile(source) {
            return false;
        }
        let Some(preview) = self.document(source, /*width*/ 1) else {
            return false;
        };
        let shortcuts = self.shortcuts(source);
        let config = ModalWindowConfig::new(preview.title())
            .with_shortcuts(&shortcuts)
            .with_sizing(ModalSizing::large());
        let Some(frame) = self.modal.render(buffer, area, &config) else {
            return false;
        };

        let full_width = frame.content.width.max(1);
        let mut document = self.document(source, full_width);
        let needs_scrollbar = document
            .as_ref()
            .is_some_and(|document| document.lines().len() > usize::from(frame.content.height))
            && full_width > 1;
        let render_width = full_width.saturating_sub(u16::from(needs_scrollbar)).max(1);
        if needs_scrollbar {
            document = self.document(source, render_width);
        }
        let Some(document) = document else {
            return false;
        };

        self.row_count = document.lines().len();
        self.content_height = frame.content.height;
        self.content_width = render_width;
        let maximum = self.maximum_scroll();
        if self.follow_bottom {
            self.scroll_offset = maximum;
        } else {
            self.scroll_offset = self.scroll_offset.min(maximum);
        }
        let content_area = Rect::new(
            frame.content.x,
            frame.content.y,
            render_width,
            frame.content.height,
        );
        self.content_area = Some(content_area);
        self.scrollbar_area = needs_scrollbar.then(|| {
            Rect::new(
                frame.content.right().saturating_sub(1),
                frame.content.y,
                1,
                frame.content.height,
            )
        });
        let visible = document
            .lines()
            .iter()
            .skip(self.scroll_offset)
            .take(usize::from(frame.content.height))
            .map(|line| line.line.clone())
            .collect::<Vec<Line<'static>>>();
        Paragraph::new(visible).render(content_area, buffer);
        if let Some(scrollbar) = self.scrollbar_area {
            paint_scrollbar(
                buffer,
                scrollbar,
                self.row_count,
                self.scroll_offset,
                usize::from(self.content_height),
            );
        }
        true
    }

    pub(super) fn copy_text(
        &mut self,
        source: &(impl BlockViewerSource + ?Sized),
    ) -> Option<String> {
        if !self.reconcile(source) {
            return None;
        }
        let document = self.document(source, self.content_width.max(1))?;
        let mut text = String::new();
        for (index, line) in document.lines().iter().enumerate() {
            if index > 0 {
                text.push_str(line.joiner_to_previous.as_str());
            }
            text.push_str(&line.line.to_string());
        }
        (!text.is_empty()).then_some(text)
    }

    fn shortcuts(&self, source: &(impl BlockViewerSource + ?Sized)) -> Vec<ModalShortcut<'static>> {
        let mut shortcuts = vec![
            ModalShortcut::hint("Esc close"),
            ModalShortcut::action(COPY_SHORTCUT, "y copy"),
        ];
        if self.supports_raw(source) {
            shortcuts.push(ModalShortcut::action(RAW_SHORTCUT, "r raw"));
        }
        shortcuts.push(ModalShortcut::hint("j/k scroll"));
        shortcuts
    }

    fn document(
        &self,
        source: &(impl BlockViewerSource + ?Sized),
        width: u16,
    ) -> Option<BlockViewerDocument> {
        source.block_viewer_document(self.entry_id, width, self.mode())
    }
}

fn paint_scrollbar(
    buffer: &mut Buffer,
    area: Rect,
    row_count: usize,
    scroll_offset: usize,
    viewport_height: usize,
) {
    if area.is_empty() || row_count <= viewport_height || viewport_height == 0 {
        return;
    }
    let track = Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::DIM);
    let thumb = Style::default().fg(Color::Gray);
    for y in area.y..area.bottom() {
        if let Some(cell) = buffer.cell_mut((area.x, y)) {
            cell.set_char('│').set_style(track);
        }
    }
    let thumb_height = viewport_height
        .saturating_mul(viewport_height)
        .div_ceil(row_count)
        .clamp(1, viewport_height);
    let travel = viewport_height.saturating_sub(thumb_height);
    let maximum = row_count.saturating_sub(viewport_height);
    let thumb_top = scroll_offset
        .min(maximum)
        .saturating_mul(travel)
        .checked_div(maximum)
        .unwrap_or(0);
    for offset in thumb_top..thumb_top.saturating_add(thumb_height) {
        if let Some(cell) = buffer.cell_mut((area.x, area.y.saturating_add(offset as u16))) {
            cell.set_char('█').set_style(thumb);
        }
    }
}
