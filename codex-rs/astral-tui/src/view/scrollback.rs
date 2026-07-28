// Derived from Grok Build's scrollback viewport and scrollbar behavior at
// commit 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Modified for Astral's app-server-backed transcript projection.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use super::AstralTheme;
use super::transcript::TranscriptAnchor;
use super::transcript::TranscriptLayout;
use super::transcript::TranscriptSection;

/// Measured position of a scrollback viewport.
///
/// Follow mode can be measured from the tail, while manual navigation supplies
/// the anchored top line directly. Both paths resolve to the same visible
/// slice and scrollbar coordinates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ScrollbackViewport {
    pub(crate) first_visible_line: usize,
    pub(crate) end_visible_line: usize,
    pub(crate) total_lines: usize,
    pub(crate) viewport_lines: usize,
    pub(crate) has_content_above: bool,
    pub(crate) has_content_below: bool,
}

impl ScrollbackViewport {
    pub(crate) fn measure(
        total_lines: usize,
        viewport_lines: usize,
        distance_from_bottom: usize,
    ) -> Self {
        let viewport_lines = viewport_lines.max(1);
        let max_top = total_lines.saturating_sub(viewport_lines);
        let distance_from_bottom = distance_from_bottom.min(max_top);
        let first_visible_line = max_top.saturating_sub(distance_from_bottom);
        Self::from_first(total_lines, viewport_lines, first_visible_line)
    }

    pub(crate) fn from_first(
        total_lines: usize,
        viewport_lines: usize,
        first_visible_line: usize,
    ) -> Self {
        let viewport_lines = viewport_lines.max(1);
        let max_top = total_lines.saturating_sub(viewport_lines);
        let first_visible_line = first_visible_line.min(max_top);
        let end_visible_line = first_visible_line
            .saturating_add(viewport_lines)
            .min(total_lines);
        Self {
            first_visible_line,
            end_visible_line,
            total_lines,
            viewport_lines,
            has_content_above: first_visible_line > 0,
            has_content_below: end_visible_line < total_lines,
        }
    }

    pub(crate) fn needs_scrollbar(self) -> bool {
        self.total_lines > self.viewport_lines
    }
}

/// Stateful navigation for Astral's fullscreen transcript.
///
/// Follow mode stays pinned to the tail. Manual mode stores a stable item
/// anchor so streaming growth and reflow before that item do not move the
/// content the user was reading.
#[derive(Debug)]
pub(crate) struct ScrollbackNavigation {
    follow_mode: bool,
    first_visible_line: usize,
    total_lines: usize,
    viewport_lines: usize,
    width: u16,
    pending_distance_from_bottom: usize,
    anchor: Option<TranscriptAnchor>,
    sections: Vec<TranscriptSection>,
}

impl Default for ScrollbackNavigation {
    fn default() -> Self {
        Self {
            follow_mode: true,
            first_visible_line: 0,
            total_lines: 0,
            viewport_lines: 0,
            width: 0,
            pending_distance_from_bottom: 0,
            anchor: None,
            sections: Vec::new(),
        }
    }
}

impl ScrollbackNavigation {
    pub(crate) fn prepare(
        &mut self,
        layout: &TranscriptLayout,
        width: u16,
        viewport_lines: usize,
    ) -> ScrollbackViewport {
        let viewport_lines = viewport_lines.max(1);
        let max_top = layout.lines.len().saturating_sub(viewport_lines);
        if self.follow_mode {
            self.first_visible_line = max_top;
        } else if let Some(anchor) = self.anchor.as_ref()
            && let Some(section) = layout.section(&anchor.item_id)
        {
            let section_height = section.lines.len().max(1);
            let line_offset = if width == self.width {
                anchor.line_offset
            } else {
                anchor
                    .line_offset
                    .saturating_mul(section_height)
                    .checked_div(anchor.section_height)
                    .unwrap_or(0)
            };
            self.first_visible_line = section
                .lines
                .start
                .saturating_add(line_offset.min(section_height - 1));
        } else if self.viewport_lines == 0 {
            self.first_visible_line = max_top.saturating_sub(self.pending_distance_from_bottom);
        }
        self.first_visible_line = self.first_visible_line.min(max_top);
        self.total_lines = layout.lines.len();
        self.viewport_lines = viewport_lines;
        self.width = width;
        self.sections.clone_from(&layout.sections);
        self.pending_distance_from_bottom = 0;
        self.refresh_anchor();
        ScrollbackViewport::from_first(
            self.total_lines,
            self.viewport_lines,
            self.first_visible_line,
        )
    }

    pub(crate) fn scroll_up(&mut self, lines: usize) {
        self.follow_mode = false;
        if self.viewport_lines == 0 {
            self.pending_distance_from_bottom =
                self.pending_distance_from_bottom.saturating_add(lines);
            return;
        }
        self.first_visible_line = self.first_visible_line.saturating_sub(lines);
        self.refresh_anchor();
    }

    pub(crate) fn scroll_down(&mut self, lines: usize) {
        if self.viewport_lines == 0 {
            self.pending_distance_from_bottom =
                self.pending_distance_from_bottom.saturating_sub(lines);
            self.follow_mode = self.pending_distance_from_bottom == 0;
            return;
        }
        let max_top = self.total_lines.saturating_sub(self.viewport_lines);
        let was_at_bottom = self.first_visible_line == max_top;
        self.first_visible_line = self.first_visible_line.saturating_add(lines).min(max_top);
        self.follow_mode = was_at_bottom;
        self.refresh_anchor();
    }

    pub(crate) fn scroll_to_bottom(&mut self) {
        self.follow_mode = true;
        self.pending_distance_from_bottom = 0;
        self.first_visible_line = self.total_lines.saturating_sub(self.viewport_lines);
        self.anchor = None;
    }

    pub(crate) fn scroll_to_top(&mut self) {
        self.set_scroll_offset(0);
    }

    pub(crate) fn set_scroll_offset(&mut self, offset: usize) {
        let max_top = self.total_lines.saturating_sub(self.viewport_lines);
        self.follow_mode = false;
        self.pending_distance_from_bottom = 0;
        self.first_visible_line = offset.min(max_top);
        self.refresh_anchor();
    }

    pub(crate) fn page_up(&mut self) -> ScrollbackViewport {
        self.scroll_up(self.page_scroll_lines());
        self.viewport()
    }

    pub(crate) fn page_down(&mut self) -> ScrollbackViewport {
        self.scroll_down(self.page_scroll_lines());
        self.viewport()
    }

    pub(crate) fn half_page_up(&mut self) {
        self.scroll_up(self.half_page_scroll_lines());
    }

    pub(crate) fn half_page_down(&mut self) {
        self.scroll_down(self.half_page_scroll_lines());
    }

    pub(crate) fn reveal_entry(&mut self, item_id: &str) {
        let Some(section) = self
            .sections
            .iter()
            .find(|section| section.item_id == item_id)
        else {
            return;
        };
        let viewport_end = self.first_visible_line.saturating_add(self.viewport_lines);
        if section.lines.start < self.first_visible_line {
            self.first_visible_line = section.lines.start;
        } else if section.lines.end > viewport_end {
            self.first_visible_line = section.lines.end.saturating_sub(self.viewport_lines);
        } else {
            return;
        }
        self.follow_mode = false;
        self.refresh_anchor();
    }

    pub(crate) fn entry_top(&self, item_id: &str) -> Option<usize> {
        self.sections
            .iter()
            .find(|section| section.item_id == item_id)
            .map(|section| section.lines.start)
    }

    pub(crate) fn scroll_entry_to_top(&mut self, item_id: &str) {
        let Some(top) = self.entry_top(item_id) else {
            return;
        };
        self.set_scroll_offset(top);
    }

    pub(crate) fn distance_from_bottom(&self) -> usize {
        if self.viewport_lines == 0 {
            self.pending_distance_from_bottom
        } else if self.follow_mode {
            0
        } else {
            self.total_lines
                .saturating_sub(self.viewport_lines)
                .saturating_sub(self.first_visible_line)
        }
    }

    pub(crate) fn sections(&self) -> &[TranscriptSection] {
        &self.sections
    }

    pub(crate) fn viewport(&self) -> ScrollbackViewport {
        ScrollbackViewport::from_first(
            self.total_lines,
            self.viewport_lines,
            self.first_visible_line,
        )
    }

    fn page_scroll_lines(&self) -> usize {
        self.viewport_lines.saturating_sub(2).max(1)
    }

    fn half_page_scroll_lines(&self) -> usize {
        (self.viewport_lines / 2).max(1)
    }

    fn refresh_anchor(&mut self) {
        self.anchor = (!self.follow_mode)
            .then(|| {
                TranscriptAnchor::at(&self.sections, self.total_lines, self.first_visible_line)
            })
            .flatten();
    }
}

pub(crate) struct ScrollbackPane<'a> {
    pub(crate) lines: &'a [Line<'static>],
    pub(crate) viewport: ScrollbackViewport,
}

impl ScrollbackPane<'_> {
    pub(crate) fn render(
        self,
        content_area: Rect,
        scrollbar_area: Rect,
        buffer: &mut Buffer,
        theme: AstralTheme,
    ) -> ScrollbackViewport {
        let viewport = self.viewport;
        if !content_area.is_empty() {
            let visible =
                self.lines[viewport.first_visible_line..viewport.end_visible_line].to_vec();
            Paragraph::new(Text::from(visible)).render(content_area, buffer);
        }
        render_scrollbar(scrollbar_area, viewport, buffer, theme);
        viewport
    }
}

pub(crate) fn render_follow_indicator(
    viewport: ScrollbackViewport,
    transcript_area: Rect,
    y: u16,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    if !viewport.has_content_below || y >= buffer.area.bottom() || transcript_area.width == 0 {
        return;
    }
    let x = transcript_area.x + transcript_area.width / 2;
    if let Some(cell) = buffer.cell_mut((x, y)) {
        cell.set_symbol("▼")
            .set_style(Style::default().fg(theme.gray));
    }
}

fn render_scrollbar(
    area: Rect,
    viewport: ScrollbackViewport,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    if area.is_empty() || !viewport.needs_scrollbar() {
        return;
    }
    let track_height = usize::from(area.height);
    let thumb_height = viewport
        .viewport_lines
        .saturating_mul(track_height)
        .div_ceil(viewport.total_lines)
        .clamp(1, track_height);
    let max_top = viewport.total_lines.saturating_sub(viewport.viewport_lines);
    let thumb_travel = track_height.saturating_sub(thumb_height);
    let thumb_top = viewport
        .first_visible_line
        .saturating_mul(thumb_travel)
        .saturating_add(max_top / 2)
        .checked_div(max_top)
        .unwrap_or(0);
    let track_style = Style::default().bg(theme.panel_background);
    let thumb_style = Style::default()
        .fg(if viewport.has_content_below {
            theme.gray
        } else {
            theme.gray_dim
        })
        .bg(theme.panel_background);
    for row in 0..track_height {
        let y = area.y + u16::try_from(row).unwrap_or(u16::MAX);
        if let Some(cell) = buffer.cell_mut((area.x, y)) {
            if (thumb_top..thumb_top + thumb_height).contains(&row) {
                cell.set_symbol("█").set_style(thumb_style);
            } else {
                cell.set_symbol(" ").set_style(track_style);
            }
        }
    }
}

#[cfg(test)]
#[path = "scrollback_tests.rs"]
mod tests;
