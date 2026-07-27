//! Styled wrapping adapted from Grok Build's Markdown output pipeline.

use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use textwrap::WordSeparator;
use textwrap::WordSplitter;

use super::LineJoiner;
use super::Segment;

pub(super) struct WrappedSegments {
    pub(super) spans: Vec<Span<'static>>,
    pub(super) joiner_to_previous: LineJoiner,
}

pub(super) fn wrap_segments(segments: &[Segment], width: usize) -> Vec<Vec<Span<'static>>> {
    wrap_segments_with_joiners(segments, width)
        .into_iter()
        .map(|wrapped| wrapped.spans)
        .collect()
}

pub(super) fn wrap_segments_with_joiners(
    segments: &[Segment],
    width: usize,
) -> Vec<WrappedSegments> {
    let logical_lines = split_logical_lines(segments);
    let mut output = Vec::new();
    for logical in logical_lines {
        let plain = logical
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<String>();
        if plain.is_empty() {
            output.push(WrappedSegments {
                spans: Vec::new(),
                joiner_to_previous: LineJoiner::HardBreak,
            });
            continue;
        }
        let options = textwrap::Options::new(width.max(1))
            .word_separator(WordSeparator::AsciiSpace)
            .word_splitter(WordSplitter::NoHyphenation)
            .break_words(true);
        let mut cursor = 0;
        for wrapped in textwrap::wrap(&plain, &options) {
            let wrapped = wrapped.as_ref();
            let start = plain[cursor..]
                .find(wrapped)
                .map_or(cursor, |relative| cursor + relative);
            let end = start + wrapped.len();
            let joiner_to_previous = if cursor == 0 {
                LineJoiner::HardBreak
            } else if plain[cursor..start].chars().any(char::is_whitespace) {
                LineJoiner::Space
            } else {
                LineJoiner::None
            };
            output.push(WrappedSegments {
                spans: styled_range(&logical, start, end),
                joiner_to_previous,
            });
            cursor = end;
        }
    }
    output
}

fn split_logical_lines(segments: &[Segment]) -> Vec<Vec<Segment>> {
    let mut lines = vec![Vec::new()];
    for segment in segments {
        for (index, part) in segment.text.split('\n').enumerate() {
            if index > 0 {
                lines.push(Vec::new());
            }
            if !part.is_empty()
                && let Some(line) = lines.last_mut()
            {
                line.push(Segment {
                    text: part.to_string(),
                    style: segment.style,
                });
            }
        }
    }
    lines
}

fn styled_range(segments: &[Segment], start: usize, end: usize) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut offset = 0;
    for segment in segments {
        let segment_end = offset + segment.text.len();
        let overlap_start = start.max(offset);
        let overlap_end = end.min(segment_end);
        if overlap_start < overlap_end {
            spans.push(Span::styled(
                segment.text[overlap_start - offset..overlap_end - offset].to_string(),
                segment.style,
            ));
        }
        offset = segment_end;
    }
    spans
}

pub(super) fn padded_background_line(
    mut spans: Vec<Span<'static>>,
    width: u16,
    background: Style,
) -> Line<'static> {
    let line = Line::from(spans.clone());
    let padding = usize::from(width).saturating_sub(line.width());
    spans.push(Span::styled(" ".repeat(padding), background));
    Line::from(spans).style(background)
}
