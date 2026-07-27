//! Per-turn timeline rail for the Astral transcript.
//!
//! Geometry and glyph hierarchy are derived from Grok Build's
//! `views/timeline.rs` at commit `47348d13ec4508dcfe440e34c6d511bb02998fb2`
//! (Apache-2.0). Astral owns the turn and viewport projection.

use std::ops::Range;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::view::AstralTheme;

pub(crate) const RAIL_WIDTH: u16 = 2;
const MIN_TERMINAL_WIDTH: u16 = 60;
const MIN_TURNS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RailEligibility {
    pub(crate) visible: bool,
    pub(crate) area_width: u16,
    pub(crate) turn_count: usize,
}

pub(crate) fn rail_width(input: RailEligibility) -> u16 {
    if input.visible && input.area_width >= MIN_TERMINAL_WIDTH && input.turn_count >= MIN_TURNS {
        RAIL_WIDTH
    } else {
        0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RailViewport {
    pub(crate) active: Option<usize>,
    pub(crate) at_bottom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TimelineRail {
    rect: Rect,
    window: Range<usize>,
    ticks_y: u16,
    active: Option<usize>,
    up_y: u16,
    down_y: u16,
}

pub(crate) fn compute_rail(
    scrollback_area: Rect,
    rail_x: u16,
    turn_count: usize,
    viewport: RailViewport,
) -> Option<TimelineRail> {
    if turn_count < MIN_TURNS {
        return None;
    }
    let height = usize::from(scrollback_area.height);
    let max_ticks = height.checked_sub(2)?;
    if max_ticks == 0 {
        return None;
    }
    let window = if turn_count <= max_ticks {
        0..turn_count
    } else {
        let tail_start = turn_count - max_ticks;
        let start = if viewport.at_bottom {
            viewport
                .active
                .map_or(tail_start, |active| active.min(tail_start))
        } else {
            viewport
                .active
                .unwrap_or(turn_count - 1)
                .saturating_sub(max_ticks / 2)
                .min(tail_start)
        };
        start..start + max_ticks
    };
    let total_rows = window.len() + 2;
    let top = scrollback_area.y + u16::try_from((height - total_rows) / 2).unwrap_or(u16::MAX);
    let ticks_y = top + 1;
    let down_y = ticks_y + u16::try_from(window.len()).unwrap_or(u16::MAX);
    Some(TimelineRail {
        rect: Rect::new(
            rail_x,
            scrollback_area.y,
            RAIL_WIDTH,
            scrollback_area.height,
        ),
        window,
        ticks_y,
        active: viewport.active,
        up_y: top,
        down_y,
    })
}

pub(crate) fn render_rail(
    buffer: &mut Buffer,
    rail: &TimelineRail,
    turn_count: usize,
    theme: AstralTheme,
) {
    let active = rail.active.unwrap_or(turn_count.saturating_sub(1));
    let chevron_x = rail.rect.x + RAIL_WIDTH - 1;
    buffer.set_string(
        chevron_x,
        rail.up_y,
        "▲",
        Style::default().fg(if active > 0 {
            theme.gray
        } else {
            theme.gray_dim
        }),
    );
    buffer.set_string(
        chevron_x,
        rail.down_y,
        "▼",
        Style::default().fg(if active + 1 < turn_count {
            theme.gray
        } else {
            theme.gray_dim
        }),
    );
    for (row, turn_index) in rail.window.clone().enumerate() {
        let y = rail.ticks_y + u16::try_from(row).unwrap_or(u16::MAX);
        let (glyph, color) = if rail.active == Some(turn_index) {
            ("━━", theme.text_primary)
        } else {
            (" ─", theme.gray_dim)
        };
        buffer.set_string(rail.rect.x, y, glyph, Style::default().fg(color));
    }
}

#[cfg(test)]
#[path = "timeline_rail_tests.rs"]
mod tests;
