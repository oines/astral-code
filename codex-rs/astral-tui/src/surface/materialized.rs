//! Width-resolved entries supplied by an authoritative transcript projector.

use astral_tui_scrollback::MarkdownLine;
use astral_tui_scrollback::RenderedEntry;
use astral_tui_scrollback::TranscriptEntryId;

use super::SurfaceEntryPresentation;

/// Spacing before an entry supplied by an external transcript projector.
///
/// This is deliberately separate from `groupable`: suppressing a blank row
/// does not imply Grok-style grouping or folding semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceEntrySpacing {
    Separate,
    Continue,
}

/// One stable, ordered entry already rendered by an authoritative transcript
/// projector.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterializedSurfaceEntry {
    pub(super) id: TranscriptEntryId,
    pub(super) presentation_group: Option<String>,
    pub(super) spacing: SurfaceEntrySpacing,
    pub(super) presentation: SurfaceEntryPresentation,
    pub(super) rendered: RenderedEntry,
}

impl MaterializedSurfaceEntry {
    pub fn new(
        id: TranscriptEntryId,
        presentation_group: impl Into<String>,
        spacing: SurfaceEntrySpacing,
        presentation: SurfaceEntryPresentation,
        lines: Vec<MarkdownLine>,
    ) -> Self {
        Self {
            id,
            presentation_group: Some(presentation_group.into()),
            spacing,
            presentation,
            rendered: RenderedEntry::from_lines(lines),
        }
    }

    /// Construct an entry that has no source-backed grouping identity.
    pub fn ungrouped(
        id: TranscriptEntryId,
        spacing: SurfaceEntrySpacing,
        presentation: SurfaceEntryPresentation,
        lines: Vec<MarkdownLine>,
    ) -> Self {
        Self {
            id,
            presentation_group: None,
            spacing,
            presentation,
            rendered: RenderedEntry::from_lines(lines),
        }
    }
}
