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
use astral_tui::TranscriptEntryId;

use crate::history_cell::HistoryCell;
use crate::history_transcript::HistoryEntryId;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::adaptive_wrap_hyperlink_lines;
use crate::wrapping::RtOptions;

const LIVE_TAIL_RAW_ID: u64 = u64::MAX;

pub(crate) struct HistorySurfaceTail<'a> {
    pub(crate) lines: &'a [HyperlinkLine],
    pub(crate) is_stream_continuation: bool,
}

pub(crate) fn materialize_history_surface<'a>(
    entries: impl IntoIterator<Item = (HistoryEntryId, &'a Arc<dyn HistoryCell>)>,
    live_tail: Option<HistorySurfaceTail<'_>>,
    width: u16,
) -> ConversationSurface {
    let committed = entries.into_iter().map(|(id, cell)| {
        materialize_entry(
            TranscriptEntryId::new(id.value()),
            cell.transcript_hyperlink_lines(width),
            spacing(cell.is_stream_continuation()),
            settled_presentation(),
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

fn settled_presentation() -> SurfaceEntryPresentation {
    SurfaceEntryPresentation {
        lifecycle: EntryLifecycle::Restored,
        mode: DisplayMode::Expanded,
        foldable: false,
        groupable: false,
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
