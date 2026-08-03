//! Source-preserving Markdown rendering primitives for Astral transcript entries.
//!
//! The complete parser and entry renderer are layered on top of this module.
//! Keeping width-aware wrapping and syntax highlighting here gives inline and
//! fullscreen views one implementation without coupling either view to event
//! projection.

use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use std::ops::Range;

#[path = "markdown/style.rs"]
mod style;
#[path = "markdown/syntax.rs"]
mod syntax;
#[path = "markdown/table.rs"]
mod table;
#[path = "markdown/wrapping.rs"]
mod wrapping;

pub use style::MarkdownStyle;
pub use style::MarkdownSyntaxTheme;
pub use syntax::CodeLineHighlighter;
pub use table::MarkdownTable;
pub use table::MarkdownTableAlignment;
use wrapping::wrap_segments_with_joiners;

/// Separator required before a rendered line when reconstructing selected text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineJoiner {
    HardBreak,
    Space,
    None,
}

impl LineJoiner {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HardBreak => "\n",
            Self::Space => " ",
            Self::None => "",
        }
    }
}

/// One rendered Markdown line and its relationship to the preceding line.
#[derive(Debug, Clone)]
pub struct MarkdownLine {
    pub line: Line<'static>,
    pub joiner_to_previous: LineJoiner,
    pub links: Vec<MarkdownLink>,
}

/// One Markdown hyperlink segment on a rendered line.
///
/// A logical link can produce one segment per visual line after wrapping. Its
/// stable `id` lets selection and opening treat those segments as one link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownLink {
    pub id: u32,
    pub columns: Range<u16>,
    pub target: String,
}

/// Highlights a fenced code block without parsing the surrounding Markdown.
pub fn highlight_fenced_code(
    source: &str,
    fence_info: &str,
    theme: MarkdownSyntaxTheme,
) -> Option<Vec<Vec<Span<'static>>>> {
    syntax::highlight_code(source, fence_info, theme).map(|lines| {
        lines
            .into_iter()
            .map(|line| {
                line.into_iter()
                    .map(|segment| Span::styled(segment.text, segment.style))
                    .collect()
            })
            .collect()
    })
}

/// Wraps literal Markdown source while retaining selection joiners.
pub fn render_literal_with_metadata(text: &str, width: u16, style: Style) -> Vec<MarkdownLine> {
    wrap_segments_with_joiners(
        &[Segment {
            text: text.to_string(),
            style,
            link: None,
        }],
        usize::from(width).max(1),
    )
    .into_iter()
    .map(|wrapped| MarkdownLine {
        line: Line::from(wrapped.spans),
        joiner_to_previous: wrapped.joiner_to_previous,
        links: wrapped.links,
    })
    .collect()
}

/// Wraps one styled logical line without discarding span styles.
pub fn wrap_styled_line_with_metadata(line: &Line<'_>, width: u16) -> Vec<MarkdownLine> {
    let segments = line
        .spans
        .iter()
        .map(|span| Segment {
            text: span.content.to_string(),
            style: line.style.patch(span.style),
            link: None,
        })
        .collect::<Vec<_>>();
    wrap_segments_with_joiners(&segments, usize::from(width).max(1))
        .into_iter()
        .map(|wrapped| MarkdownLine {
            line: Line::from(wrapped.spans),
            joiner_to_previous: wrapped.joiner_to_previous,
            links: wrapped.links,
        })
        .collect()
}

#[derive(Debug, Clone)]
struct Segment {
    text: String,
    style: Style,
    link: Option<SegmentLink>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SegmentLink {
    id: u32,
    target: String,
}

#[cfg(test)]
#[path = "markdown_tests.rs"]
mod tests;
