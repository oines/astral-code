use std::path::Path;

use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::PatchChangeKind;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use textwrap::Options;

use super::RenderOptions;
use super::tool_header;
use crate::DisplayMode;
use crate::ToolPresentation;

pub(super) fn render_edit(tool: &ToolPresentation, options: RenderOptions) -> Text<'static> {
    let (added, removed) = change_counts(&tool.changes);
    let suffix = if options.mode == DisplayMode::Collapsed && (added > 0 || removed > 0) {
        vec![
            format!(" +{added}").green(),
            "/".dim(),
            format!("-{removed}").red(),
        ]
    } else {
        Vec::new()
    };
    let mut lines = vec![tool_header(
        tool,
        edit_label(&tool.changes),
        &edit_title(tool),
        suffix,
    )];
    if options.mode == DisplayMode::Collapsed {
        return Text::from(lines);
    }

    let mut body = tool
        .changes
        .iter()
        .flat_map(|change| render_change(change, options.width))
        .collect::<Vec<_>>();
    if options.mode == DisplayMode::Truncated && body.len() > options.max_output_lines {
        let hidden = body.len() - options.max_output_lines;
        body.truncate(options.max_output_lines);
        body.push(vec!["  └ ".dim(), format!("{hidden} more lines").dim()].into());
    }
    lines.extend(body);
    Text::from(lines)
}

fn edit_label(changes: &[FileUpdateChange]) -> &'static str {
    match changes {
        [
            FileUpdateChange {
                kind: PatchChangeKind::Add,
                ..
            },
        ] => "Create",
        [
            FileUpdateChange {
                kind: PatchChangeKind::Delete,
                ..
            },
        ] => "Delete",
        [
            FileUpdateChange {
                kind: PatchChangeKind::Update { move_path: Some(_) },
                ..
            },
        ] => "Move",
        _ => "Edit",
    }
}

fn edit_title(tool: &ToolPresentation) -> String {
    let [change] = tool.changes.as_slice() else {
        return tool.title.clone();
    };
    let source = compact_path(&change.path);
    match &change.kind {
        PatchChangeKind::Update {
            move_path: Some(destination),
        } => format!(
            "{source} → {}",
            compact_path(&destination.display().to_string())
        ),
        PatchChangeKind::Add
        | PatchChangeKind::Delete
        | PatchChangeKind::Update { move_path: None } => source,
    }
}

fn render_change(change: &FileUpdateChange, width: u16) -> Vec<Line<'static>> {
    let (added, removed) = change_counts(std::slice::from_ref(change));
    let (operation, path) = match &change.kind {
        PatchChangeKind::Add => ("A".green(), change.path.clone()),
        PatchChangeKind::Delete => ("D".red(), change.path.clone()),
        PatchChangeKind::Update {
            move_path: Some(move_path),
        } => (
            "R".magenta(),
            format!("{} → {}", change.path, move_path.display()),
        ),
        PatchChangeKind::Update { move_path: None } => ("M".cyan(), change.path.clone()),
    };
    let mut lines = vec![
        vec![
            "  ".into(),
            operation,
            " ".dim(),
            path.dim(),
            format!("  +{added}").green(),
            "/".dim(),
            format!("-{removed}").red(),
        ]
        .into(),
    ];
    lines.extend(diff_lines(change).flat_map(|line| wrap_diff_line(&line.text, line.kind, width)));
    lines
}

fn diff_lines(change: &FileUpdateChange) -> impl Iterator<Item = DiffLine> + '_ {
    change
        .diff
        .lines()
        .filter_map(move |line| match &change.kind {
            PatchChangeKind::Add => Some(DiffLine::new(format!("+{line}"), DiffLineKind::Added)),
            PatchChangeKind::Delete => {
                Some(DiffLine::new(format!("-{line}"), DiffLineKind::Removed))
            }
            PatchChangeKind::Update { .. }
                if line.starts_with("--- ")
                    || line.starts_with("+++ ")
                    || line.starts_with("diff --git ")
                    || line.starts_with("index ")
                    || line.starts_with("Moved to: ") =>
            {
                None
            }
            PatchChangeKind::Update { .. } if line.starts_with("@@") => {
                Some(DiffLine::new(line.to_string(), DiffLineKind::Hunk))
            }
            PatchChangeKind::Update { .. } if line.starts_with('+') => {
                Some(DiffLine::new(line.to_string(), DiffLineKind::Added))
            }
            PatchChangeKind::Update { .. } if line.starts_with('-') => {
                Some(DiffLine::new(line.to_string(), DiffLineKind::Removed))
            }
            PatchChangeKind::Update { .. } if line.starts_with('\\') => {
                Some(DiffLine::new(line.to_string(), DiffLineKind::Note))
            }
            PatchChangeKind::Update { .. } => {
                Some(DiffLine::new(line.to_string(), DiffLineKind::Context))
            }
        })
}

fn wrap_diff_line(text: &str, kind: DiffLineKind, width: u16) -> Vec<Line<'static>> {
    let options = Options::new(usize::from(width).max(1))
        .initial_indent("  │ ")
        .subsequent_indent("  │   ")
        .word_separator(textwrap::WordSeparator::AsciiSpace)
        .word_splitter(textwrap::WordSplitter::NoHyphenation)
        .break_words(true);
    textwrap::wrap(text, &options)
        .into_iter()
        .map(|line| Line::from(Span::styled(line.into_owned(), kind.style())))
        .collect()
}

fn change_counts(changes: &[FileUpdateChange]) -> (usize, usize) {
    changes.iter().fold((0, 0), |(added, removed), change| {
        let (change_added, change_removed) = match &change.kind {
            PatchChangeKind::Add => (change.diff.lines().count(), 0),
            PatchChangeKind::Delete => (0, change.diff.lines().count()),
            PatchChangeKind::Update { .. } => change
                .diff
                .lines()
                .filter(|line| !line.starts_with("+++") && !line.starts_with("---"))
                .fold((0, 0), |(added, removed), line| {
                    (
                        added + usize::from(line.starts_with('+')),
                        removed + usize::from(line.starts_with('-')),
                    )
                }),
        };
        (added + change_added, removed + change_removed)
    })
}

fn compact_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map_or_else(|| path.to_string(), str::to_string)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffLineKind {
    Hunk,
    Added,
    Removed,
    Context,
    Note,
}

impl DiffLineKind {
    fn style(self) -> Style {
        match self {
            Self::Hunk => Style::default().cyan().dim(),
            Self::Added => Style::default().green(),
            Self::Removed => Style::default().red(),
            Self::Context | Self::Note => Style::default().dim(),
        }
    }
}

struct DiffLine {
    text: String,
    kind: DiffLineKind,
}

impl DiffLine {
    fn new(text: String, kind: DiffLineKind) -> Self {
        Self { text, kind }
    }
}
