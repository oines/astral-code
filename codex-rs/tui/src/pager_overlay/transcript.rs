use super::*;

struct CellRenderable {
    cell: Arc<dyn HistoryCell>,
    style: Style,
}

impl Renderable for CellRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let hyperlink_lines = self.cell.transcript_hyperlink_lines(area.width);
        let p = Paragraph::new(Text::from(visible_lines(hyperlink_lines.clone())))
            .style(self.style)
            .wrap(Wrap { trim: false });
        p.render(area, buf);
        mark_buffer_hyperlinks(buf, area, &hyperlink_lines, /*scroll_rows*/ 0);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.cell.desired_transcript_height(width)
    }
}

struct HyperlinkLinesRenderable {
    lines: Vec<HyperlinkLine>,
}

impl Renderable for HyperlinkLinesRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(Text::from(visible_lines(self.lines.clone())))
            .wrap(Wrap { trim: false })
            .render(area, buf);
        mark_buffer_hyperlinks(buf, area, &self.lines, /*scroll_rows*/ 0);
    }

    fn desired_height(&self, width: u16) -> u16 {
        Paragraph::new(Text::from(visible_lines(self.lines.clone())))
            .wrap(Wrap { trim: false })
            .line_count(width)
            .try_into()
            .unwrap_or(/*default*/ 0)
    }
}

pub(crate) struct TranscriptOverlay {
    /// Pager UI state and the renderables currently displayed.
    ///
    /// The invariant is that `view.renderables` is `render_cells(cells)` plus an optional trailing
    /// live-tail renderable appended after the committed cells.
    pub(super) view: PagerView,
    /// Committed transcript cells (does not include the live tail).
    pub(super) cells: TranscriptEntries,
    /// Cache key for the render-only live tail appended after committed cells.
    live_tail_key: Option<LiveTailKey>,
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
    /// Creates a transcript overlay for a fixed set of committed cells.
    ///
    /// This overlay does not own the "active cell"; callers may optionally append a live tail via
    /// `sync_live_tail` during draws to reflect in-flight activity.
    pub(crate) fn new(
        transcript_cells: Vec<(HistoryEntryId, Arc<dyn HistoryCell>)>,
        keymap: PagerKeymap,
    ) -> Self {
        let cells = TranscriptEntries::new(transcript_cells);
        Self {
            view: PagerView::new(
                Self::render_cells(&cells),
                "T R A N S C R I P T".to_string(),
                usize::MAX,
                keymap,
            ),
            cells,
            live_tail_key: None,
            is_done: false,
        }
    }

    fn render_cells(cells: &TranscriptEntries) -> Vec<Box<dyn Renderable>> {
        cells
            .iter()
            .enumerate()
            .flat_map(|(index, entry)| {
                let mut v: Vec<Box<dyn Renderable>> = Vec::new();
                let mut cell_renderable = if entry.cell().as_any().is::<UserHistoryCell>() {
                    Box::new(CachedRenderable::new(CellRenderable {
                        cell: entry.cell().clone(),
                        style: if cells.highlighted() == Some(entry.id()) {
                            user_message_style().reversed()
                        } else {
                            user_message_style()
                        },
                    })) as Box<dyn Renderable>
                } else {
                    Box::new(CachedRenderable::new(CellRenderable {
                        cell: entry.cell().clone(),
                        style: Style::default(),
                    })) as Box<dyn Renderable>
                };
                if !entry.cell().is_stream_continuation() && index > 0 {
                    cell_renderable = Box::new(InsetRenderable::new(
                        cell_renderable,
                        Insets::tlbr(
                            /*top*/ 1, /*left*/ 0, /*bottom*/ 0, /*right*/ 0,
                        ),
                    ));
                }
                v.push(cell_renderable);
                v
            })
            .collect()
    }

    /// Insert a committed history cell while keeping any cached live tail.
    ///
    /// The live tail is temporarily removed, the committed cells are rebuilt,
    /// then the tail is reattached. If the tail previously had no leading
    /// spacing because it was the only renderable, we add the missing inset
    /// when the first committed cell arrives.
    ///
    /// This expects `cell` to be a committed transcript cell (not the in-flight active cell). If
    /// the overlay was scrolled to bottom before insertion, it remains pinned to bottom after the
    /// insertion to preserve the "follow along" behavior.
    pub(crate) fn insert_cell(&mut self, id: HistoryEntryId, cell: Arc<dyn HistoryCell>) {
        let follow_bottom = self.view.is_scrolled_to_bottom();
        let had_prior_cells = !self.cells.is_empty();
        let tail_renderable = self.take_live_tail_renderable();
        self.cells.insert(id, cell);
        self.view.renderables = Self::render_cells(&self.cells);
        if let Some(tail) = tail_renderable {
            let tail = if !had_prior_cells
                && self
                    .live_tail_key
                    .is_some_and(|key| !key.is_stream_continuation)
            {
                // The tail was rendered as the only entry, so it lacks a top
                // inset; add one now that it follows a committed cell.
                Box::new(InsetRenderable::new(
                    tail,
                    Insets::tlbr(
                        /*top*/ 1, /*left*/ 0, /*bottom*/ 0, /*right*/ 0,
                    ),
                )) as Box<dyn Renderable>
            } else {
                tail
            };
            self.view.renderables.push(tail);
        }
        if follow_bottom {
            self.view.scroll_offset = usize::MAX;
        }
    }

    /// Replace committed transcript cells while keeping any cached in-progress output that is
    /// currently shown at the end of the overlay.
    ///
    /// This is used when existing history is trimmed (for example after rollback) so the
    /// transcript overlay immediately reflects the same committed cells as the main transcript.
    pub(crate) fn replace_cells(&mut self, cells: Vec<(HistoryEntryId, Arc<dyn HistoryCell>)>) {
        let follow_bottom = self.view.is_scrolled_to_bottom();
        self.cells.replace(cells);
        self.rebuild_renderables();
        if follow_bottom {
            self.view.scroll_offset = usize::MAX;
        }
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
        let follow_bottom = self.view.is_scrolled_to_bottom();
        if self.cells.consolidate(range, consolidated) {
            self.rebuild_renderables();
        }
        if follow_bottom {
            self.view.scroll_offset = usize::MAX;
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
        width: u16,
        active_key: Option<ActiveCellTranscriptKey>,
        compute_lines: impl FnOnce(u16) -> Option<Vec<HyperlinkLine>>,
    ) {
        let next_key = active_key.map(|key| LiveTailKey {
            width,
            revision: key.revision,
            is_stream_continuation: key.is_stream_continuation,
            animation_tick: key.animation_tick,
        });

        if self.live_tail_key == next_key {
            return;
        }
        let follow_bottom = self.view.is_scrolled_to_bottom();

        self.take_live_tail_renderable();
        self.live_tail_key = next_key;

        if let Some(key) = next_key {
            let lines = compute_lines(width).unwrap_or_default();
            if !lines.is_empty() {
                self.view.renderables.push(Self::live_tail_renderable(
                    lines,
                    !self.cells.is_empty(),
                    key.is_stream_continuation,
                ));
            }
        }
        if follow_bottom {
            self.view.scroll_offset = usize::MAX;
        }
    }

    pub(crate) fn set_highlight_cell(&mut self, cell: Option<usize>) {
        self.cells.set_highlight_index(cell);
        self.rebuild_renderables();
        if let Some(index) = self.cells.highlighted_index() {
            self.view.scroll_chunk_into_view(index);
        }
    }

    /// Returns whether the underlying pager view is currently pinned to the bottom.
    ///
    /// The `App` draw loop uses this to decide whether to schedule animation frames for the live
    /// tail; if the user has scrolled up, we avoid driving animation work that they cannot see.
    pub(crate) fn is_scrolled_to_bottom(&self) -> bool {
        self.view.is_scrolled_to_bottom()
    }

    fn rebuild_renderables(&mut self) {
        let tail_renderable = self.take_live_tail_renderable();
        self.view.renderables = Self::render_cells(&self.cells);
        if let Some(tail) = tail_renderable {
            self.view.renderables.push(tail);
        }
    }

    /// Removes and returns the cached live-tail renderable, if present.
    ///
    /// The live tail is represented as a single optional renderable appended after the committed
    /// cell renderables, so this relies on the live tail always being the final entry in
    /// `view.renderables` when present.
    fn take_live_tail_renderable(&mut self) -> Option<Box<dyn Renderable>> {
        (self.view.renderables.len() > self.cells.len()).then(|| self.view.renderables.pop())?
    }

    fn live_tail_renderable(
        lines: Vec<HyperlinkLine>,
        has_prior_cells: bool,
        is_stream_continuation: bool,
    ) -> Box<dyn Renderable> {
        let mut renderable: Box<dyn Renderable> =
            Box::new(CachedRenderable::new(HyperlinkLinesRenderable { lines }));
        if has_prior_cells && !is_stream_continuation {
            renderable = Box::new(InsetRenderable::new(
                renderable,
                Insets::tlbr(
                    /*top*/ 1, /*left*/ 0, /*bottom*/ 0, /*right*/ 0,
                ),
            ));
        }
        renderable
    }

    fn render_hints(&self, area: Rect, buf: &mut Buffer) {
        let line1 = Rect::new(area.x, area.y, area.width, 1);
        let line2 = Rect::new(area.x, area.y.saturating_add(1), area.width, 1);
        render_key_hints(
            line1,
            buf,
            &[
                (
                    first_or_empty(&self.view.keymap.scroll_up)
                        .into_iter()
                        .chain(first_or_empty(&self.view.keymap.scroll_down))
                        .collect(),
                    "to scroll",
                ),
                (
                    first_or_empty(&self.view.keymap.page_up)
                        .into_iter()
                        .chain(first_or_empty(&self.view.keymap.page_down))
                        .collect(),
                    "to page",
                ),
                (
                    first_or_empty(&self.view.keymap.jump_top)
                        .into_iter()
                        .chain(first_or_empty(&self.view.keymap.jump_bottom))
                        .collect(),
                    "to jump",
                ),
            ],
        );

        let mut pairs: Vec<(Vec<KeyBinding>, &str)> =
            vec![(first_or_empty(&self.view.keymap.close), "to quit")];
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
        }
        render_key_hints(line2, buf, &pairs);
    }

    pub(crate) fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let top_h = area.height.saturating_sub(3);
        let top = Rect::new(area.x, area.y, area.width, top_h);
        let bottom = Rect::new(area.x, area.y + top_h, area.width, 3);
        self.view.render(top, buf);
        self.render_hints(bottom, buf);
    }
}

impl TranscriptOverlay {
    pub(crate) fn handle_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        match event {
            TuiEvent::Key(key_event) => match key_event {
                e if self.view.keymap.close.is_pressed(e)
                    || self.view.keymap.close_transcript.is_pressed(e) =>
                {
                    self.is_done = true;
                    Ok(())
                }
                other => self.view.handle_key_event(tui, other),
            },
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
