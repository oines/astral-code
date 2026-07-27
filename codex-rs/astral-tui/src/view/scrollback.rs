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

/// Measured position of a scrollback viewport.
///
/// Astral stores manual scroll distance from the bottom because new streaming
/// rows naturally stay visible at zero. Rendering uses the equivalent
/// top-origin offset required by the scrollbar and visible slice.
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

    fn needs_scrollbar(self) -> bool {
        self.total_lines > self.viewport_lines
    }
}

pub(crate) struct ScrollbackPane<'a> {
    pub(crate) lines: &'a [Line<'static>],
    pub(crate) distance_from_bottom: usize,
}

impl ScrollbackPane<'_> {
    pub(crate) fn render(
        self,
        content_area: Rect,
        scrollbar_area: Rect,
        buffer: &mut Buffer,
        theme: AstralTheme,
    ) -> ScrollbackViewport {
        let viewport = ScrollbackViewport::measure(
            self.lines.len(),
            usize::from(content_area.height),
            self.distance_from_bottom,
        );
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
