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
use astral_tui_scrollback::MarkdownLine;
use astral_tui_scrollback::RenderedEntry;
use astral_tui_scrollback::TranscriptEntryId;
use astral_tui_scrollback::render_entry;
use astral_tui_scrollback::render_verb_group_header;

use crate::ConversationState;

/// Stable identity shared by viewport anchors, keyboard selection, and mouse
/// hit testing. Synthetic group headers have their own namespace, so they do
/// not collide with the source entry that anchors the group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceNodeId {
    Entry(TranscriptEntryId),
    VerbGroup(TranscriptEntryId),
}

/// Source metadata for one rendered surface node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceNodeKind {
    Entry {
        lifecycle: EntryLifecycle,
    },
    VerbGroup {
        mode: DisplayMode,
        members: Vec<TranscriptEntryId>,
    },
}

/// One ordered, width-resolved conversation node.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceNode {
    id: SurfaceNodeId,
    turn_id: String,
    kind: SurfaceNodeKind,
    rows: Range<usize>,
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
}

impl ConversationSurface {
    /// Render source entries in authoritative transcript order, inserting one
    /// synthetic header for each Grok-style verb group. A collapsed group
    /// replaces only its claimed members; transparent entries inside the span
    /// remain visible at their original source position.
    pub fn render(conversation: &ConversationState, options: EntryRenderOptions) -> Self {
        let mut surface = Self {
            width: options.width,
            row_count: 0,
            nodes: Vec::new(),
        };

        for turn in conversation.transcript().turns() {
            let groups = conversation.verb_groups(turn.id());
            let mut group_index = 0usize;

            for (entry_index, entry) in turn.entries().iter().enumerate() {
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
                        SurfaceNodeKind::VerbGroup { mode, members },
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
                    },
                    rendered,
                );
            }
        }

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
        self.nodes.iter().flat_map(|node| node.rendered.lines())
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

    fn push(
        &mut self,
        id: SurfaceNodeId,
        turn_id: &str,
        kind: SurfaceNodeKind,
        rendered: RenderedEntry,
    ) {
        let start = self.row_count;
        self.row_count = self.row_count.saturating_add(rendered.lines().len());
        self.nodes.push(SurfaceNode {
            id,
            turn_id: turn_id.to_string(),
            kind,
            rows: start..self.row_count,
            rendered,
        });
    }
}

#[cfg(test)]
#[path = "surface_tests.rs"]
mod tests;
