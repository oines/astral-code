//! Structured diff rows derived from Grok Build's edit renderer at commit
//! `47348d13ec4508dcfe440e34c6d511bb02998fb2` (Apache-2.0).

use std::borrow::Cow;
use std::path::Path;

use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::PatchChangeKind;
use diffy::Line as DiffyLine;
use diffy::Patch;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::CodeLineHighlighter;
use crate::DiffStyle;
use crate::LineJoiner;
use crate::MarkdownLine;
use crate::render_literal_with_metadata;
use crate::wrap_styled_line_with_metadata;

use crate::render::prefix_lines;

const CONTENT_GAP: &str = "  ";
const TAB_WIDTH: usize = 4;

pub(super) fn render_file_change(
    change: &FileUpdateChange,
    width: u16,
    style: DiffStyle,
    indent: &'static str,
) -> Vec<MarkdownLine> {
    let path = match &change.kind {
        PatchChangeKind::Update {
            move_path: Some(destination),
        } => destination.as_path(),
        PatchChangeKind::Add
        | PatchChangeKind::Delete
        | PatchChangeKind::Update { move_path: None } => Path::new(&change.path),
    };
    match &change.kind {
        PatchChangeKind::Add => render_whole_file(
            &change.diff,
            DiffLineKind::Added,
            path,
            width,
            style,
            indent,
        ),
        PatchChangeKind::Delete => render_whole_file(
            &change.diff,
            DiffLineKind::Removed,
            path,
            width,
            style,
            indent,
        ),
        PatchChangeKind::Update { move_path } => render_update(
            update_diff(&change.diff, move_path.is_some()),
            path,
            width,
            style,
            indent,
        ),
    }
}

pub(super) fn change_counts(changes: &[FileUpdateChange]) -> (usize, usize) {
    changes.iter().fold((0, 0), |(added, removed), change| {
        let (change_added, change_removed) = match &change.kind {
            PatchChangeKind::Add => (change.diff.lines().count(), 0),
            PatchChangeKind::Delete => (0, change.diff.lines().count()),
            PatchChangeKind::Update { move_path } => {
                let source = update_diff(&change.diff, move_path.is_some());
                Patch::from_str(source).map_or_else(
                    |_| fallback_change_counts(source),
                    |patch| {
                        patch.hunks().iter().flat_map(diffy::Hunk::lines).fold(
                            (0, 0),
                            |(added, removed), line| match line {
                                DiffyLine::Insert(_) => (added + 1, removed),
                                DiffyLine::Delete(_) => (added, removed + 1),
                                DiffyLine::Context(_) => (added, removed),
                            },
                        )
                    },
                )
            }
        };
        (added + change_added, removed + change_removed)
    })
}

fn render_whole_file(
    source: &str,
    kind: DiffLineKind,
    path: &Path,
    width: u16,
    style: DiffStyle,
    indent: &'static str,
) -> Vec<MarkdownLine> {
    let number_width = line_number_width(source.lines().count());
    let layout = DiffLayout {
        number_width,
        width,
        style,
        indent,
    };
    let mut highlighter = CodeLineHighlighter::for_path(path, source, style.syntax_theme);
    source
        .lines()
        .enumerate()
        .flat_map(|(index, raw)| {
            let text = expand_tabs(raw);
            let highlighted = highlighter
                .as_mut()
                .and_then(|highlighter| highlighter.highlight_line(&text));
            render_diff_line(
                DiffRow {
                    line_number: index + 1,
                    kind,
                    text: &text,
                    highlighted,
                },
                layout,
            )
        })
        .collect()
}

fn render_update(
    source: &str,
    path: &Path,
    width: u16,
    style: DiffStyle,
    indent: &'static str,
) -> Vec<MarkdownLine> {
    let normalized;
    let patch = match Patch::from_str(source) {
        Ok(patch) => patch,
        Err(_) => {
            normalized = format!(
                "--- a/source\n+++ b/source\n{}\n",
                source.trim_end_matches(['\n', '\r'])
            );
            let Ok(patch) = Patch::from_str(&normalized) else {
                return render_unparsed(source, width, indent);
            };
            patch
        }
    };
    let number_width = patch
        .hunks()
        .iter()
        .flat_map(|hunk| {
            [
                hunk.old_range().end().saturating_sub(1),
                hunk.new_range().end().saturating_sub(1),
            ]
        })
        .max()
        .map_or(1, line_number_width);
    let layout = DiffLayout {
        number_width,
        width,
        style,
        indent,
    };
    let mut lines = Vec::new();
    let mut previous_new_end = None;

    for hunk in patch.hunks() {
        if let Some(previous_new_end) = previous_new_end {
            lines.push(hunk_separator(
                hunk.new_range().start().saturating_sub(previous_new_end),
                indent,
                style,
            ));
        }
        let mut old_highlighter = CodeLineHighlighter::for_path(path, source, style.syntax_theme);
        let mut new_highlighter = CodeLineHighlighter::for_path(path, source, style.syntax_theme);
        let mut old_line = hunk.old_range().start();
        let mut new_line = hunk.new_range().start();

        for line in hunk.lines() {
            let (line_number, kind, raw) = match line {
                DiffyLine::Insert(text) => (new_line, DiffLineKind::Added, text),
                DiffyLine::Delete(text) => (old_line, DiffLineKind::Removed, text),
                DiffyLine::Context(text) => (new_line, DiffLineKind::Context, text),
            };
            let raw = raw.trim_end_matches(['\n', '\r']);
            let text = expand_tabs(raw);
            let highlighted = match kind {
                DiffLineKind::Added => new_highlighter
                    .as_mut()
                    .and_then(|highlighter| highlighter.highlight_line(&text)),
                DiffLineKind::Removed => old_highlighter
                    .as_mut()
                    .and_then(|highlighter| highlighter.highlight_line(&text)),
                DiffLineKind::Context => {
                    if let Some(highlighter) = old_highlighter.as_mut() {
                        let _ = highlighter.highlight_line(&text);
                    }
                    new_highlighter
                        .as_mut()
                        .and_then(|highlighter| highlighter.highlight_line(&text))
                }
            };
            lines.extend(render_diff_line(
                DiffRow {
                    line_number,
                    kind,
                    text: &text,
                    highlighted,
                },
                layout,
            ));
            match kind {
                DiffLineKind::Added => new_line += 1,
                DiffLineKind::Removed => old_line += 1,
                DiffLineKind::Context => {
                    old_line += 1;
                    new_line += 1;
                }
            }
        }
        previous_new_end = Some(hunk.new_range().end());
    }
    lines
}

fn render_diff_line(row: DiffRow<'_>, layout: DiffLayout) -> Vec<MarkdownLine> {
    let prefix_width =
        Line::from(layout.indent).width() + layout.number_width + Line::from(CONTENT_GAP).width();
    let content_width = usize::from(layout.width)
        .saturating_sub(prefix_width)
        .max(1);
    let background = background(row.kind, layout.style);
    let mut spans = content_spans(row.text, row.kind, row.highlighted, layout.style);
    if spans.is_empty() {
        spans.push(Span::raw(" "));
    }
    if let Some(background) = background {
        for span in &mut spans {
            span.style = span.style.bg(background);
        }
    }
    let content_width_u16 = u16::try_from(content_width).unwrap_or(u16::MAX);
    let mut lines = wrap_styled_line_with_metadata(&Line::from(spans), content_width_u16);
    for (index, line) in lines.iter_mut().enumerate() {
        let padding = content_width.saturating_sub(line.line.width());
        if padding > 0 {
            let style = background.map_or_else(Style::default, |color| Style::default().bg(color));
            line.line
                .push_span(Span::styled(" ".repeat(padding), style));
        }
        let number = if index == 0 {
            Span::styled(
                format!("{:>width$}", row.line_number, width = layout.number_width),
                gutter_style(row.kind, layout.style),
            )
        } else {
            Span::raw(" ".repeat(layout.number_width))
        };
        line.line.spans.splice(
            0..0,
            [Span::raw(layout.indent), number, Span::raw(CONTENT_GAP)],
        );
    }
    lines
}

fn content_spans(
    text: &str,
    kind: DiffLineKind,
    highlighted: Option<Vec<Span<'static>>>,
    style: DiffStyle,
) -> Vec<Span<'static>> {
    if matches!(kind, DiffLineKind::Added | DiffLineKind::Removed)
        && background(kind, style).is_none()
    {
        return vec![Span::styled(text.to_string(), fallback_style(kind, style))];
    }
    highlighted
        .filter(|spans| !spans.is_empty())
        .unwrap_or_else(|| vec![Span::styled(text.to_string(), fallback_style(kind, style))])
}

fn render_unparsed(source: &str, width: u16, indent: &'static str) -> Vec<MarkdownLine> {
    let prefix = format!("{indent}│ ");
    let prefix_width = u16::try_from(Line::from(prefix.as_str()).width()).unwrap_or(u16::MAX);
    let mut lines = render_literal_with_metadata(
        source,
        width.saturating_sub(prefix_width).max(1),
        Style::default().dim(),
    );
    prefix_lines(&mut lines, Line::from(prefix.clone()), Line::from(prefix));
    lines
}

fn hunk_separator(unchanged: usize, indent: &'static str, style: DiffStyle) -> MarkdownLine {
    let label = match unchanged {
        0 => "…".to_string(),
        1 => "… 1 unchanged line".to_string(),
        unchanged => format!("… {unchanged} unchanged lines"),
    };
    MarkdownLine {
        line: vec![
            indent.into(),
            Span::styled(label, Style::default().fg(style.gutter)),
        ]
        .into(),
        joiner_to_previous: LineJoiner::HardBreak,
        links: Vec::new(),
    }
}

fn gutter_style(kind: DiffLineKind, style: DiffStyle) -> Style {
    match kind {
        DiffLineKind::Added => Style::default().fg(style.insert_foreground),
        DiffLineKind::Removed => Style::default().fg(style.delete_foreground),
        DiffLineKind::Context => Style::default().fg(style.gutter),
    }
}

fn fallback_style(kind: DiffLineKind, style: DiffStyle) -> Style {
    match kind {
        DiffLineKind::Added => Style::default().fg(style.insert_foreground),
        DiffLineKind::Removed => Style::default().fg(style.delete_foreground),
        DiffLineKind::Context => Style::default().fg(style.equal_foreground),
    }
}

fn background(kind: DiffLineKind, style: DiffStyle) -> Option<ratatui::style::Color> {
    match kind {
        DiffLineKind::Added => style.insert_background,
        DiffLineKind::Removed => style.delete_background,
        DiffLineKind::Context => None,
    }
}

fn update_diff(source: &str, moved: bool) -> &str {
    if moved && let Some((diff, _)) = source.rsplit_once("\n\nMoved to: ") {
        diff
    } else {
        source
    }
}

fn expand_tabs(text: &str) -> Cow<'_, str> {
    if text.contains('\t') {
        Cow::Owned(text.replace('\t', &" ".repeat(TAB_WIDTH)))
    } else {
        Cow::Borrowed(text)
    }
}

fn fallback_change_counts(diff: &str) -> (usize, usize) {
    diff.lines()
        .filter(|line| !line.starts_with("+++") && !line.starts_with("---"))
        .fold((0, 0), |(added, removed), line| {
            (
                added + usize::from(line.starts_with('+')),
                removed + usize::from(line.starts_with('-')),
            )
        })
}

fn line_number_width(max_line_number: usize) -> usize {
    max_line_number.max(1).to_string().len()
}

#[derive(Debug, Clone, Copy)]
struct DiffLayout {
    number_width: usize,
    width: u16,
    style: DiffStyle,
    indent: &'static str,
}

struct DiffRow<'a> {
    line_number: usize,
    kind: DiffLineKind,
    text: &'a str,
    highlighted: Option<Vec<Span<'static>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffLineKind {
    Added,
    Removed,
    Context,
}
