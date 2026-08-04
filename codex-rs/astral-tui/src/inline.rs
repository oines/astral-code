//! Terminal-native inline host over the shared conversation surface.
//!
//! Finalized nodes cross a print-once frontier into the terminal's native
//! scrollback. Everything after the first unstable node remains in the pinned
//! live region. Projection and rendering stay shared with fullscreen mode;
//! this module owns only commit bookkeeping and terminal insertion policy.

use std::collections::HashSet;
use std::io;
use std::ops::Range;

use astral_terminal_inline::Terminal;
use astral_tui_scrollback::DisplayMode;
use astral_tui_scrollback::EntryLifecycle;
use astral_tui_scrollback::EntryRenderOptions;
use astral_tui_scrollback::TranscriptEntryId;
use ratatui::backend::Backend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::ConversationState;
use crate::ConversationSurface;
use crate::SurfaceNode;
use crate::SurfaceNodeId;
use crate::SurfaceNodeKind;
use crate::SurfaceRenderer;

const MAX_COMMIT_ROWS: u16 = 2_000;

/// Result of one terminal-native commit pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InlineCommitResult {
    pub committed_nodes: usize,
    pub tail_start: usize,
}

/// Inline conversation controller over one canonical rendered surface.
pub struct InlineHost {
    thread_id: String,
    surface: ConversationSurface,
    frontier: CommitFrontier,
    renderer: SurfaceRenderer,
}

impl InlineHost {
    pub fn new(conversation: &ConversationState, terminal_width: u16) -> Self {
        Self::from_surface(
            conversation.transcript().thread_id(),
            render_surface(conversation, terminal_width),
        )
    }

    /// Construct an inline host from a surface materialized by an external,
    /// authoritative transcript projector.
    pub fn from_surface(thread_id: impl Into<String>, surface: ConversationSurface) -> Self {
        Self {
            thread_id: thread_id.into(),
            surface,
            frontier: CommitFrontier::default(),
            renderer: SurfaceRenderer::default(),
        }
    }

    pub fn surface(&self) -> &ConversationSurface {
        &self.surface
    }

    /// Rebuild after transcript growth or terminal reflow. A new thread owns a
    /// fresh terminal transcript, so its print-once frontier starts empty.
    pub fn refresh_surface(&mut self, conversation: &ConversationState, terminal_width: u16) {
        self.refresh_materialized_surface(
            conversation.transcript().thread_id(),
            render_surface(conversation, terminal_width),
        );
    }

    /// Replace the externally materialized surface while retaining committed
    /// identities for the same thread.
    pub fn refresh_materialized_surface(
        &mut self,
        thread_id: impl Into<String>,
        surface: ConversationSurface,
    ) {
        let thread_id = thread_id.into();
        if self.thread_id != thread_id {
            self.thread_id = thread_id;
            self.frontier = CommitFrontier::default();
        }
        self.surface = surface;
    }

    /// Insert the leading run of stable nodes into native scrollback.
    ///
    /// A failed terminal write leaves that node and every later node
    /// uncommitted, so the next frame can retry without losing content.
    pub fn commit_to_terminal<B>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> io::Result<InlineCommitResult>
    where
        B: Backend,
    {
        if !terminal.is_inline() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "inline host requires an inline terminal viewport",
            ));
        }
        let terminal_width = terminal.viewport_area().width;
        if terminal_width == 0 {
            return Ok(InlineCommitResult {
                committed_nodes: 0,
                tail_start: self.frontier.tail_start(&self.surface),
            });
        }
        let expected_width = content_width(terminal_width);
        if self.surface.width() != expected_width {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "inline surface width is stale; refresh it before committing",
            ));
        }

        let renderer = self.renderer;
        self.commit_with(|surface, rows| {
            let height = commit_height(rows.len());
            terminal.insert_before(height, move |buffer| {
                render_committed(buffer, renderer, surface, rows);
            })
        })
    }

    /// Commit the leading run of stable nodes through a caller-owned terminal
    /// writer. A successful callback advances the print-once frontier; a
    /// failure leaves that node and every later node available for retry.
    pub fn commit_with<F>(&mut self, mut commit_node: F) -> io::Result<InlineCommitResult>
    where
        F: FnMut(&ConversationSurface, Range<usize>) -> io::Result<()>,
    {
        let ready = self.frontier.ready_nodes(&self.surface);
        let mut committed_nodes = 0usize;
        for id in ready {
            let (rows, mark) = {
                let Some(node) = self.surface.node(id) else {
                    return Err(io::Error::other(
                        "inline commit frontier referenced a missing surface node",
                    ));
                };
                (node.footprint_rows(), CommitMark::from_node(node))
            };
            commit_node(&self.surface, rows)?;
            self.frontier.mark(mark);
            committed_nodes += 1;
        }

        Ok(InlineCommitResult {
            committed_nodes,
            tail_start: self.frontier.tail_start(&self.surface),
        })
    }

    /// Height of the tail that will remain after the next successful commit
    /// pass, capped by the host's available rows.
    ///
    /// Inline callers size the pinned viewport before `insert_before` runs.
    /// Measuring the current uncommitted suffix would therefore include nodes
    /// about to leave the viewport and make the composer jump after every
    /// commit.
    pub fn live_tail_height(&self, available_rows: u16) -> u16 {
        let tail_rows = self
            .surface
            .row_count()
            .saturating_sub(self.frontier.projected_tail_start(&self.surface));
        usize::from(available_rows).min(tail_rows) as u16
    }

    /// First row that remains live after the next successful commit pass.
    pub fn projected_tail_start(&self) -> usize {
        self.frontier.projected_tail_start(&self.surface)
    }

    /// Render only the uncommitted tail with the same lines, gaps, and rails
    /// used for committed output. Tall tails are clipped from the top so the
    /// newest activity remains visible above the composer.
    pub fn render_live_tail(&self, area: Rect, buffer: &mut Buffer) -> Range<usize> {
        let visible = self.live_tail_rows(area.height);
        self.renderer
            .render_rows(area, buffer, &self.surface, visible.clone());
        visible
    }

    /// Shared row range painted by [`Self::render_live_tail`]. Hosts use this
    /// to project semantic hyperlink metadata onto the same clipped rows.
    pub fn live_tail_rows(&self, available_rows: u16) -> Range<usize> {
        let tail_start = self.frontier.tail_start(&self.surface);
        let visible_start = self
            .surface
            .row_count()
            .saturating_sub(usize::from(available_rows))
            .max(tail_start);
        visible_start..self.surface.row_count()
    }
}

#[derive(Debug, Default)]
struct CommitFrontier {
    committed_nodes: HashSet<SurfaceNodeId>,
    committed_entries: HashSet<TranscriptEntryId>,
}

impl CommitFrontier {
    fn ready_nodes(&self, surface: &ConversationSurface) -> Vec<SurfaceNodeId> {
        let mut ready = Vec::new();
        for node in surface.nodes() {
            if self.covers(node) {
                continue;
            }
            if !is_commit_ready(node) {
                break;
            }
            ready.push(node.id());
        }
        ready
    }

    fn tail_start(&self, surface: &ConversationSurface) -> usize {
        surface
            .nodes()
            .iter()
            .find(|node| !self.covers(node))
            .map_or(surface.row_count(), |node| node.rows().start)
    }

    fn projected_tail_start(&self, surface: &ConversationSurface) -> usize {
        surface
            .nodes()
            .iter()
            .find(|node| !self.covers(node) && !is_commit_ready(node))
            .map_or(surface.row_count(), |node| node.rows().start)
    }

    fn covers(&self, node: &SurfaceNode) -> bool {
        if self.committed_nodes.contains(&node.id()) {
            return true;
        }
        match node.kind() {
            SurfaceNodeKind::Entry { .. } => match node.id() {
                SurfaceNodeId::Entry(entry_id) => self.committed_entries.contains(&entry_id),
                SurfaceNodeId::VerbGroup(_) => false,
            },
            SurfaceNodeKind::VerbGroup { members, .. } => {
                !members.is_empty()
                    && members
                        .iter()
                        .all(|member| self.committed_entries.contains(member))
            }
        }
    }

    fn mark(&mut self, mark: CommitMark) {
        self.committed_nodes.insert(mark.id);
        match mark.source {
            CommitSource::Entry(entry_id) => {
                self.committed_entries.insert(entry_id);
            }
            CommitSource::CollapsedGroup(members) => {
                self.committed_entries.extend(members);
            }
            CommitSource::ExpandedGroup => {}
        }
    }
}

fn is_commit_ready(node: &SurfaceNode) -> bool {
    match node.kind() {
        SurfaceNodeKind::Entry {
            lifecycle,
            turn_settled,
            presentation_stable,
            ..
        } => {
            *presentation_stable
                && (*turn_settled || !matches!(lifecycle, EntryLifecycle::Running { .. }))
        }
        SurfaceNodeKind::VerbGroup {
            running,
            turn_settled,
            presentation_stable,
            ..
        } => *presentation_stable && (*turn_settled || !running),
    }
}

struct CommitMark {
    id: SurfaceNodeId,
    source: CommitSource,
}

impl CommitMark {
    fn from_node(node: &SurfaceNode) -> Self {
        let source = match (node.id(), node.kind()) {
            (SurfaceNodeId::Entry(entry_id), SurfaceNodeKind::Entry { .. }) => {
                CommitSource::Entry(entry_id)
            }
            (
                SurfaceNodeId::VerbGroup(_),
                SurfaceNodeKind::VerbGroup {
                    mode: DisplayMode::Collapsed,
                    members,
                    ..
                },
            ) => CommitSource::CollapsedGroup(members.clone()),
            (SurfaceNodeId::VerbGroup(_), SurfaceNodeKind::VerbGroup { .. }) => {
                CommitSource::ExpandedGroup
            }
            _ => unreachable!("surface node identity must match its kind"),
        };
        Self {
            id: node.id(),
            source,
        }
    }
}

enum CommitSource {
    Entry(TranscriptEntryId),
    CollapsedGroup(Vec<TranscriptEntryId>),
    ExpandedGroup,
}

fn render_surface(conversation: &ConversationState, terminal_width: u16) -> ConversationSurface {
    ConversationSurface::render(
        conversation,
        EntryRenderOptions::new(content_width(terminal_width)),
    )
}

fn content_width(terminal_width: u16) -> u16 {
    SurfaceRenderer::content_width(Rect::new(0, 0, terminal_width, 1))
}

fn commit_height(full_height: usize) -> u16 {
    full_height.min(usize::from(MAX_COMMIT_ROWS)) as u16
}

fn render_committed(
    buffer: &mut Buffer,
    renderer: SurfaceRenderer,
    surface: &ConversationSurface,
    rows: std::ops::Range<usize>,
) {
    let full_height = rows.len();
    let commit_height = usize::from(buffer.area.height);
    if full_height <= commit_height {
        renderer.render_rows(buffer.area, buffer, surface, rows);
        return;
    }

    let content_height = commit_height.saturating_sub(1);
    renderer.render_rows(
        buffer.area,
        buffer,
        surface,
        rows.start..rows.start.saturating_add(content_height),
    );
    let hidden = full_height.saturating_sub(content_height);
    let footer = Rect::new(
        buffer.area.x,
        buffer.area.bottom().saturating_sub(1),
        buffer.area.width,
        1,
    );
    Paragraph::new(Line::from(format!(
        "… {hidden} more lines — /transcript to view"
    )))
    .dim()
    .render(footer, buffer);
}

#[cfg(test)]
#[path = "inline_tests.rs"]
mod tests;
