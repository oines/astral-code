//! Projection from authoritative `HistoryCell` terminal rows into Astral's
//! shared conversation surface.
//!
//! Cells remain responsible for producing terminal rows; the surface adds
//! stable identity and viewport navigation without changing cell rendering.

use std::sync::Arc;

use astral_tui::ConversationSurface;
use astral_tui::DisplayMode;
use astral_tui::EntryLifecycle;
use astral_tui::LineJoiner;
use astral_tui::MarkdownLine;
use astral_tui::MarkdownLink;
use astral_tui::MaterializedSurfaceEntry;
use astral_tui::SurfaceEntryPresentation;
use astral_tui::SurfaceEntrySpacing;
use astral_tui::SurfaceRenderer;
use astral_tui::TranscriptEntryId;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::history_cell::HistoryCell;
use crate::history_cell::HistoryRenderMode;
use crate::history_transcript::HistoryEntryId;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::TerminalHyperlink;
use crate::terminal_hyperlinks::adaptive_wrap_hyperlink_lines;
use crate::terminal_hyperlinks::mark_buffer_hyperlinks;
use crate::wrapping::RtOptions;

const LIVE_TAIL_RAW_ID: u64 = u64::MAX;

pub(crate) struct HistorySurfaceTail<'a> {
    pub(crate) lines: &'a [HyperlinkLine],
    pub(crate) is_stream_continuation: bool,
}

#[cfg(test)]
pub(crate) fn materialize_history_surface<'a>(
    entries: impl IntoIterator<Item = (HistoryEntryId, &'a Arc<dyn HistoryCell>)>,
    live_tail: Option<HistorySurfaceTail<'_>>,
    width: u16,
) -> ConversationSurface {
    materialize_history_surface_with_modes(entries, live_tail, width, |_, _| None)
}

pub(crate) fn materialize_history_surface_with_modes<'a>(
    entries: impl IntoIterator<Item = (HistoryEntryId, &'a Arc<dyn HistoryCell>)>,
    live_tail: Option<HistorySurfaceTail<'_>>,
    width: u16,
    mut mode_for: impl FnMut(HistoryEntryId, &dyn HistoryCell) -> Option<DisplayMode>,
) -> ConversationSurface {
    materialize_history_surface_with(entries, live_tail, width, |id, cell, width| {
        let policy = cell.transcript_presentation();
        let mode = policy.normalize(mode_for(id, cell));
        (
            cell.transcript_hyperlink_lines_for_presentation(width, mode),
            settled_presentation(mode, policy.is_foldable(), policy.is_groupable()),
        )
    })
}

/// Project the normal chat display into the shared surface without changing
/// the authoritative `HistoryCell` ordering or rich/raw rendering policy.
pub(crate) fn materialize_history_display_surface<'a>(
    entries: impl IntoIterator<Item = (HistoryEntryId, &'a Arc<dyn HistoryCell>)>,
    live_tail: Option<HistorySurfaceTail<'_>>,
    width: u16,
    mode: HistoryRenderMode,
) -> ConversationSurface {
    materialize_history_surface_with(entries, live_tail, width, |_, cell, width| {
        (
            cell.display_hyperlink_lines_for_mode(width, mode),
            settled_presentation(DisplayMode::Expanded, false, false),
        )
    })
}

fn materialize_history_surface_with<'a>(
    entries: impl IntoIterator<Item = (HistoryEntryId, &'a Arc<dyn HistoryCell>)>,
    live_tail: Option<HistorySurfaceTail<'_>>,
    width: u16,
    mut render_cell: impl FnMut(
        HistoryEntryId,
        &dyn HistoryCell,
        u16,
    ) -> (Vec<HyperlinkLine>, SurfaceEntryPresentation),
) -> ConversationSurface {
    let committed = entries.into_iter().map(|(id, cell)| {
        let (lines, presentation) = render_cell(id, cell.as_ref(), width);
        materialize_entry(
            TranscriptEntryId::new(id.value()),
            lines,
            spacing(cell.is_stream_continuation()),
            presentation,
            width,
        )
    });
    let live = live_tail.filter(|tail| !tail.lines.is_empty()).map(|tail| {
        materialize_entry(
            TranscriptEntryId::new(LIVE_TAIL_RAW_ID),
            tail.lines.to_vec(),
            spacing(tail.is_stream_continuation),
            live_presentation(),
            width,
        )
    });
    ConversationSurface::from_materialized(width, committed.chain(live))
}

/// Render a fixed surface range into terminal-ready rows, preserving the
/// exact styles and chrome painted by [`SurfaceRenderer`].
pub(crate) fn render_history_surface_rows(
    surface: &ConversationSurface,
    outer_width: u16,
    rows: std::ops::Range<usize>,
) -> Vec<HyperlinkLine> {
    if outer_width == 0 || rows.is_empty() {
        return Vec::new();
    }
    let height = u16::try_from(rows.len()).unwrap_or(u16::MAX);
    let area = Rect::new(/*x*/ 0, /*y*/ 0, outer_width, height);
    let mut buffer = Buffer::empty(area);
    SurfaceRenderer::default().render_rows(area, &mut buffer, surface, rows.clone());
    surface_buffer_rows(&buffer, area, surface, rows)
}

/// Reapply semantic links after a live-tail render. Surface rendering keeps
/// link metadata out of the cell symbols so geometry remains escape-free.
pub(crate) fn mark_history_surface_links(
    buffer: &mut Buffer,
    area: Rect,
    surface: &ConversationSurface,
    rows: std::ops::Range<usize>,
) {
    let lines = surface_buffer_rows(buffer, area, surface, rows);
    mark_buffer_hyperlinks(buffer, area, &lines, /*scroll_rows*/ 0);
}

fn surface_buffer_rows(
    buffer: &Buffer,
    area: Rect,
    surface: &ConversationSurface,
    rows: std::ops::Range<usize>,
) -> Vec<HyperlinkLine> {
    let content_x = SurfaceRenderer::content_area(area).x.saturating_sub(area.x) as usize;
    rows.take(usize::from(area.height))
        .enumerate()
        .map(|(offset, surface_row)| {
            let y = area.y.saturating_add(offset as u16);
            let mut cells = (area.left()..area.right())
                .filter_map(|x| {
                    let cell = buffer.cell((x, y))?;
                    (!cell.skip).then(|| (cell.symbol().to_string(), cell.style()))
                })
                .collect::<Vec<_>>();
            while cells.last().is_some_and(|(symbol, style)| {
                symbol.trim().is_empty()
                    && !style
                        .bg
                        .is_some_and(|background| background != Color::Reset)
                    && !style.add_modifier.contains(Modifier::REVERSED)
            }) {
                cells.pop();
            }

            let mut spans: Vec<Span<'static>> = Vec::new();
            for (symbol, style) in cells {
                if let Some(last) = spans.last_mut()
                    && last.style == style
                {
                    last.content.to_mut().push_str(&symbol);
                } else {
                    spans.push(Span::styled(symbol, style));
                }
            }
            let line = Line::from(spans);
            let line_width = line.width();
            let hyperlinks = surface
                .line_at_row(surface_row)
                .into_iter()
                .flat_map(|line| &line.links)
                .filter_map(|link| {
                    let start = content_x.saturating_add(usize::from(link.columns.start));
                    let end = content_x.saturating_add(usize::from(link.columns.end));
                    (start < end && start < line_width).then(|| TerminalHyperlink {
                        columns: start..end.min(line_width),
                        destination: link.target.clone(),
                    })
                })
                .collect();
            HyperlinkLine { line, hyperlinks }
        })
        .collect()
}

fn materialize_entry(
    id: TranscriptEntryId,
    terminal_rows: Vec<HyperlinkLine>,
    spacing: SurfaceEntrySpacing,
    presentation: SurfaceEntryPresentation,
    width: u16,
) -> MaterializedSurfaceEntry {
    MaterializedSurfaceEntry::ungrouped(
        id,
        spacing,
        presentation,
        materialize_terminal_rows(terminal_rows, width),
    )
}

fn materialize_terminal_rows(lines: Vec<HyperlinkLine>, width: u16) -> Vec<MarkdownLine> {
    let mut next_link_id = 0u32;
    lines
        .into_iter()
        .flat_map(|line| {
            if line.width() <= usize::from(width) {
                vec![line]
            } else {
                adaptive_wrap_hyperlink_lines(
                    std::slice::from_ref(&line),
                    RtOptions::new(usize::from(width).max(1)),
                )
            }
        })
        .map(|line| {
            let links = line
                .hyperlinks
                .into_iter()
                .filter_map(|link| {
                    let start = u16::try_from(link.columns.start).ok()?;
                    let end = u16::try_from(link.columns.end).ok()?;
                    (start < end).then(|| {
                        let id = next_link_id;
                        next_link_id = next_link_id.wrapping_add(1);
                        MarkdownLink {
                            id,
                            columns: start..end,
                            target: link.destination,
                        }
                    })
                })
                .collect();
            MarkdownLine {
                line: line.line,
                joiner_to_previous: LineJoiner::HardBreak,
                links,
            }
        })
        .collect()
}

fn spacing(is_stream_continuation: bool) -> SurfaceEntrySpacing {
    if is_stream_continuation {
        SurfaceEntrySpacing::Continue
    } else {
        SurfaceEntrySpacing::Separate
    }
}

fn settled_presentation(
    mode: DisplayMode,
    foldable: bool,
    groupable: bool,
) -> SurfaceEntryPresentation {
    SurfaceEntryPresentation {
        lifecycle: EntryLifecycle::Restored,
        mode,
        foldable,
        groupable,
        turn_settled: true,
        presentation_stable: true,
    }
}

fn live_presentation() -> SurfaceEntryPresentation {
    SurfaceEntryPresentation {
        lifecycle: EntryLifecycle::Running {
            started_at_ms: None,
        },
        mode: DisplayMode::Expanded,
        foldable: false,
        groupable: false,
        turn_settled: false,
        presentation_stable: false,
    }
}

#[cfg(test)]
#[path = "history_surface_tests.rs"]
mod tests;
