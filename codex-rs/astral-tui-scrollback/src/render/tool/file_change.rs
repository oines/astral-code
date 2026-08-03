//! File-change cards derived from Grok Build's edit block at commit
//! `47348d13ec4508dcfe440e34c6d511bb02998fb2` (Apache-2.0).
//!
//! The renderer is a pure view over app-server `FileUpdateChange` values. It
//! does not classify `Edit`, `Write`, or `apply_patch` tool names.

use std::path::Path;

use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::PatchChangeKind;
use codex_app_server_protocol::ThreadItem;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::DisplayMode;
use crate::EntryDisplayState;
use crate::LineJoiner;
use crate::LiveItem;
use crate::MarkdownLine;
use crate::wrap_styled_line_with_metadata;

use super::super::EntryRenderOptions;
use super::super::prefix_lines;
use super::super::truncate_with_ellipsis;

#[path = "file_change/diff.rs"]
mod diff;

pub(super) fn render(
    item: &ThreadItem,
    live: &LiveItem,
    state: EntryDisplayState,
    options: EntryRenderOptions,
) -> Option<Vec<MarkdownLine>> {
    let ThreadItem::FileChange {
        changes, status, ..
    } = item
    else {
        return None;
    };
    let changes = if changes.is_empty() {
        live.file_changes()
    } else {
        changes
    };
    let mut lines = render_header(changes, status, state.mode(), options);
    if state.mode() == DisplayMode::Collapsed || changes.is_empty() {
        return Some(lines);
    }

    let body = render_changes(changes, options);
    if !body.is_empty() {
        lines.push(markdown_line(Vec::new()));
        lines.extend(body);
    }
    Some(lines)
}

fn render_header(
    changes: &[FileUpdateChange],
    status: &PatchApplyStatus,
    mode: DisplayMode,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    let label = edit_label(changes);
    let prefix = Line::from(vec![
        status_marker(status),
        format!("{label} ").bold().dim(),
    ]);
    let prefix_width = u16::try_from(prefix.width()).unwrap_or(u16::MAX);
    let title = edit_title(changes);
    let (added, removed) = diff::change_counts(changes);
    let mut title_line = Line::from(Span::styled(
        title,
        Style::default().fg(options.diff_style.path),
    ));
    if mode == DisplayMode::Collapsed && (added > 0 || removed > 0) {
        title_line.push_span(format!("  +{added}").green());
        title_line.push_span("/".dim());
        title_line.push_span(format!("-{removed}").red());
    }
    let mut lines = wrap_styled_line_with_metadata(
        &title_line,
        options.width.saturating_sub(prefix_width).max(1),
    );
    prefix_lines(
        &mut lines,
        prefix,
        Line::from(" ".repeat(usize::from(prefix_width))),
    );
    if mode == DisplayMode::Collapsed {
        lines.truncate(1);
        if let Some(line) = lines.first_mut() {
            truncate_with_ellipsis(line, options.width);
        }
    }
    lines
}

fn render_changes(changes: &[FileUpdateChange], options: EntryRenderOptions) -> Vec<MarkdownLine> {
    match changes {
        [] => Vec::new(),
        [change] => diff::render_file_change(change, options.width, options.diff_style, "  "),
        changes => {
            let mut lines = Vec::new();
            for (index, change) in changes.iter().enumerate() {
                if index > 0 {
                    lines.push(markdown_line(Vec::new()));
                }
                lines.extend(render_change_header(change, options));
                lines.extend(diff::render_file_change(
                    change,
                    options.width,
                    options.diff_style,
                    "    ",
                ));
            }
            lines
        }
    }
}

fn render_change_header(
    change: &FileUpdateChange,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
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
    let prefix = Line::from(vec!["  ".into(), operation, " ".into()]);
    let prefix_width = u16::try_from(prefix.width()).unwrap_or(u16::MAX);
    let mut body = Line::from(Span::styled(
        path,
        Style::default().fg(options.diff_style.path),
    ));
    if added > 0 || removed > 0 {
        body.push_span(format!("  +{added}").green());
        body.push_span("/".dim());
        body.push_span(format!("-{removed}").red());
    }
    let mut lines =
        wrap_styled_line_with_metadata(&body, options.width.saturating_sub(prefix_width).max(1));
    prefix_lines(
        &mut lines,
        prefix,
        Line::from(" ".repeat(usize::from(prefix_width))),
    );
    lines
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

fn edit_title(changes: &[FileUpdateChange]) -> String {
    let [change] = changes else {
        return if changes.is_empty() {
            "files".to_string()
        } else {
            format!("{} files", changes.len())
        };
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

fn status_marker(status: &PatchApplyStatus) -> Span<'static> {
    match status {
        PatchApplyStatus::InProgress => "◇ ".magenta(),
        PatchApplyStatus::Completed => "◆ ".green(),
        PatchApplyStatus::Failed => "× ".red(),
        PatchApplyStatus::Declined => "– ".dim(),
    }
}

fn markdown_line(spans: Vec<Span<'static>>) -> MarkdownLine {
    MarkdownLine {
        line: Line::from(spans),
        joiner_to_previous: LineJoiner::HardBreak,
        links: Vec::new(),
    }
}
