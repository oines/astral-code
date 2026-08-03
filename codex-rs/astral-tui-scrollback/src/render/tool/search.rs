//! Glob/Grep cards derived from Grok Build's `SearchToolCallBlock` at commit
//! `47348d13ec4508dcfe440e34c6d511bb02998fb2` (Apache-2.0).

use codex_app_server_protocol::CoreToolCallStatus;
use codex_app_server_protocol::ThreadItem;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::DisplayMode;
use crate::EntryDisplayState;
use crate::LineJoiner;
use crate::MarkdownLine;
use crate::render_literal_with_metadata;
use crate::search_tool::GrepOutputMode;
use crate::search_tool::SearchCall;
use crate::search_tool::SearchKind;
use crate::wrap_styled_line_with_metadata;

use super::super::EntryRenderOptions;
use super::super::prefix_lines;
use super::super::truncate_with_ellipsis;

pub(super) fn render(
    item: &ThreadItem,
    state: EntryDisplayState,
    options: EntryRenderOptions,
) -> Option<Vec<MarkdownLine>> {
    let call = SearchCall::from_item(item)?;
    let mut lines = render_header(call, state.mode(), options);
    if state.mode() == DisplayMode::Collapsed {
        return Some(lines);
    }
    let body = if let Some(error) = call.failure_text() {
        render_notice(error, options.width, Style::default().red())
    } else if let Some(result) = call.result() {
        match call.kind() {
            SearchKind::Glob { .. } => render_glob(result, options),
            SearchKind::Grep { output_mode, .. } => render_grep(result, output_mode, call, options),
        }
    } else {
        render_metadata_only(call, options)
    };
    if !body.is_empty() {
        lines.push(markdown_line(Line::default()));
        lines.extend(body);
    }
    Some(lines)
}

fn render_header(
    call: SearchCall<'_>,
    mode: DisplayMode,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    let mut body = match call.kind() {
        SearchKind::Glob { pattern, path } => {
            let mut body = Line::from(Span::styled(
                pattern.to_string(),
                options.markdown_style.inline_code,
            ));
            append_scope(&mut body, None, path, options);
            body
        }
        SearchKind::Grep {
            pattern,
            path,
            glob,
            ..
        } => {
            let glob_is_term = (pattern.is_empty() || pattern == ".") && glob.is_some();
            let mut body = Line::from(Span::styled(
                if glob_is_term {
                    glob.unwrap_or_default().to_string()
                } else {
                    format!("{pattern:?}")
                },
                options.markdown_style.inline_code,
            ));
            append_scope(
                &mut body,
                if glob_is_term { None } else { glob },
                path,
                options,
            );
            body
        }
    };
    body.push_span(format!(" {}", result_summary(call)).dim());
    if let Some(duration_ms) = call.duration_ms().filter(|duration| *duration >= 0) {
        body.push_span(format!("  {}", duration_label(duration_ms)).dim());
    }

    let prefix = Line::from(vec![status_marker(call.status()), "Search ".bold().dim()]);
    let prefix_width = u16::try_from(prefix.width()).unwrap_or(u16::MAX);
    let mut lines =
        wrap_styled_line_with_metadata(&body, options.width.saturating_sub(prefix_width).max(1));
    prefix_lines(
        &mut lines,
        prefix,
        Line::from(" ".repeat(usize::from(prefix_width))),
    );
    if mode == DisplayMode::Collapsed {
        let wrapped = lines.len() > 1;
        lines.truncate(1);
        if wrapped && let Some(line) = lines.first_mut() {
            truncate_with_ellipsis(line, options.width);
        }
    }
    lines
}

fn append_scope(
    body: &mut Line<'static>,
    glob: Option<&str>,
    path: Option<&str>,
    options: EntryRenderOptions,
) {
    if let Some(glob) = glob {
        body.push_span(" in ".dim());
        body.push_span(Span::styled(
            glob.to_string(),
            options.markdown_style.inline_code,
        ));
    }
    if let Some(path) = path {
        body.push_span(" in ".dim());
        body.push_span(Span::styled(
            path.to_string(),
            Style::default().fg(options.diff_style.path),
        ));
    }
}

fn render_glob(result: &str, options: EntryRenderOptions) -> Vec<MarkdownLine> {
    let paths = result_lines(result);
    if paths.is_empty() {
        return vec![markdown_line(Line::from("  (no results)".dim()))];
    }
    let mut lines = render_paths(paths.into_iter(), options);
    if preview_truncated(result) {
        lines.push(markdown_line(Line::from("  … results truncated".dim())));
    }
    lines
}

fn render_grep(
    result: &str,
    output_mode: GrepOutputMode,
    call: SearchCall<'_>,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    let mut lines = render_metadata_only(call, options);
    lines.push(markdown_line(Line::default()));
    let result_lines = grep_result_lines(result, output_mode);
    if result_lines.is_empty() {
        lines.push(markdown_line(Line::from("  (no results)".dim())));
    } else {
        match output_mode {
            GrepOutputMode::Files => lines.extend(render_paths(result_lines.into_iter(), options)),
            GrepOutputMode::Count => render_counts(&mut lines, result_lines, options),
            GrepOutputMode::Content => render_matches(&mut lines, result_lines, options),
        }
    }
    if pagination_applied(result) {
        lines.push(markdown_line(Line::from("  … pagination applied".dim())));
    }
    if preview_truncated(result) {
        lines.push(markdown_line(Line::from("  … results truncated".dim())));
    }
    lines
}

fn render_metadata_only(call: SearchCall<'_>, options: EntryRenderOptions) -> Vec<MarkdownLine> {
    let SearchKind::Grep {
        output_mode,
        file_type,
        ignore_case,
        multiline,
        ..
    } = call.kind()
    else {
        return Vec::new();
    };
    let mode = match output_mode {
        GrepOutputMode::Content => "pattern",
        GrepOutputMode::Files => "files",
        GrepOutputMode::Count => "count",
    };
    let mut line = Line::from(vec!["  mode: ".dim(), mode.into()]);
    if let Some(file_type) = file_type {
        line.push_span(", type: ".dim());
        line.push_span(Span::styled(
            file_type.to_string(),
            options.markdown_style.inline_code,
        ));
    }
    if ignore_case {
        line.push_span(", case-insensitive: true".dim());
    }
    if multiline {
        line.push_span(", multiline: true".dim());
    }
    vec![markdown_line(line)]
}

fn render_counts(lines: &mut Vec<MarkdownLine>, results: Vec<&str>, options: EntryRenderOptions) {
    for raw in results {
        let Some((path, count)) = raw.rsplit_once(':') else {
            lines.extend(render_plain(raw, options));
            continue;
        };
        let line = Line::from(vec![
            Span::styled(
                path.to_string(),
                Style::default().fg(options.diff_style.path),
            ),
            format!(":{count}").into(),
        ]);
        lines.extend(wrap_indented(line, options));
    }
}

fn render_matches(lines: &mut Vec<MarkdownLine>, results: Vec<&str>, options: EntryRenderOptions) {
    let mut previous_path = None;
    for raw in results {
        if raw == "--" {
            lines.push(markdown_line(Line::from("    …".dim())));
            continue;
        }
        let parsed = split_numbered_result(raw, ':')
            .map(|parts| (parts, false))
            .or_else(|| split_numbered_result(raw, '-').map(|parts| (parts, true)));
        let Some(((path, number, content), context)) = parsed else {
            lines.extend(render_plain(raw, options));
            continue;
        };
        if previous_path != Some(path) {
            if previous_path.is_some() {
                lines.push(markdown_line(Line::default()));
            }
            lines.extend(render_paths(std::iter::once(path), options));
            previous_path = Some(path);
        }
        lines.extend(render_match_line(number, content, context, options));
    }
}

fn render_match_line(
    number: &str,
    content: &str,
    context: bool,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    let prefix = Line::from(vec![
        "    ".into(),
        format!("{number:>4}").dim(),
        if context { "- ".dim() } else { "  ".into() },
    ]);
    let prefix_width = u16::try_from(prefix.width()).unwrap_or(u16::MAX);
    let content = if context {
        Line::from(content.to_string().dim())
    } else {
        Line::from(content.to_string())
    };
    let mut lines =
        wrap_styled_line_with_metadata(&content, options.width.saturating_sub(prefix_width).max(1));
    prefix_lines(
        &mut lines,
        prefix,
        Line::from(" ".repeat(usize::from(prefix_width))),
    );
    lines
}

fn render_paths<'a>(
    paths: impl Iterator<Item = &'a str>,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    paths
        .flat_map(|path| {
            wrap_indented(
                Line::from(Span::styled(
                    path.to_string(),
                    Style::default().fg(options.diff_style.path),
                )),
                options,
            )
        })
        .collect()
}

fn render_plain(raw: &str, options: EntryRenderOptions) -> Vec<MarkdownLine> {
    wrap_indented(Line::from(raw.to_string()), options)
}

fn wrap_indented(line: Line<'static>, options: EntryRenderOptions) -> Vec<MarkdownLine> {
    let mut lines = wrap_styled_line_with_metadata(&line, options.width.saturating_sub(2).max(1));
    prefix_lines(&mut lines, Line::from("  "), Line::from("  "));
    lines
}

fn result_summary(call: SearchCall<'_>) -> String {
    if call.failed() {
        return "(failed)".to_string();
    }
    let Some(result) = call.result() else {
        return match call.kind() {
            SearchKind::Glob { .. }
            | SearchKind::Grep {
                output_mode: GrepOutputMode::Files,
                ..
            } => "(searching files)".to_string(),
            SearchKind::Grep { .. } => "(searching)".to_string(),
        };
    };
    match call.kind() {
        SearchKind::Glob { .. } => {
            let count = result_lines(result).len();
            if preview_truncated(result) && count > 0 {
                format!("({count}+ files)")
            } else {
                count_summary(count, "file", "files")
            }
        }
        SearchKind::Grep { output_mode, .. } => grep_summary(result, output_mode),
    }
}

fn grep_summary(result: &str, output_mode: GrepOutputMode) -> String {
    let count = match output_mode {
        GrepOutputMode::Files => parse_found_count(result)
            .unwrap_or_else(|| grep_result_lines(result, output_mode).len()),
        GrepOutputMode::Count => parse_found_count(result).unwrap_or_else(|| {
            grep_result_lines(result, output_mode)
                .into_iter()
                .filter_map(|line| line.rsplit_once(':')?.1.parse::<usize>().ok())
                .sum()
        }),
        GrepOutputMode::Content => {
            let results = grep_result_lines(result, output_mode);
            let count = results
                .iter()
                .filter(|line| split_numbered_result(line, ':').is_some())
                .count();
            if count == 0 && results.iter().any(|line| *line != "--") {
                return "(results)".to_string();
            }
            count
        }
    };
    match output_mode {
        GrepOutputMode::Files => count_summary(count, "file", "files"),
        GrepOutputMode::Content | GrepOutputMode::Count => count_summary(count, "match", "matches"),
    }
}

fn result_lines(result: &str) -> Vec<&str> {
    result
        .lines()
        .filter(|line| {
            let line = line.trim();
            !line.is_empty()
                && line != "No files found"
                && !line.starts_with("(Results are truncated.")
                && line != "[truncated]"
        })
        .collect()
}

fn grep_result_lines(result: &str, output_mode: GrepOutputMode) -> Vec<&str> {
    result
        .lines()
        .filter(|line| {
            let line = line.trim();
            if line.is_empty()
                || line == "No files found"
                || line == "No matches found"
                || line == "[truncated]"
                || line.starts_with("[Showing results with pagination")
            {
                return false;
            }
            match output_mode {
                GrepOutputMode::Files | GrepOutputMode::Count => !line.starts_with("Found "),
                GrepOutputMode::Content => true,
            }
        })
        .collect()
}

fn parse_found_count(result: &str) -> Option<usize> {
    result
        .lines()
        .find_map(|line| line.strip_prefix("Found "))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|count| count.parse().ok())
}

fn split_numbered_result(line: &str, delimiter: char) -> Option<(&str, &str, &str)> {
    for (index, _) in line.match_indices(delimiter) {
        let rest = &line[index + delimiter.len_utf8()..];
        let Some((number, content)) = rest.split_once(delimiter) else {
            continue;
        };
        if !number.is_empty() && number.chars().all(|character| character.is_ascii_digit()) {
            return Some((&line[..index], number, content));
        }
    }
    None
}

fn preview_truncated(result: &str) -> bool {
    result.contains("(Results are truncated.") || result.lines().any(|line| line == "[truncated]")
}

fn pagination_applied(result: &str) -> bool {
    result.contains("[Showing results with pagination")
        || result.contains("with pagination =")
        || result.lines().any(|line| {
            line.starts_with("Found ") && (line.contains(" limit:") || line.contains(" offset:"))
        })
}

fn count_summary(count: usize, singular: &str, plural: &str) -> String {
    match count {
        0 => format!("(no {plural})"),
        1 => format!("(1 {singular})"),
        count => format!("({count} {plural})"),
    }
}

fn render_notice(text: &str, width: u16, style: Style) -> Vec<MarkdownLine> {
    let mut lines = render_literal_with_metadata(text, width.saturating_sub(4).max(1), style);
    prefix_lines(
        &mut lines,
        Line::from("  │ ".dim()),
        Line::from("  │ ".dim()),
    );
    lines
}

fn status_marker(status: CoreToolCallStatus) -> Span<'static> {
    match status {
        CoreToolCallStatus::InProgress => "◇ ".magenta(),
        CoreToolCallStatus::Completed => "◆ ".green(),
        CoreToolCallStatus::Failed => "× ".red(),
        CoreToolCallStatus::Interrupted => "– ".dim(),
    }
}

fn duration_label(duration_ms: i64) -> String {
    if duration_ms < 1_000 {
        format!("{}ms", duration_ms.max(0))
    } else {
        format!("{:.1}s", duration_ms.max(0) as f64 / 1_000.0)
    }
}

fn markdown_line(line: Line<'static>) -> MarkdownLine {
    MarkdownLine {
        line,
        joiner_to_previous: LineJoiner::HardBreak,
        links: Vec::new(),
    }
}
