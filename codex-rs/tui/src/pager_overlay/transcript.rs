use super::*;
use std::time::Instant;

use astral_tui::BlockViewerHost;
use astral_tui::BlockViewerOutcome;
use astral_tui::SurfacePointer;
use crossterm::event::MouseEvent;

pub(crate) struct TranscriptOverlay {
    pub(super) surface: ConversationSurface,
    pub(super) viewport: SurfaceViewport,
    renderer: SurfaceRenderer,
    pub(super) cells: TranscriptEntries,
    display: TranscriptDisplayState,
    pointer: SurfacePointer,
    viewer: Option<BlockViewerHost>,
    pending_copy: Option<String>,
    clipboard_lease: Option<crate::clipboard_copy::ClipboardLease>,
    live_tail_key: Option<LiveTailKey>,
    live_tail_lines: Vec<HyperlinkLine>,
    surface_dirty: bool,
    keymap: PagerKeymap,
    is_done: bool,
}

/// Cache key for the active-cell "live tail" appended to the transcript overlay.
///
/// Changing any field implies a different rendered tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LiveTailKey {
    /// Current terminal width, which affects wrapping.
    width: u16,
    /// Revision that changes on in-place active cell transcript updates.
    revision: u64,
    /// Whether the tail should be treated as a continuation for spacing.
    is_stream_continuation: bool,
    /// Optional animation tick to refresh spinners/progress indicators.
    animation_tick: Option<u64>,
}

impl TranscriptOverlay {
    pub(crate) fn new(
        transcript_cells: Vec<(HistoryEntryId, Arc<dyn HistoryCell>)>,
        keymap: PagerKeymap,
    ) -> Self {
        Self {
            surface: ConversationSurface::from_materialized(1, std::iter::empty()),
            viewport: SurfaceViewport::default(),
            renderer: SurfaceRenderer::default(),
            cells: TranscriptEntries::new(transcript_cells),
            display: TranscriptDisplayState::default(),
            pointer: SurfacePointer::default(),
            viewer: None,
            pending_copy: None,
            clipboard_lease: None,
            live_tail_key: None,
            live_tail_lines: Vec::new(),
            surface_dirty: true,
            keymap,
            is_done: false,
        }
    }

    pub(crate) fn insert_cell(&mut self, id: HistoryEntryId, cell: Arc<dyn HistoryCell>) {
        self.cells.insert(id, cell);
        self.surface_dirty = true;
    }

    /// Replace committed transcript cells while keeping any cached in-progress output that is
    /// currently shown at the end of the overlay.
    ///
    /// This is used when existing history is trimmed (for example after rollback) so the
    /// transcript overlay immediately reflects the same committed cells as the main transcript.
    pub(crate) fn replace_cells(&mut self, cells: Vec<(HistoryEntryId, Arc<dyn HistoryCell>)>) {
        self.cells.replace(cells);
        self.display.retain(|id| self.cells.contains(id));
        self.reconcile_viewer();
        self.surface_dirty = true;
    }

    /// Replace a range of committed cells with a single consolidated cell.
    ///
    /// Mirrors the splice performed on `App::transcript_cells` during
    /// `ConsolidateAgentMessage` so the Ctrl+T overlay stays in sync with the
    /// main transcript. The range is clamped defensively: cells may have been
    /// inserted after the overlay opened, leaving it with fewer entries than
    /// the main transcript.
    pub(crate) fn consolidate_cells(
        &mut self,
        range: std::ops::Range<usize>,
        consolidated: Arc<dyn HistoryCell>,
    ) {
        if self.cells.consolidate(range, consolidated) {
            self.display.retain(|id| self.cells.contains(id));
            self.reconcile_viewer();
            self.surface_dirty = true;
        }
    }

    /// Sync the active-cell live tail with the current width and cell state.
    ///
    /// Recomputes the tail only when the cache key changes, preserving scroll
    /// position and dropping the tail if there is nothing to render.
    ///
    /// The overlay owns committed transcript cells while the live tail is derived from the current
    /// active cell, which can mutate in place while streaming. `App` calls this during
    /// `TuiEvent::Draw` for `Overlay::Transcript`, passing a key that changes when the active cell
    /// mutates or animates so the cached tail stays fresh.
    ///
    /// Passing a key that does not change on in-place active-cell mutations will freeze the tail in
    /// `Ctrl+T` while the main viewport continues to update.
    pub(crate) fn sync_live_tail(
        &mut self,
        area: Rect,
        active_key: Option<ActiveCellTranscriptKey>,
        compute_lines: impl FnOnce(u16) -> Option<Vec<HyperlinkLine>>,
    ) {
        let width = SurfaceRenderer::content_width(Self::conversation_area(area));
        let next_key = active_key.map(|key| LiveTailKey {
            width,
            revision: key.revision,
            is_stream_continuation: key.is_stream_continuation,
            animation_tick: key.animation_tick,
        });

        if self.live_tail_key == next_key {
            return;
        }
        self.live_tail_key = next_key;
        self.live_tail_lines = next_key
            .and_then(|_| compute_lines(width))
            .unwrap_or_default();
        self.surface_dirty = true;
    }

    pub(crate) fn set_highlight_cell(&mut self, cell: Option<usize>) {
        self.cells.set_highlight_index(cell);
        self.sync_highlight();
    }

    /// Returns whether the shared surface viewport is currently pinned to the bottom.
    ///
    /// The `App` draw loop uses this to decide whether to schedule animation frames for the live
    /// tail; if the user has scrolled up, we avoid driving animation work that they cannot see.
    pub(crate) fn is_scrolled_to_bottom(&self) -> bool {
        self.viewport.is_following_bottom()
    }

    pub(super) fn ensure_surface(&mut self, area: Rect) {
        let width = SurfaceRenderer::content_width(area);
        if self.surface_dirty || self.surface.width() != width {
            let tail = self.live_tail_key.map(|key| HistorySurfaceTail {
                lines: self.live_tail_lines.as_slice(),
                is_stream_continuation: key.is_stream_continuation,
            });
            let cells = &self.cells;
            let display = &mut self.display;
            self.surface = crate::history_surface::materialize_history_surface_with_modes(
                cells.iter().map(|entry| (entry.id(), entry.cell())),
                tail,
                width,
                |id, cell| Some(display.mode_for(id, cell)),
            );
            self.surface_dirty = false;
        }
        self.viewport.prepare(&self.surface, area.height);
        self.pointer.prepare(area, &self.surface);
        if self.cells.highlighted().is_some() {
            self.sync_highlight();
        }
    }

    fn sync_highlight(&mut self) {
        let selected = self
            .cells
            .highlighted()
            .map(|id| SurfaceNodeId::Entry(astral_tui::TranscriptEntryId::new(id.value())));
        self.viewport.select_node(&self.surface, selected);
    }

    pub(super) fn apply_key_event(&mut self, viewport_area: Rect, key_event: KeyEvent) -> bool {
        if self.viewer.is_some() {
            return self.apply_viewer_key_event(key_event);
        }
        self.ensure_surface(Self::conversation_area(viewport_area));
        match key_event {
            event if self.keymap.scroll_up.is_pressed(event) => self
                .viewport
                .move_selection(&self.surface, ScrollDirection::Up),
            event if self.keymap.scroll_down.is_pressed(event) => self
                .viewport
                .move_selection(&self.surface, ScrollDirection::Down),
            event if self.keymap.page_up.is_pressed(event) => self
                .viewport
                .scroll_page(&self.surface, ScrollDirection::Up),
            event if self.keymap.page_down.is_pressed(event) => self
                .viewport
                .scroll_page(&self.surface, ScrollDirection::Down),
            event if self.keymap.half_page_up.is_pressed(event) => self.viewport.scroll_rows(
                &self.surface,
                ScrollDirection::Up,
                usize::from(self.viewport.height().saturating_add(1) / 2),
            ),
            event if self.keymap.half_page_down.is_pressed(event) => self.viewport.scroll_rows(
                &self.surface,
                ScrollDirection::Down,
                usize::from(self.viewport.height().saturating_add(1) / 2),
            ),
            event if self.keymap.jump_top.is_pressed(event) => {
                let scrolled = self.viewport.scroll_to_top(&self.surface);
                let selected = self.viewport.select_first(&self.surface);
                scrolled || selected
            }
            event if self.keymap.jump_bottom.is_pressed(event) => {
                let scrolled = self.viewport.scroll_to_bottom(&self.surface);
                let selected = self.viewport.select_last(&self.surface);
                scrolled || selected
            }
            event if key_hint::plain(KeyCode::Left).is_press(event) => {
                self.apply_fold_action(FoldAction::Collapse)
            }
            event if key_hint::plain(KeyCode::Right).is_press(event) => {
                self.apply_fold_action(FoldAction::Expand)
            }
            event if key_hint::plain(KeyCode::Char('e')).is_press(event) => {
                self.apply_fold_action(FoldAction::Toggle)
            }
            event if key_hint::plain(KeyCode::Enter).is_press(event) => self.open_selected_viewer(),
            _ => false,
        }
    }

    fn apply_viewer_key_event(&mut self, key_event: KeyEvent) -> bool {
        let outcome = {
            let Some(viewer) = self.viewer.as_mut() else {
                return false;
            };
            viewer.handle_key_event(key_event, &self.cells)
        };
        self.apply_viewer_outcome(outcome)
    }

    fn apply_viewer_mouse_event(&mut self, mouse: MouseEvent) -> bool {
        let outcome = {
            let Some(viewer) = self.viewer.as_mut() else {
                return false;
            };
            viewer.handle_mouse_event(mouse, &self.cells)
        };
        self.apply_viewer_outcome(outcome)
    }

    fn apply_viewer_outcome(&mut self, outcome: BlockViewerOutcome) -> bool {
        match outcome {
            BlockViewerOutcome::Unchanged => false,
            BlockViewerOutcome::Changed => true,
            BlockViewerOutcome::Close => {
                self.viewer = None;
                true
            }
            BlockViewerOutcome::Copy(text) => {
                self.pending_copy = Some(text);
                true
            }
        }
    }

    fn open_selected_viewer(&mut self) -> bool {
        let Some(node) = self.viewport.selected() else {
            return false;
        };
        let Some(viewer) = BlockViewerHost::open(&self.cells, node) else {
            return false;
        };
        self.viewer = Some(viewer);
        true
    }

    fn reconcile_viewer(&mut self) {
        if self
            .viewer
            .as_ref()
            .is_some_and(|viewer| !viewer.is_available(&self.cells))
        {
            self.viewer = None;
        }
    }

    fn apply_fold_action(&mut self, action: FoldAction) -> bool {
        let Some(node) = self.viewport.selected() else {
            return false;
        };
        self.apply_node_fold_action(node, action)
    }

    fn apply_node_fold_action(&mut self, node: SurfaceNodeId, action: FoldAction) -> bool {
        let SurfaceNodeId::Entry(id) = node else {
            return false;
        };
        let Some(entry) = self.cells.get_by_surface_id(id) else {
            return false;
        };
        if !self
            .display
            .apply(entry.id(), entry.cell().as_ref(), action)
        {
            return false;
        }
        self.surface_dirty = true;
        true
    }

    pub(super) fn apply_mouse_event(
        &mut self,
        viewport_area: Rect,
        mouse: MouseEvent,
        now: Instant,
    ) -> bool {
        if self.viewer.is_some() {
            return self.apply_viewer_mouse_event(mouse);
        }
        // Backtrack preview owns selection semantics. Keep its existing key-only state machine
        // isolated from pointer selection until it has a dedicated interaction design.
        if self.cells.highlighted().is_some() {
            return false;
        }
        let area = Self::conversation_area(viewport_area);
        self.ensure_surface(area);
        let outcome =
            self.pointer
                .handle_event(mouse, now, area, &self.surface, &mut self.viewport);
        let folded = outcome
            .activated()
            .is_some_and(|node| self.apply_node_fold_action(node, FoldAction::Toggle));
        outcome.changed() || folded
    }

    fn handle_key_event(&mut self, tui: &mut tui::Tui, key_event: KeyEvent) {
        let changed = self.apply_key_event(tui.terminal.viewport_area, key_event);
        self.finish_input(tui, changed);
    }

    fn handle_mouse_event(&mut self, tui: &mut tui::Tui, mouse: MouseEvent) {
        let changed = self.apply_mouse_event(tui.terminal.viewport_area, mouse, Instant::now());
        self.finish_input(tui, changed);
    }

    fn finish_input(&mut self, tui: &mut tui::Tui, mut changed: bool) {
        if let Some(text) = self.pending_copy.take() {
            match crate::clipboard_copy::copy_to_clipboard(&text) {
                Ok(lease) => self.clipboard_lease = lease,
                Err(error) => tracing::warn!("failed to copy transcript viewer content: {error}"),
            }
            changed = true;
        }
        if changed {
            tui.frame_requester()
                .schedule_frame_in(crate::tui::TARGET_FRAME_INTERVAL);
        }
    }

    fn render_hints(&self, area: Rect, buf: &mut Buffer) {
        let line1 = Rect::new(area.x, area.y, area.width, 1);
        let line2 = Rect::new(area.x, area.y.saturating_add(1), area.width, 1);
        render_key_hints(
            line1,
            buf,
            &[
                (
                    first_or_empty(&self.keymap.scroll_up)
                        .into_iter()
                        .chain(first_or_empty(&self.keymap.scroll_down))
                        .collect(),
                    "to select",
                ),
                (
                    first_or_empty(&self.keymap.page_up)
                        .into_iter()
                        .chain(first_or_empty(&self.keymap.page_down))
                        .collect(),
                    "to page",
                ),
                (
                    first_or_empty(&self.keymap.jump_top)
                        .into_iter()
                        .chain(first_or_empty(&self.keymap.jump_bottom))
                        .collect(),
                    "to jump",
                ),
            ],
        );

        let mut pairs: Vec<(Vec<KeyBinding>, &str)> =
            vec![(first_or_empty(&self.keymap.close), "to quit")];
        if self.cells.highlighted().is_some() {
            pairs.push((
                vec![
                    key_hint::plain(KeyCode::Esc),
                    key_hint::plain(KeyCode::Left),
                ],
                "to edit prev",
            ));
            pairs.push((vec![key_hint::plain(KeyCode::Right)], "to edit next"));
            pairs.push((vec![key_hint::plain(KeyCode::Enter)], "to edit message"));
        } else {
            pairs.push((vec![key_hint::plain(KeyCode::Esc)], "to edit prev"));
            if self
                .viewport
                .selected()
                .and_then(|id| self.surface.node(id))
                .is_some_and(astral_tui::SurfaceNode::is_foldable)
            {
                pairs.push((
                    vec![
                        key_hint::plain(KeyCode::Left),
                        key_hint::plain(KeyCode::Right),
                    ],
                    "to fold",
                ));
                pairs.push((vec![key_hint::plain(KeyCode::Char('e'))], "to toggle"));
            }
            if self
                .viewport
                .selected()
                .and_then(|node| BlockViewerHost::open(&self.cells, node))
                .is_some()
            {
                pairs.push((vec![key_hint::plain(KeyCode::Enter)], "to open"));
            }
        }
        render_key_hints(line2, buf, &pairs);
    }

    pub(crate) fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let top_h = area.height.saturating_sub(3);
        let top = Rect::new(area.x, area.y, area.width, top_h);
        let bottom = Rect::new(area.x, area.y + top_h, area.width, 3);
        self.ensure_surface(top);
        self.renderer
            .render(top, buf, &self.surface, &self.viewport);
        self.mark_visible_hyperlinks(top, buf);
        self.render_hints(bottom, buf);
        if let Some(viewer) = self.viewer.as_mut()
            && !viewer.render(buf, area, &self.cells)
        {
            self.viewer = None;
        }
    }

    fn mark_visible_hyperlinks(&self, area: Rect, buf: &mut Buffer) {
        let content_area = SurfaceRenderer::content_area(area);
        let lines = self
            .viewport
            .visible_rows(&self.surface)
            .filter_map(|row| self.surface.line_at_row(row))
            .map(|line| HyperlinkLine {
                line: line.line.clone(),
                hyperlinks: line
                    .links
                    .iter()
                    .map(|link| TerminalHyperlink {
                        columns: usize::from(link.columns.start)..usize::from(link.columns.end),
                        destination: link.target.clone(),
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        mark_buffer_hyperlinks(buf, content_area, &lines, /*scroll_rows*/ 0);
    }

    pub(super) fn conversation_area(area: Rect) -> Rect {
        Rect::new(area.x, area.y, area.width, area.height.saturating_sub(3))
    }

    pub(crate) fn handle_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        match event {
            TuiEvent::Key(key_event) if self.viewer.is_some() => {
                self.handle_key_event(tui, key_event);
                Ok(())
            }
            TuiEvent::Key(key_event) => match key_event {
                e if self.keymap.close.is_pressed(e)
                    || self.keymap.close_transcript.is_pressed(e) =>
                {
                    self.is_done = true;
                    Ok(())
                }
                other => {
                    self.handle_key_event(tui, other);
                    Ok(())
                }
            },
            TuiEvent::Mouse(mouse) => {
                self.handle_mouse_event(tui, mouse);
                Ok(())
            }
            TuiEvent::Draw | TuiEvent::Resize => {
                tui.draw(u16::MAX, |frame| {
                    self.render(frame.area(), frame.buffer);
                })?;
                Ok(())
            }
            _ => Ok(()),
        }
    }
    pub(crate) fn is_done(&self) -> bool {
        self.is_done
    }

    #[cfg(test)]
    pub(crate) fn committed_cell_count(&self) -> usize {
        self.cells.len()
    }
}
