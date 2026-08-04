//! Pure clipped renderer for the shared conversation surface.
//!
//! Hosts choose a viewport and terminal rectangle. This module paints the
//! already width-resolved surface without rewrapping or reprojecting entries,
//! so fullscreen and inline modes cannot disagree about content geometry.

use std::ops::Range;

use astral_tui_scrollback::DisplayMode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::ConversationSurface;
use crate::SurfaceNode;
use crate::SurfaceNodeId;
use crate::SurfaceViewport;

const NORMAL_CHROME_WIDTH: u16 = 5;

/// Theme roles used by the surface chrome. Entry content keeps the exact
/// styles produced by `astral-tui-scrollback`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceRenderStyle {
    pub rail: Style,
    pub selection: Style,
    pub hover: Style,
    pub scrollbar_track: Style,
    pub scrollbar_thumb: Style,
    pub scrollbar_follow_thumb: Style,
}

impl Default for SurfaceRenderStyle {
    fn default() -> Self {
        Self {
            rail: Style::default().fg(Color::DarkGray),
            selection: Style::default().fg(Color::Cyan),
            hover: Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
            scrollbar_track: Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
            scrollbar_thumb: Style::default().fg(Color::Gray),
            scrollbar_follow_thumb: Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        }
    }
}

/// Draws one clipped view over [`ConversationSurface`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceRenderer {
    style: SurfaceRenderStyle,
}

impl Default for SurfaceRenderer {
    fn default() -> Self {
        Self::new(SurfaceRenderStyle::default())
    }
}

impl SurfaceRenderer {
    pub fn new(style: SurfaceRenderStyle) -> Self {
        Self { style }
    }

    /// Content rectangle used for entry projection and terminal hyperlinks.
    pub fn content_area(area: Rect) -> Rect {
        Columns::for_area(area).content
    }

    /// Width hosts must use when building `EntryRenderOptions` for `area`.
    pub fn content_width(area: Rect) -> u16 {
        Self::content_area(area).width.max(1)
    }

    pub fn render(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        surface: &ConversationSurface,
        viewport: &SurfaceViewport,
    ) {
        if area.is_empty() {
            return;
        }

        Clear.render(area, buffer);
        let columns = Columns::for_area(area);
        let visible = visible_rows(area, surface, viewport);
        self.paint_rows(area, buffer, surface, &visible, columns);

        if let Some(hovered) = viewport
            .hovered()
            .filter(|hovered| Some(*hovered) != viewport.selected())
        {
            self.paint_node_box(
                BoxPaintContext {
                    area,
                    buffer: &mut *buffer,
                    surface,
                    visible: &visible,
                    columns,
                },
                hovered,
                self.style.hover,
                BoxExtent::Node,
            );
        }
        if let Some(selected) = viewport.selected() {
            self.paint_node_box(
                BoxPaintContext {
                    area,
                    buffer: &mut *buffer,
                    surface,
                    visible: &visible,
                    columns,
                },
                selected,
                self.style.selection,
                BoxExtent::DenseGroup,
            );
        }
        self.paint_expandable_indicator(area, buffer, surface, &visible, columns, viewport);
        self.paint_scrollbar(area, buffer, surface, viewport, columns);
    }

    /// Paint a fixed range from the shared surface without viewport-only
    /// selection, hover, or scrollbar chrome.
    ///
    /// Inline live-tail and terminal-native commit paths use this method so a
    /// node has identical wrapping, spacing, styles, and rails on both sides of
    /// the print-once frontier.
    pub fn render_rows(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        surface: &ConversationSurface,
        rows: Range<usize>,
    ) {
        if area.is_empty() {
            return;
        }

        Clear.render(area, buffer);
        let start = rows.start.min(surface.row_count());
        let visible = start
            ..rows
                .end
                .min(surface.row_count())
                .min(start.saturating_add(usize::from(area.height)));
        self.paint_rows(area, buffer, surface, &visible, Columns::for_area(area));
    }

    fn paint_rows(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        surface: &ConversationSurface,
        visible: &Range<usize>,
        columns: Columns,
    ) {
        let lines = visible
            .clone()
            .filter_map(|row| surface.line_at_row(row).map(|line| line.line.clone()))
            .collect::<Vec<Line<'static>>>();
        Paragraph::new(lines).render(columns.content, buffer);

        self.paint_rails(area, buffer, surface, visible, columns);
    }

    fn paint_rails(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        surface: &ConversationSurface,
        visible: &Range<usize>,
        columns: Columns,
    ) {
        let Some(x) = columns.rail else {
            return;
        };
        for node in surface.nodes().iter().filter(|node| node.is_groupable()) {
            let Some(rows) = clipped_rows(node.rows(), visible) else {
                continue;
            };
            let collapsed = node.display_mode() == DisplayMode::Collapsed;
            let symbol = if collapsed { '❙' } else { '┃' };
            let style = accent_style(node, self.style.rail, collapsed);
            for row in rows {
                let y = area
                    .y
                    .saturating_add(row.saturating_sub(visible.start) as u16);
                if let Some(cell) = buffer.cell_mut((x, y)) {
                    cell.set_char(symbol).set_style(style);
                }
            }
        }
    }

    fn paint_node_box(
        &self,
        context: BoxPaintContext<'_, '_>,
        id: SurfaceNodeId,
        style: Style,
        extent: BoxExtent,
    ) {
        let (Some(left), Some(right), Some(node)) = (
            context.columns.left_border,
            context.columns.right_border,
            context.surface.node(id),
        ) else {
            return;
        };
        let rows = match extent {
            BoxExtent::Node => node.rows(),
            BoxExtent::DenseGroup => dense_group_rows(context.surface, node),
        };
        let Some(clipped) = clipped_rows(rows.clone(), context.visible) else {
            return;
        };
        for row in clipped.clone() {
            let y = context
                .area
                .y
                .saturating_add(row.saturating_sub(context.visible.start) as u16);
            let clipped_edge = (row == clipped.start && rows.start < context.visible.start)
                || (row == clipped.end.saturating_sub(1) && rows.end > context.visible.end);
            let symbol = if clipped_edge { '┆' } else { '│' };
            paint_char(context.buffer, left, y, symbol, style);
            paint_char(context.buffer, right, y, symbol, style);
        }

        if rows.start > context.visible.start
            && context
                .surface
                .node_at_row(rows.start.saturating_sub(1))
                .is_none()
        {
            let y = context
                .area
                .y
                .saturating_add(rows.start.saturating_sub(context.visible.start + 1) as u16);
            paint_char(context.buffer, left, y, '┌', style);
            paint_char(context.buffer, right, y, '┐', style);
        }
        if rows.end < context.visible.end && context.surface.node_at_row(rows.end).is_none() {
            let y = context
                .area
                .y
                .saturating_add(rows.end.saturating_sub(context.visible.start) as u16);
            paint_char(context.buffer, left, y, '└', style);
            paint_char(context.buffer, right, y, '┘', style);
        }
    }

    fn paint_expandable_indicator(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        surface: &ConversationSurface,
        visible: &Range<usize>,
        columns: Columns,
        viewport: &SurfaceViewport,
    ) {
        for (id, style) in [
            (viewport.hovered(), self.style.hover),
            (viewport.selected(), self.style.selection),
        ]
        .into_iter()
        .filter_map(|(id, style)| id.map(|id| (id, style)))
        {
            let Some(node) = surface
                .node(id)
                .filter(|node| node.is_foldable() && node.display_mode() == DisplayMode::Collapsed)
            else {
                continue;
            };
            if visible.contains(&node.rows().start) {
                let y = area
                    .y
                    .saturating_add(node.rows().start.saturating_sub(visible.start) as u16);
                paint_char(buffer, columns.content.x, y, '›', style);
            }
        }
    }

    fn paint_scrollbar(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        surface: &ConversationSurface,
        viewport: &SurfaceViewport,
        columns: Columns,
    ) {
        let Some(x) = columns.scrollbar else {
            return;
        };
        let height = usize::from(area.height);
        let total = surface.row_count();
        if height == 0 || total <= height {
            return;
        }
        for y in area.y..area.y.saturating_add(area.height) {
            paint_char(buffer, x, y, '│', self.style.scrollbar_track);
        }

        let thumb_height = height
            .saturating_mul(height)
            .div_ceil(total)
            .clamp(1, height);
        let travel = height.saturating_sub(thumb_height);
        let maximum_top = total.saturating_sub(height);
        let thumb_top = viewport
            .top()
            .min(maximum_top)
            .saturating_mul(travel)
            .checked_div(maximum_top)
            .unwrap_or(0);
        let style = if viewport.is_following_bottom() {
            self.style.scrollbar_follow_thumb
        } else {
            self.style.scrollbar_thumb
        };
        for offset in thumb_top..thumb_top.saturating_add(thumb_height) {
            paint_char(buffer, x, area.y.saturating_add(offset as u16), '█', style);
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum BoxExtent {
    Node,
    DenseGroup,
}

#[derive(Debug, Clone, Copy)]
struct Columns {
    content: Rect,
    left_border: Option<u16>,
    rail: Option<u16>,
    right_border: Option<u16>,
    scrollbar: Option<u16>,
}

struct BoxPaintContext<'buffer, 'surface> {
    area: Rect,
    buffer: &'buffer mut Buffer,
    surface: &'surface ConversationSurface,
    visible: &'surface Range<usize>,
    columns: Columns,
}

impl Columns {
    fn for_area(area: Rect) -> Self {
        if area.width >= NORMAL_CHROME_WIDTH.saturating_add(1) {
            return Self {
                content: Rect::new(
                    area.x.saturating_add(3),
                    area.y,
                    area.width.saturating_sub(NORMAL_CHROME_WIDTH),
                    area.height,
                ),
                left_border: Some(area.x),
                rail: Some(area.x.saturating_add(1)),
                right_border: Some(area.right().saturating_sub(2)),
                scrollbar: Some(area.right().saturating_sub(1)),
            };
        }

        let scrollbar = (area.width > 1).then(|| area.right().saturating_sub(1));
        Self {
            content: Rect::new(
                area.x,
                area.y,
                area.width
                    .saturating_sub(if scrollbar.is_some() { 1 } else { 0 }),
                area.height,
            ),
            left_border: None,
            rail: None,
            right_border: None,
            scrollbar,
        }
    }
}

fn visible_rows(
    area: Rect,
    surface: &ConversationSurface,
    viewport: &SurfaceViewport,
) -> Range<usize> {
    let start = viewport.top().min(surface.row_count());
    let end = start
        .saturating_add(usize::from(area.height))
        .min(viewport.end(surface));
    start..end
}

fn clipped_rows(rows: Range<usize>, visible: &Range<usize>) -> Option<Range<usize>> {
    let clipped = rows.start.max(visible.start)..rows.end.min(visible.end);
    (!clipped.is_empty()).then_some(clipped)
}

fn dense_group_rows(surface: &ConversationSurface, selected: &SurfaceNode) -> Range<usize> {
    if !selected.is_groupable() {
        return selected.rows();
    }
    let Some(presentation_group) = selected.presentation_group() else {
        return selected.rows();
    };
    let nodes = surface.nodes();
    let Some(index) = nodes.iter().position(|node| node.id() == selected.id()) else {
        return selected.rows();
    };
    let mut start = index;
    while start > 0
        && nodes[start - 1].is_groupable()
        && nodes[start - 1].presentation_group() == Some(presentation_group)
    {
        start -= 1;
    }
    let mut end = index.saturating_add(1);
    while end < nodes.len()
        && nodes[end].is_groupable()
        && nodes[end].presentation_group() == Some(presentation_group)
    {
        end += 1;
    }
    nodes[start].rows().start..nodes[end - 1].rows().end
}

fn accent_style(node: &SurfaceNode, fallback: Style, collapsed: bool) -> Style {
    let foreground = node
        .rendered()
        .lines()
        .first()
        .and_then(|line| line.line.spans.iter().find_map(|span| span.style.fg));
    let style = foreground.map_or(fallback, |color| fallback.fg(color));
    if collapsed {
        style.add_modifier(Modifier::DIM)
    } else {
        style
    }
}

fn paint_char(buffer: &mut Buffer, x: u16, y: u16, symbol: char, style: Style) {
    if let Some(cell) = buffer.cell_mut((x, y)) {
        cell.set_char(symbol).set_style(style);
    }
}

#[cfg(test)]
#[path = "surface_renderer_tests.rs"]
mod tests;
