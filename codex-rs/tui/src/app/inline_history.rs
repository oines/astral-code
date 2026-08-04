//! Shared-Surface host for the normal inline conversation view.
//!
//! `ChatWidget` and `HistoryCell` remain the authoritative protocol projection.
//! This module only materializes their ordered display rows, advances the
//! print-once terminal frontier, and snapshots the remaining live suffix.

use std::io;

use astral_tui::InlineHost;
use astral_tui::SurfaceRenderer;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::chatwidget::ChatWidget;
use crate::history_surface::HistorySurfaceTail;
use crate::history_surface::mark_history_surface_links;
use crate::history_surface::materialize_history_display_surface;
use crate::history_surface::render_history_surface_rows;
use crate::history_transcript::HistoryTranscript;
use crate::insert_history::HistoryLineWrapPolicy;
use crate::insert_history::InsertHistoryMode;
use crate::tui;

#[derive(Debug, Clone, Copy)]
enum ReplayLimit {
    Full,
    Tail(usize),
    SkipRows(usize),
}

#[derive(Default)]
pub(super) struct InlineHistoryState {
    host: Option<InlineHost>,
    replay_limit: Option<ReplayLimit>,
    replay_loading: bool,
}

impl InlineHistoryState {
    pub(super) fn refresh(
        &mut self,
        transcript: &HistoryTranscript,
        chat_widget: &ChatWidget,
        outer_width: u16,
    ) {
        let surface_width = SurfaceRenderer::content_width(Rect::new(
            /*x*/ 0,
            /*y*/ 0,
            outer_width,
            /*height*/ 1,
        ));
        let live_lines = chat_widget.active_cell_display_hyperlink_lines(surface_width);
        let live_tail = live_lines.as_deref().map(|lines| HistorySurfaceTail {
            lines,
            is_stream_continuation: chat_widget
                .active_cell_transcript_key()
                .is_some_and(|key| key.is_stream_continuation),
        });
        let surface = materialize_history_display_surface(
            transcript.entries(),
            live_tail,
            surface_width,
            chat_widget.history_render_mode(),
        );
        let thread_id = chat_widget
            .thread_id()
            .map(|id| id.to_string())
            .unwrap_or_else(|| "pending-thread".to_string());
        if let Some(host) = &mut self.host {
            host.refresh_materialized_surface(thread_id, surface);
        } else {
            self.host = Some(InlineHost::from_surface(thread_id, surface));
        }
    }

    pub(super) fn reset_for_replay(&mut self, max_rows: Option<usize>) {
        self.host = None;
        self.replay_limit = Some(max_rows.map_or(ReplayLimit::Full, ReplayLimit::Tail));
        self.replay_loading = false;
    }

    pub(super) fn begin_replay(&mut self, max_rows: Option<usize>) {
        self.reset_for_replay(max_rows);
        self.replay_loading = true;
    }

    pub(super) fn finish_replay(&mut self) {
        self.replay_loading = false;
    }

    pub(super) fn clear(&mut self) {
        self.host = None;
        self.replay_limit = None;
        self.replay_loading = false;
    }

    pub(super) fn live_tail_height(&self, available_rows: u16) -> u16 {
        self.host
            .as_ref()
            .map_or(0, |host| host.live_tail_height(available_rows))
    }

    pub(super) fn live_tail_snapshot(&self, outer_width: u16, height: u16) -> Buffer {
        let area = Rect::new(/*x*/ 0, /*y*/ 0, outer_width, height);
        let mut buffer = Buffer::empty(area);
        let Some(host) = &self.host else {
            return buffer;
        };
        let tail_start = host.projected_tail_start();
        let visible_start = host
            .surface()
            .row_count()
            .saturating_sub(usize::from(height))
            .max(tail_start);
        let visible = visible_start..host.surface().row_count();
        SurfaceRenderer::default().render_rows(area, &mut buffer, host.surface(), visible.clone());
        mark_history_surface_links(&mut buffer, area, host.surface(), visible);
        buffer
    }

    pub(super) fn commit(
        &mut self,
        terminal: &mut tui::Terminal,
        outer_width: u16,
        insert_mode: InsertHistoryMode,
        wrap_policy: HistoryLineWrapPolicy,
    ) -> io::Result<()> {
        let Some(host) = &mut self.host else {
            return Ok(());
        };
        if self.replay_loading {
            return Ok(());
        }
        let mut skip_rows = match self.replay_limit.take() {
            Some(ReplayLimit::Full) | None => 0,
            Some(ReplayLimit::Tail(max_rows)) => {
                host.projected_tail_start().saturating_sub(max_rows)
            }
            Some(ReplayLimit::SkipRows(rows)) => rows,
        };
        let mut retry_skip_rows = None;
        let result = host.commit_with(|surface, mut rows| {
            let skipped = skip_rows.min(rows.len());
            rows.start = rows.start.saturating_add(skipped);
            skip_rows = skip_rows.saturating_sub(skipped);
            if rows.is_empty() {
                return Ok(());
            }
            let lines = render_history_surface_rows(surface, outer_width, rows);
            let result =
                crate::insert_history::insert_history_hyperlink_lines_with_mode_and_wrap_policy(
                    terminal,
                    lines,
                    insert_mode,
                    wrap_policy,
                );
            if result.is_err() {
                retry_skip_rows = Some(skip_rows.saturating_add(skipped));
            }
            result
        });
        if let Some(rows) = retry_skip_rows {
            self.replay_limit = Some(ReplayLimit::SkipRows(rows));
        }
        result?;
        Ok(())
    }
}

pub(super) fn render_snapshot(snapshot: &Buffer, area: Rect, target: &mut Buffer) {
    let width = snapshot.area.width.min(area.width);
    let height = snapshot.area.height.min(area.height);
    for y in 0..height {
        for x in 0..width {
            target[(area.x.saturating_add(x), area.y.saturating_add(y))] = snapshot[(
                snapshot.area.x.saturating_add(x),
                snapshot.area.y.saturating_add(y),
            )]
                .clone();
        }
    }
}
