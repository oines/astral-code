//! Per-turn timeline rail for the Astral transcript.
//!
//! Geometry and glyph hierarchy are derived from Grok Build's
//! `views/timeline.rs` at commit `47348d13ec4508dcfe440e34c6d511bb02998fb2`
//! (Apache-2.0). Astral owns the turn and viewport projection.

use std::ops::Range;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;

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
    pub(crate) up_target: Option<usize>,
    pub(crate) down_target: Option<usize>,
    pub(crate) at_bottom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TimelineRail {
    rect: Rect,
    window: Range<usize>,
    ticks_y: u16,
    active: Option<usize>,
    up_target: Option<usize>,
    down_target: Option<usize>,
    up_y: u16,
    down_y: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TimelineHit {
    Tick(usize),
    Up,
    Down,
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
        up_target: viewport.up_target,
        down_target: viewport.down_target,
        up_y: top,
        down_y,
    })
}

impl TimelineRail {
    pub(crate) fn contains(&self, column: u16, row: u16) -> bool {
        self.rect.contains((column, row).into())
    }

    pub(crate) fn hit(&self, column: u16, row: u16) -> Option<TimelineHit> {
        if !self.contains(column, row) {
            return None;
        }
        if row == self.up_y {
            return Some(TimelineHit::Up);
        }
        if row == self.down_y {
            return Some(TimelineHit::Down);
        }
        if row >= self.ticks_y {
            let relative = usize::from(row - self.ticks_y);
            if relative < self.window.len() {
                return Some(TimelineHit::Tick(self.window.start + relative));
            }
        }
        None
    }

    pub(crate) fn target(&self, hit: TimelineHit) -> Option<usize> {
        match hit {
            TimelineHit::Tick(turn_index) => Some(turn_index),
            TimelineHit::Up => self.up_target,
            TimelineHit::Down => self.down_target,
        }
    }

    pub(crate) fn contains_hit(&self, hit: TimelineHit) -> bool {
        match hit {
            TimelineHit::Tick(turn_index) => self.window.contains(&turn_index),
            TimelineHit::Up | TimelineHit::Down => true,
        }
    }
}

pub(crate) fn render_rail(
    buffer: &mut Buffer,
    rail: &TimelineRail,
    hovered: Option<TimelineHit>,
    theme: AstralTheme,
) {
    let dim = Style::default().fg(theme.gray_dim);
    let normal = Style::default().fg(theme.gray);
    let bright = Style::default().fg(theme.text_primary);
    let up_enabled = rail.target(TimelineHit::Up).is_some();
    let down_enabled = rail.target(TimelineHit::Down).is_some();
    let chevron_x = rail.rect.x + RAIL_WIDTH - 1;
    buffer.set_string(
        chevron_x,
        rail.up_y,
        "▲",
        if hovered == Some(TimelineHit::Up) && up_enabled {
            bright
        } else if up_enabled {
            normal
        } else {
            dim
        },
    );
    buffer.set_string(
        chevron_x,
        rail.down_y,
        "▼",
        if hovered == Some(TimelineHit::Down) && down_enabled {
            bright
        } else if down_enabled {
            normal
        } else {
            dim
        },
    );
    for (row, turn_index) in rail.window.clone().enumerate() {
        let y = rail.ticks_y + u16::try_from(row).unwrap_or(u16::MAX);
        let (glyph, style) = if rail.active == Some(turn_index) {
            ("━━", bright)
        } else if hovered == Some(TimelineHit::Tick(turn_index)) {
            (" ━", bright)
        } else {
            (" ─", dim)
        };
        buffer.set_string(rail.rect.x, y, glyph, style);
    }
}

pub(crate) fn render_tick_hover_popup(
    buffer: &mut Buffer,
    rail: &TimelineRail,
    scrollback_area: Rect,
    turn_index: usize,
    preview: &str,
    theme: AstralTheme,
) {
    if !rail.window.contains(&turn_index) || preview.trim().is_empty() {
        return;
    }
    let tick_y = rail.ticks_y + u16::try_from(turn_index - rail.window.start).unwrap_or(u16::MAX);
    let max_text_width = (scrollback_area.width / 2).clamp(16, 32);
    let source = Line::from(preview.trim().to_string());
    let lines = astral_tui_scrollback::wrap_styled_line_with_metadata(&source, max_text_width)
        .into_iter()
        .take(2)
        .map(|line| line.line)
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return;
    }
    let text_width = lines
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or_default()
        .min(usize::from(max_text_width));
    let card_width = u16::try_from(text_width)
        .unwrap_or(max_text_width)
        .saturating_add(4);
    let card_height = u16::try_from(lines.len()).unwrap_or(2).saturating_add(2);
    if card_height > scrollback_area.height {
        return;
    }
    let card_x = rail
        .rect
        .x
        .saturating_sub(card_width.saturating_add(1))
        .max(scrollback_area.x);
    let card_y = tick_y
        .saturating_sub(card_height / 2)
        .max(scrollback_area.y)
        .min(scrollback_area.bottom().saturating_sub(card_height));
    let card_area = Rect::new(card_x, card_y, card_width, card_height);
    Clear.render(card_area, buffer);
    buffer.set_style(card_area, Style::default().bg(theme.bg_base));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.gray).bg(theme.bg_base));
    let inner = block.inner(card_area);
    block.render(card_area, buffer);
    let text_style = Style::default().fg(theme.text_primary).bg(theme.bg_base);
    for (index, line) in lines.into_iter().enumerate() {
        let y = inner.y + u16::try_from(index).unwrap_or(u16::MAX);
        if y >= inner.bottom() {
            break;
        }
        buffer.set_line(
            inner.x.saturating_add(1),
            y,
            &line.patch_style(text_style),
            max_text_width,
        );
    }
}

#[cfg(test)]
#[path = "timeline_rail_tests.rs"]
mod tests;
