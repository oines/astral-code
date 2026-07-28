//! Structured diff rows derived from Grok Build's edit renderer at commit
//! `47348d13ec4508dcfe440e34c6d511bb02998fb2` (Apache-2.0).
//!
//! Astral keeps this renderer stateless: app-server `FileUpdateChange` values
//! remain authoritative, while the view reconstructs hunk-local old/new
//! syntax state, line-number gutters, semantic backgrounds, and wrapping.

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
use textwrap::Options;
use textwrap::core::display_width;

use crate::markdown::CodeLineHighlighter;
use crate::render::DiffStyle;
use crate::render::EditCopyKind;
use crate::render::EditCopyLine;
use crate::render::EditViewerLine;

const CONTENT_GAP: &str = "  ";
const TAB_WIDTH: usize = 4;

pub(super) fn render_file_change(
    change: &FileUpdateChange,
    change_index: usize,
    width: u16,
    style: DiffStyle,
    indent: &'static str,
) -> Vec<EditViewerLine> {
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
            change_index,
            path,
            width,
            style,
            indent,
        ),
        PatchChangeKind::Delete => render_whole_file(
            &change.diff,
            DiffLineKind::Removed,
            change_index,
            path,
            width,
            style,
            indent,
        ),
        PatchChangeKind::Update { .. } => {
            render_update(&change.diff, change_index, path, width, style, indent)
        }
    }
}

pub(super) fn change_counts(changes: &[FileUpdateChange]) -> (usize, usize) {
    changes.iter().fold((0, 0), |(added, removed), change| {
        let (change_added, change_removed) = match &change.kind {
            PatchChangeKind::Add => (change.diff.lines().count(), 0),
            PatchChangeKind::Delete => (0, change.diff.lines().count()),
            PatchChangeKind::Update { .. } => Patch::from_str(&change.diff).map_or_else(
                |_| fallback_change_counts(&change.diff),
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
            ),
        };
        (added + change_added, removed + change_removed)
    })
}

fn render_whole_file(
    source: &str,
    kind: DiffLineKind,
    change_index: usize,
    path: &Path,
    width: u16,
    style: DiffStyle,
    indent: &'static str,
) -> Vec<EditViewerLine> {
    let line_count = source.lines().count();
    let number_width = line_number_width(line_count);
    let mut highlighter = CodeLineHighlighter::for_path(path, source, style.syntax_theme);
    source
        .lines()
        .enumerate()
        .flat_map(|(index, raw)| {
            let text = expand_tabs(raw);
            let highlighted = highlighter
                .as_mut()
                .and_then(|highlighter| highlighter.highlight_line(&text));
            let line_number = index + 1;
            render_diff_line(
                line_number,
                kind,
                &text,
                highlighted,
                number_width,
                width,
                style,
                indent,
                EditCopyLine {
                    change_index,
                    kind: kind.copy_kind(),
                    text: raw.to_string(),
                    old_line: (kind == DiffLineKind::Removed).then_some(line_number),
                    new_line: (kind == DiffLineKind::Added).then_some(line_number),
                },
            )
        })
        .collect()
}

fn render_update(
    source: &str,
    change_index: usize,
    path: &Path,
    width: u16,
    style: DiffStyle,
    indent: &'static str,
) -> Vec<EditViewerLine> {
    let normalized;
    let patch = match Patch::from_str(source) {
        Ok(patch) => patch,
        Err(_) => {
            // App-server updates may contain a bare hunk without file headers
            // or a trailing newline. Add display-only headers for parsing;
            // the original path and text remain authoritative for rendering.
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
    let mut lines = Vec::new();
    let mut previous_new_end = None;

    for hunk in patch.hunks() {
        if let Some(previous_new_end) = previous_new_end {
            let unchanged = hunk.new_range().start().saturating_sub(previous_new_end);
            lines.push(hunk_separator(unchanged, indent, style));
        }

        let mut old_highlighter = CodeLineHighlighter::for_path(path, source, style.syntax_theme);
        let mut new_highlighter = CodeLineHighlighter::for_path(path, source, style.syntax_theme);
        let mut old_line = hunk.old_range().start();
        let mut new_line = hunk.new_range().start();

        for line in hunk.lines() {
            match line {
                DiffyLine::Insert(text) => {
                    let raw = text.trim_end_matches(['\n', '\r']);
                    let text = expand_tabs(raw);
                    let highlighted = new_highlighter
                        .as_mut()
                        .and_then(|highlighter| highlighter.highlight_line(&text));
                    lines.extend(render_diff_line(
                        new_line,
                        DiffLineKind::Added,
                        &text,
                        highlighted,
                        number_width,
                        width,
                        style,
                        indent,
                        EditCopyLine {
                            change_index,
                            kind: EditCopyKind::Insert,
                            text: raw.to_string(),
                            old_line: None,
                            new_line: Some(new_line),
                        },
                    ));
                    new_line += 1;
                }
                DiffyLine::Delete(text) => {
                    let raw = text.trim_end_matches(['\n', '\r']);
                    let text = expand_tabs(raw);
                    let highlighted = old_highlighter
                        .as_mut()
                        .and_then(|highlighter| highlighter.highlight_line(&text));
                    lines.extend(render_diff_line(
                        old_line,
                        DiffLineKind::Removed,
                        &text,
                        highlighted,
                        number_width,
                        width,
                        style,
                        indent,
                        EditCopyLine {
                            change_index,
                            kind: EditCopyKind::Delete,
                            text: raw.to_string(),
                            old_line: Some(old_line),
                            new_line: None,
                        },
                    ));
                    old_line += 1;
                }
                DiffyLine::Context(text) => {
                    let raw = text.trim_end_matches(['\n', '\r']);
                    let text = expand_tabs(raw);
                    if let Some(highlighter) = old_highlighter.as_mut() {
                        let _ = highlighter.highlight_line(&text);
                    }
                    let highlighted = new_highlighter
                        .as_mut()
                        .and_then(|highlighter| highlighter.highlight_line(&text));
                    lines.extend(render_diff_line(
                        new_line,
                        DiffLineKind::Context,
                        &text,
                        highlighted,
                        number_width,
                        width,
                        style,
                        indent,
                        EditCopyLine {
                            change_index,
                            kind: EditCopyKind::Context,
                            text: raw.to_string(),
                            old_line: Some(old_line),
                            new_line: Some(new_line),
                        },
                    ));
                    old_line += 1;
                    new_line += 1;
                }
            }
        }
        previous_new_end = Some(hunk.new_range().end());
    }
    lines
}

#[allow(clippy::too_many_arguments)]
fn render_diff_line(
    line_number: usize,
    kind: DiffLineKind,
    text: &str,
    highlighted: Option<Vec<Span<'static>>>,
    number_width: usize,
    width: u16,
    style: DiffStyle,
    indent: &'static str,
    copy: EditCopyLine,
) -> Vec<EditViewerLine> {
    let prefix_width = display_width(indent) + number_width + display_width(CONTENT_GAP);
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
    let spans = content_spans(text, kind, highlighted, style);
    wrap_styled_spans(&spans, content_width)
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| {
            let mut spans = vec![indent.into()];
            if index == 0 {
                spans.push(Span::styled(
                    format!("{line_number:>number_width$}"),
                    gutter_style(kind, style),
                ));
            } else {
                spans.push(" ".repeat(number_width).into());
            }
            spans.push(CONTENT_GAP.into());
            let background = background(kind, style);
            let mut chunk_width = 0;
            if chunk.is_empty() {
                spans.push(styled_content(" ", kind, style, background));
                chunk_width = 1;
            } else {
                for mut span in chunk {
                    chunk_width += display_width(span.content.as_ref());
                    if let Some(background) = background {
                        span.style = span.style.bg(background);
                    }
                    spans.push(span);
                }
            }
            let padding = content_width.saturating_sub(chunk_width);
            if padding > 0 {
                let mut padding_style = Style::default();
                if let Some(background) = background {
                    padding_style = padding_style.bg(background);
                }
                spans.push(Span::styled(" ".repeat(padding), padding_style));
            }
            EditViewerLine {
                line: Line::from(spans),
                copy: Some(copy.clone()),
            }
        })
        .collect()
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

fn styled_content(
    text: &str,
    kind: DiffLineKind,
    style: DiffStyle,
    background: Option<ratatui::style::Color>,
) -> Span<'static> {
    let mut content_style = fallback_style(kind, style);
    if let Some(background) = background {
        content_style = content_style.bg(background);
    }
    Span::styled(text.to_string(), content_style)
}

fn render_unparsed(source: &str, width: u16, indent: &'static str) -> Vec<EditViewerLine> {
    let prefix = format!("{indent}│ ");
    let continuation = format!("{indent}│   ");
    let options = Options::new(usize::from(width).max(1))
        .initial_indent(&prefix)
        .subsequent_indent(&continuation)
        .word_separator(textwrap::WordSeparator::AsciiSpace)
        .word_splitter(textwrap::WordSplitter::NoHyphenation)
        .break_words(true);
    source
        .lines()
        .flat_map(|line| textwrap::wrap(line, &options))
        .map(|line| EditViewerLine {
            line: Line::from(line.into_owned().dim()),
            copy: None,
        })
        .collect()
}

fn hunk_separator(unchanged: usize, indent: &'static str, style: DiffStyle) -> EditViewerLine {
    let label = match unchanged {
        0 => "…".to_string(),
        1 => "… 1 unchanged line".to_string(),
        unchanged => format!("… {unchanged} unchanged lines"),
    };
    EditViewerLine {
        line: vec![
            indent.into(),
            Span::styled(label, Style::default().fg(style.gutter)),
        ]
        .into(),
        copy: None,
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

fn wrap_styled_spans(spans: &[Span<'static>], max_width: usize) -> Vec<Vec<Span<'static>>> {
    let mut lines = Vec::new();
    let mut current = Vec::new();
    let mut current_width = 0;

    for span in spans {
        let mut remaining = span.content.as_ref();
        while !remaining.is_empty() {
            let mut byte_end = 0;
            let mut segment_width = 0;
            for character in remaining.chars() {
                let mut buffer = [0; 4];
                let character_width = display_width(character.encode_utf8(&mut buffer));
                if current_width + segment_width + character_width > max_width {
                    break;
                }
                byte_end += character.len_utf8();
                segment_width += character_width;
            }
            if byte_end == 0 {
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                    current_width = 0;
                    continue;
                }
                let Some(character) = remaining.chars().next() else {
                    break;
                };
                byte_end = character.len_utf8();
                let mut buffer = [0; 4];
                segment_width = display_width(character.encode_utf8(&mut buffer)).max(1);
            }
            let (segment, rest) = remaining.split_at(byte_end);
            current.push(Span::styled(segment.to_string(), span.style));
            current_width += segment_width;
            remaining = rest;
            if current_width >= max_width {
                lines.push(std::mem::take(&mut current));
                current_width = 0;
            }
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffLineKind {
    Added,
    Removed,
    Context,
}

impl DiffLineKind {
    fn copy_kind(self) -> EditCopyKind {
        match self {
            Self::Added => EditCopyKind::Insert,
            Self::Removed => EditCopyKind::Delete,
            Self::Context => EditCopyKind::Context,
        }
    }
}
