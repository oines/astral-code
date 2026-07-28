//! File-change card presentation derived from Grok Build's edit block at
//! `47348d13ec4508dcfe440e34c6d511bb02998fb2` (Apache-2.0).
//!
//! The card remains a pure view over app-server `FileUpdateChange` values.

use std::path::Path;

use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::PatchChangeKind;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Text;

use super::RenderOptions;
use super::tool_header_with_title_style;
use crate::DisplayMode;
use crate::ToolPresentation;
use crate::render::EditViewerLine;

mod diff;

pub(super) fn render_edit(tool: &ToolPresentation, options: RenderOptions) -> Text<'static> {
    Text::from(
        render_edit_viewer_lines(tool, options)
            .into_iter()
            .map(|line| line.line)
            .collect::<Vec<_>>(),
    )
}

pub(super) fn render_edit_viewer_lines(
    tool: &ToolPresentation,
    options: RenderOptions,
) -> Vec<EditViewerLine> {
    let (added, removed) = diff::change_counts(&tool.changes);
    let suffix = if options.mode == DisplayMode::Collapsed && (added > 0 || removed > 0) {
        vec![
            format!(" +{added}").green(),
            "/".dim(),
            format!("-{removed}").red(),
        ]
    } else {
        Vec::new()
    };
    let mut lines = vec![EditViewerLine {
        line: tool_header_with_title_style(
            tool,
            edit_label(&tool.changes),
            &edit_title(tool),
            Style::default().fg(options.diff_style.path),
            suffix,
        ),
        copy: None,
    }];
    if options.mode == DisplayMode::Collapsed {
        return lines;
    }

    let mut body = render_changes(&tool.changes, options);
    if options.mode == DisplayMode::Truncated && body.len() > options.max_output_lines {
        let hidden = body.len() - options.max_output_lines;
        body.truncate(options.max_output_lines);
        body.push(EditViewerLine {
            line: vec!["  └ ".dim(), format!("{hidden} more lines").dim()].into(),
            copy: None,
        });
    }
    if !body.is_empty() {
        lines.push(EditViewerLine {
            line: Line::default(),
            copy: None,
        });
        lines.extend(body);
    }
    lines
}

fn render_changes(changes: &[FileUpdateChange], options: RenderOptions) -> Vec<EditViewerLine> {
    match changes {
        [] => Vec::new(),
        [change] => diff::render_file_change(change, 0, options.width, options.diff_style, "  "),
        changes => {
            let mut lines = Vec::new();
            for (index, change) in changes.iter().enumerate() {
                if index > 0 {
                    lines.push(EditViewerLine {
                        line: Line::default(),
                        copy: None,
                    });
                }
                lines.push(EditViewerLine {
                    line: render_change_header(change, options),
                    copy: None,
                });
                lines.extend(diff::render_file_change(
                    change,
                    index,
                    options.width,
                    options.diff_style,
                    "    ",
                ));
            }
            lines
        }
    }
}

fn render_change_header(change: &FileUpdateChange, options: RenderOptions) -> Line<'static> {
    let (added, removed) = diff::change_counts(std::slice::from_ref(change));
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
    let mut spans = vec![
        "  ".into(),
        operation,
        " ".into(),
        path.fg(options.diff_style.path),
    ];
    if added > 0 || removed > 0 {
        spans.extend([
            format!("  +{added}").green(),
            "/".dim(),
            format!("-{removed}").red(),
        ]);
    }
    spans.into()
}

fn edit_label(changes: &[FileUpdateChange]) -> &'static str {
    match changes {
        [
            FileUpdateChange {
                kind: PatchChangeKind::Add,
                ..
            },
        ] => "Creating",
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

fn compact_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map_or_else(|| path.to_string(), str::to_string)
}
