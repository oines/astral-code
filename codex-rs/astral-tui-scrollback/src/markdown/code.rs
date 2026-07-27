//! Width-aware presentation for fenced and indented code lines.

use ratatui::text::Line;
use ratatui::text::Span;

use super::MarkdownStyle;
use super::Segment;
use super::wrapping::padded_background_line;
use super::wrapping::wrap_segments;

pub(super) fn render_code_line(
    width: u16,
    style: MarkdownStyle,
    source: &str,
    highlighted: Option<&[Segment]>,
    initial_prefix: Vec<Span<'static>>,
    subsequent_prefix: Vec<Span<'static>>,
) -> Vec<Line<'static>> {
    let prefix_width = Line::from(initial_prefix.clone()).width();
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
    let leading = source
        .chars()
        .take_while(|character| *character == ' ')
        .count();
    let indent = " ".repeat(leading.min(content_width.saturating_sub(1)));
    let body = source.trim_start_matches(' ');
    let body_segments = if let Some(highlighted) = highlighted {
        trim_segment_prefix(highlighted, leading)
    } else {
        vec![Segment {
            text: body.to_string(),
            style: style.code,
        }]
    };
    let wrapped = if body.is_empty() {
        vec![Vec::new()]
    } else {
        wrap_segments(
            &body_segments,
            content_width.saturating_sub(indent.len()).max(1),
        )
    };
    wrapped
        .into_iter()
        .enumerate()
        .map(|(index, mut content)| {
            let mut spans = if index == 0 {
                initial_prefix.clone()
            } else {
                subsequent_prefix.clone()
            };
            spans.push(Span::styled(indent.clone(), style.code));
            for span in &mut content {
                span.style = style.code.patch(span.style);
            }
            spans.append(&mut content);
            padded_background_line(spans, width, style.code_background)
        })
        .collect()
}

fn trim_segment_prefix(segments: &[Segment], prefix_bytes: usize) -> Vec<Segment> {
    let mut remaining = prefix_bytes;
    segments
        .iter()
        .filter_map(|segment| {
            if remaining >= segment.text.len() {
                remaining -= segment.text.len();
                return None;
            }
            let text = segment.text[remaining..].to_string();
            remaining = 0;
            Some(Segment {
                text,
                style: segment.style,
            })
        })
        .collect()
}
