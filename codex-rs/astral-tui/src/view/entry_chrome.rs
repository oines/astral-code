// Derived from Grok Build's scrollback accent and selection-box rendering at
// commit 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Adapted to Astral's line-based transcript layout.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::AstralTheme;
use super::ScrollbackViewport;
use super::transcript::TranscriptAccent;
use super::transcript::TranscriptLayout;

pub(crate) fn render_entry_chrome(
    layout: &TranscriptLayout,
    viewport: ScrollbackViewport,
    area: Rect,
    selected_id: Option<&str>,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    if area.is_empty() {
        return;
    }
    render_accent_rails(layout, viewport, area, buffer);
    if let Some(lines) = selected_id.and_then(|item_id| layout.selection_lines(item_id)) {
        render_selection_box(lines, viewport, area, buffer, theme);
    }
}

fn render_accent_rails(
    layout: &TranscriptLayout,
    viewport: ScrollbackViewport,
    area: Rect,
    buffer: &mut Buffer,
) {
    let rail_x = area.x.saturating_sub(1);
    for section in &layout.sections {
        let Some(accent) = section.accent else {
            continue;
        };
        let (symbol, color) = match accent {
            TranscriptAccent::Full(color) => ("┃", color),
            TranscriptAccent::Collapsed(color) => ("❙", color),
        };
        let start = section.lines.start.max(viewport.first_visible_line);
        let end = section.lines.end.min(viewport.end_visible_line);
        for line in start..end {
            let y = area.y.saturating_add(
                u16::try_from(line.saturating_sub(viewport.first_visible_line)).unwrap_or(u16::MAX),
            );
            if y >= area.bottom() {
                break;
            }
            if let Some(cell) = buffer.cell_mut((rail_x, y)) {
                cell.set_symbol(symbol)
                    .set_style(Style::default().fg(color));
            }
        }
    }
}

fn render_selection_box(
    lines: std::ops::Range<usize>,
    viewport: ScrollbackViewport,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    let visible_start = lines.start.max(viewport.first_visible_line);
    let visible_end = lines.end.min(viewport.end_visible_line);
    if visible_start >= visible_end {
        return;
    }

    let first_y = area.y.saturating_add(
        u16::try_from(visible_start.saturating_sub(viewport.first_visible_line))
            .unwrap_or(u16::MAX),
    );
    let last_y = area.y.saturating_add(
        u16::try_from(
            visible_end
                .saturating_sub(viewport.first_visible_line)
                .saturating_sub(1),
        )
        .unwrap_or(u16::MAX),
    );
    let left_x = area.x.saturating_sub(2);
    let right_x = area
        .right()
        .saturating_add(1)
        .min(buffer.area.right().saturating_sub(1));
    if left_x >= right_x || first_y >= area.bottom() {
        return;
    }

    let top_clipped = lines.start < viewport.first_visible_line || first_y == 0;
    let bottom_clipped = lines.end > viewport.end_visible_line;
    let style = Style::default().fg(theme.selection_border);
    for y in first_y..=last_y.min(area.bottom().saturating_sub(1)) {
        let symbol = if (y == first_y && top_clipped) || (y == last_y && bottom_clipped) {
            "┆"
        } else {
            "│"
        };
        set_symbol(buffer, left_x, y, symbol, style);
        set_symbol(buffer, right_x, y, symbol, style);
    }

    if !top_clipped {
        set_symbol(buffer, left_x, first_y - 1, "┌", style);
        set_symbol(buffer, right_x, first_y - 1, "┐", style);
    }
    if !bottom_clipped {
        set_symbol(buffer, left_x, last_y.saturating_add(1), "└", style);
        set_symbol(buffer, right_x, last_y.saturating_add(1), "┘", style);
    }
}

fn set_symbol(buffer: &mut Buffer, x: u16, y: u16, symbol: &str, style: Style) {
    if let Some(cell) = buffer.cell_mut((x, y)) {
        cell.set_symbol(symbol).set_style(style);
    }
}

#[cfg(test)]
#[path = "entry_chrome_tests.rs"]
mod tests;
