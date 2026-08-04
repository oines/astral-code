//! Shared, viewport-independent conversation surface.
//!
//! The surface materializes the canonical transcript once at a requested
//! width. Inline and fullscreen hosts consume the same ordered nodes and row
//! geometry; scrolling, selection, hover, and terminal-native commit policy
//! deliberately remain outside this module.

use std::ops::Range;

use astral_tui_scrollback::DisplayMode;
use astral_tui_scrollback::EntryBlock;
use astral_tui_scrollback::EntryLifecycle;
use astral_tui_scrollback::EntryRenderOptions;
use astral_tui_scrollback::LineJoiner;
use astral_tui_scrollback::MarkdownLine;
use astral_tui_scrollback::RenderedEntry;
use astral_tui_scrollback::TranscriptEntryId;
use astral_tui_scrollback::render_entry;
use astral_tui_scrollback::render_verb_group_header;
use codex_app_server_protocol::TurnStatus;

use crate::ConversationState;

/// Stable identity shared by viewport anchors, keyboard selection, and mouse
/// hit testing. Synthetic group headers have their own namespace, so they do
/// not collide with the source entry that anchors the group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceNodeId {
    Entry(TranscriptEntryId),
    VerbGroup(TranscriptEntryId),
}

/// Reflow-stable position inside one surface node.
///
/// A terminal resize changes soft-wrapped row counts, so retaining an absolute
/// row would jump to unrelated text. Logical line indices survive reflow;
/// `sub_rows` preserves the position inside that line and is clamped if the
/// line becomes shorter at the new width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceAnchor {
    node: SurfaceNodeId,
    logical_line: usize,
    sub_rows: usize,
}

impl SurfaceAnchor {
    pub fn node(self) -> SurfaceNodeId {
        self.node
    }

    pub fn logical_line(self) -> usize {
        self.logical_line
    }

    pub fn sub_rows(self) -> usize {
        self.sub_rows
    }
}

/// Source metadata for one rendered surface node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceNodeKind {
    Entry {
        lifecycle: EntryLifecycle,
        mode: DisplayMode,
        foldable: bool,
        groupable: bool,
        turn_settled: bool,
        presentation_stable: bool,
    },
    VerbGroup {
        mode: DisplayMode,
        members: Vec<TranscriptEntryId>,
        running: bool,
        turn_settled: bool,
        presentation_stable: bool,
    },
}

/// Presentation metadata supplied by an authoritative transcript source.
///
/// The shared surface owns geometry and interaction hit testing, not event
/// projection. Sources such as the app-server reducer or Codex `HistoryCell`
/// transcript decide these semantics before materialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceEntryPresentation {
    pub lifecycle: EntryLifecycle,
    pub mode: DisplayMode,
    pub foldable: bool,
    pub groupable: bool,
    pub turn_settled: bool,
    pub presentation_stable: bool,
}

/// One stable, ordered entry already rendered by an authoritative transcript
/// projector.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterializedSurfaceEntry {
    id: TranscriptEntryId,
    turn_id: String,
    presentation: SurfaceEntryPresentation,
    rendered: RenderedEntry,
}

impl MaterializedSurfaceEntry {
    pub fn new(
        id: TranscriptEntryId,
        turn_id: impl Into<String>,
        presentation: SurfaceEntryPresentation,
        lines: Vec<MarkdownLine>,
    ) -> Self {
        Self {
            id,
            turn_id: turn_id.into(),
            presentation,
            rendered: RenderedEntry::from_lines(lines),
        }
    }
}

/// One ordered, width-resolved conversation node.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceNode {
    id: SurfaceNodeId,
    turn_id: String,
    kind: SurfaceNodeKind,
    rows: Range<usize>,
    gap_after: usize,
    rendered: RenderedEntry,
}

impl SurfaceNode {
    pub fn id(&self) -> SurfaceNodeId {
        self.id
    }

    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }

    pub fn kind(&self) -> &SurfaceNodeKind {
        &self.kind
    }

    pub fn rows(&self) -> Range<usize> {
        self.rows.clone()
    }

    /// Number of shared layout rows between this node and the next one.
    pub fn gap_after(&self) -> usize {
        self.gap_after
    }

    /// Complete print-once footprint, including the shared spacer after this
    /// node. Inline commit and live rendering must use this exact range.
    pub fn footprint_rows(&self) -> Range<usize> {
        self.rows.start..self.rows.end.saturating_add(self.gap_after)
    }

    pub fn display_mode(&self) -> DisplayMode {
        match &self.kind {
            SurfaceNodeKind::Entry { mode, .. } | SurfaceNodeKind::VerbGroup { mode, .. } => *mode,
        }
    }

    pub fn is_foldable(&self) -> bool {
        match &self.kind {
            SurfaceNodeKind::Entry { foldable, .. } => *foldable,
            SurfaceNodeKind::VerbGroup { .. } => true,
        }
    }

    pub fn is_groupable(&self) -> bool {
        match &self.kind {
            SurfaceNodeKind::Entry { groupable, .. } => *groupable,
            SurfaceNodeKind::VerbGroup { .. } => true,
        }
    }

    /// Whether appending another item can no longer rewrite this node's
    /// view-time grouping.
    pub fn is_presentation_stable(&self) -> bool {
        match &self.kind {
            SurfaceNodeKind::Entry {
                presentation_stable,
                ..
            }
            | SurfaceNodeKind::VerbGroup {
                presentation_stable,
                ..
            } => *presentation_stable,
        }
    }

    pub fn rendered(&self) -> &RenderedEntry {
        &self.rendered
    }
}

/// Canonical rendered entry tree for one conversation at one width.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationSurface {
    width: u16,
    row_count: usize,
    nodes: Vec<SurfaceNode>,
    gap_line: MarkdownLine,
}

impl ConversationSurface {
    /// Render source entries in authoritative transcript order, inserting one
    /// synthetic header for each Grok-style verb group. A collapsed group
    /// replaces only its claimed members; transparent entries inside the span
    /// remain visible at their original source position.
    pub fn render(conversation: &ConversationState, options: EntryRenderOptions) -> Self {
        let mut surface = Self::empty(options.width);

        for turn in conversation.transcript().turns() {
            let groups = conversation.verb_groups(turn.id());
            let turn_settled = turn.status() != &TurnStatus::InProgress;
            let unstable_suffix_start = conversation.unstable_group_suffix_start(turn.id());
            let mut group_index = 0usize;

            for (entry_index, entry) in turn.entries().iter().enumerate() {
                let presentation_stable =
                    turn_settled || unstable_suffix_start.is_none_or(|start| entry_index < start);
                while groups
                    .get(group_index)
                    .is_some_and(|group| group.range().end <= entry_index)
                {
                    group_index += 1;
                }

                let active_group = groups
                    .get(group_index)
                    .filter(|group| group.range().contains(&entry_index));
                if let Some(group) = active_group.filter(|group| group.range().start == entry_index)
                {
                    let mode = conversation
                        .verb_group_mode(turn.id(), group)
                        .unwrap_or(DisplayMode::Collapsed);
                    let members = group
                        .claimed()
                        .iter()
                        .filter_map(|index| turn.entries().get(*index))
                        .map(astral_tui_scrollback::TranscriptEntry::id)
                        .collect();
                    surface.push(
                        SurfaceNodeId::VerbGroup(group.anchor()),
                        turn.id(),
                        SurfaceNodeKind::VerbGroup {
                            mode,
                            members,
                            running: group.running(),
                            turn_settled,
                            presentation_stable,
                        },
                        render_verb_group_header(group, options),
                    );
                }

                if active_group.is_some_and(|group| {
                    conversation.verb_group_mode(turn.id(), group) == Some(DisplayMode::Collapsed)
                        && group.contains_member(entry_index)
                }) {
                    continue;
                }

                let Some(display) = conversation.entry_display_state(entry.id()) else {
                    continue;
                };
                let block = EntryBlock::from_entry(entry);
                let Some(rendered) = render_entry(&block, display, options) else {
                    continue;
                };
                surface.push(
                    SurfaceNodeId::Entry(entry.id()),
                    turn.id(),
                    SurfaceNodeKind::Entry {
                        lifecycle: entry.lifecycle(),
                        mode: display.mode(),
                        foldable: block.is_foldable(),
                        groupable: block.is_groupable(),
                        turn_settled,
                        presentation_stable,
                    },
                    rendered,
                );
            }
        }

        surface.finish();
        surface
    }

    /// Build the shared surface from entries materialized by another
    /// authoritative transcript projector.
    ///
    /// Input order is preserved exactly. No event reduction, item merging, or
    /// semantic reordering occurs at this boundary.
    pub fn from_materialized(
        width: u16,
        entries: impl IntoIterator<Item = MaterializedSurfaceEntry>,
    ) -> Self {
        let mut surface = Self::empty(width);
        for entry in entries {
            let presentation = entry.presentation;
            surface.push(
                SurfaceNodeId::Entry(entry.id),
                &entry.turn_id,
                SurfaceNodeKind::Entry {
                    lifecycle: presentation.lifecycle,
                    mode: presentation.mode,
                    foldable: presentation.foldable,
                    groupable: presentation.groupable,
                    turn_settled: presentation.turn_settled,
                    presentation_stable: presentation.presentation_stable,
                },
                entry.rendered,
            );
        }
        surface.finish();
        surface
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn row_count(&self) -> usize {
        self.row_count
    }

    pub fn nodes(&self) -> &[SurfaceNode] {
        &self.nodes
    }

    pub fn lines(&self) -> impl Iterator<Item = &MarkdownLine> {
        let gap_line = &self.gap_line;
        self.nodes.iter().flat_map(move |node| {
            node.rendered
                .lines()
                .iter()
                .chain(std::iter::repeat_n(gap_line, node.gap_after))
        })
    }

    /// Resolve one shared layout row without walking all preceding entries.
    pub fn line_at_row(&self, row: usize) -> Option<&MarkdownLine> {
        if let Some(node) = self.node_at_row(row) {
            return node
                .rendered
                .lines()
                .get(row.saturating_sub(node.rows.start));
        }
        (row < self.row_count).then_some(&self.gap_line)
    }

    pub fn node(&self, id: SurfaceNodeId) -> Option<&SurfaceNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// Resolve one virtual transcript row without coupling the surface to a
    /// particular terminal viewport.
    pub fn node_at_row(&self, row: usize) -> Option<&SurfaceNode> {
        let index = self.nodes.partition_point(|node| node.rows.end <= row);
        self.nodes
            .get(index)
            .filter(|node| node.rows.contains(&row))
    }

    /// Capture a virtual row as a semantic node + logical-line anchor.
    pub fn anchor_at_row(&self, row: usize) -> Option<SurfaceAnchor> {
        let node = self
            .node_at_row(row)
            .or_else(|| self.node_before_gap_row(row))?;
        let local_row = row.saturating_sub(node.rows.start);
        let (logical_line, line_start) = node
            .rendered
            .lines()
            .iter()
            .take(local_row.saturating_add(1))
            .enumerate()
            .filter(|(index, line)| *index == 0 || line.joiner_to_previous == LineJoiner::HardBreak)
            .enumerate()
            .last()
            .map(|(logical_line, (line_start, _))| (logical_line, line_start))?;
        Some(SurfaceAnchor {
            node: node.id,
            logical_line,
            sub_rows: local_row.saturating_sub(line_start),
        })
    }

    /// Resolve an anchor after content growth or width-dependent reflow.
    pub fn row_for_anchor(&self, anchor: SurfaceAnchor) -> Option<usize> {
        let node = self.node(anchor.node)?;
        let mut logical_starts =
            node.rendered
                .lines()
                .iter()
                .enumerate()
                .filter(|(index, line)| {
                    *index == 0 || line.joiner_to_previous == LineJoiner::HardBreak
                });
        let line_start = logical_starts
            .nth(anchor.logical_line)
            .map(|(index, _)| index)
            .or_else(|| node.rendered.lines().len().checked_sub(1))?;
        let line_last = logical_starts
            .next()
            .map_or_else(
                || node.rendered.lines().len().saturating_sub(1),
                |(next_start, _)| next_start.saturating_sub(1),
            )
            .max(line_start);
        let local_row = line_start.saturating_add(anchor.sub_rows).min(line_last);
        Some(node.rows.start.saturating_add(local_row))
    }

    fn push(
        &mut self,
        id: SurfaceNodeId,
        turn_id: &str,
        kind: SurfaceNodeKind,
        rendered: RenderedEntry,
    ) {
        if rendered.lines().is_empty() {
            return;
        }
        if let Some(previous) = self.nodes.last_mut() {
            let both_groupable = previous.is_groupable()
                && match &kind {
                    SurfaceNodeKind::Entry { groupable, .. } => *groupable,
                    SurfaceNodeKind::VerbGroup { .. } => true,
                };
            let both_collapsed = previous.display_mode() == DisplayMode::Collapsed
                && match &kind {
                    SurfaceNodeKind::Entry { mode, .. }
                    | SurfaceNodeKind::VerbGroup { mode, .. } => *mode == DisplayMode::Collapsed,
                };
            previous.gap_after = usize::from(!(both_groupable && both_collapsed));
            self.row_count = self.row_count.saturating_add(previous.gap_after);
        }
        let start = self.row_count;
        self.row_count = self.row_count.saturating_add(rendered.lines().len());
        self.nodes.push(SurfaceNode {
            id,
            turn_id: turn_id.to_string(),
            kind,
            rows: start..self.row_count,
            gap_after: 0,
            rendered,
        });
    }

    fn empty(width: u16) -> Self {
        Self {
            width: width.max(1),
            row_count: 0,
            nodes: Vec::new(),
            gap_line: MarkdownLine {
                line: Default::default(),
                joiner_to_previous: LineJoiner::HardBreak,
                links: Vec::new(),
            },
        }
    }

    fn finish(&mut self) {
        if let Some(last) = self.nodes.last_mut() {
            last.gap_after = 1;
            self.row_count = self.row_count.saturating_add(last.gap_after);
        }
    }

    fn node_before_gap_row(&self, row: usize) -> Option<&SurfaceNode> {
        let insertion = self.nodes.partition_point(|node| node.rows.end <= row);
        insertion.checked_sub(1).and_then(|index| {
            let node = &self.nodes[index];
            (row < node.rows.end.saturating_add(node.gap_after)).then_some(node)
        })
    }
}

#[cfg(test)]
#[path = "surface_tests.rs"]
mod tests;
