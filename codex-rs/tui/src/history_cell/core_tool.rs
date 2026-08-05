//! Astral core tool-call history cells.

use super::*;
use codex_ansi_escape::ansi_escape_line;
use std::borrow::Cow;

#[derive(Debug)]
pub(crate) struct CoreToolCallCell {
    id: String,
    tool: String,
    arguments: serde_json::Value,
    status: codex_app_server_protocol::CoreToolCallStatus,
    result: Option<String>,
    error: Option<String>,
    duration_ms: Option<i64>,
}

impl CoreToolCallCell {
    pub(crate) fn from_item(item: codex_app_server_protocol::ThreadItem) -> Option<Self> {
        let codex_app_server_protocol::ThreadItem::CoreToolCall {
            id,
            tool,
            arguments,
            status,
            result,
            error,
            duration_ms,
        } = item
        else {
            return None;
        };
        Some(Self {
            id,
            tool,
            arguments,
            status,
            result,
            error,
            duration_ms,
        })
    }

    pub(crate) fn call_id(&self) -> &str {
        &self.id
    }

    pub(crate) fn update_from_item(&mut self, item: codex_app_server_protocol::ThreadItem) {
        let Some(updated) = Self::from_item(item) else {
            return;
        };
        *self = updated;
    }

    fn succeeded(&self) -> Option<bool> {
        match self.status {
            codex_app_server_protocol::CoreToolCallStatus::InProgress => None,
            codex_app_server_protocol::CoreToolCallStatus::Completed => Some(true),
            codex_app_server_protocol::CoreToolCallStatus::Failed
            | codex_app_server_protocol::CoreToolCallStatus::Interrupted => Some(false),
        }
    }
}

impl HistoryCell for CoreToolCallCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = self.invocation_lines(width);
        let inline_invocation = lines.len() == 1;

        if let Some(detail) = self.detail_summary(width as usize) {
            let detail_line = Line::from(detail.dim());
            let detail_wrap_width = (width as usize).saturating_sub(4).max(1);
            let wrapped = adaptive_wrap_line(
                &detail_line,
                RtOptions::new(detail_wrap_width)
                    .initial_indent("".into())
                    .subsequent_indent("    ".into()),
            );
            let body_lines = wrapped.iter().map(line_to_static).collect::<Vec<_>>();
            let initial_prefix: Span<'static> = if inline_invocation {
                "  └ ".dim()
            } else {
                "    ".into()
            };
            lines.extend(prefix_lines(body_lines, initial_prefix, "    ".into()));
        }

        lines
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        let header_text = if self.succeeded().is_some() {
            "Called"
        } else {
            "Calling"
        };
        let mut lines = vec![Line::from(format!(
            "{header_text} {}",
            format_core_tool_invocation(&self.tool, &self.arguments)
        ))];
        if let Some(detail) = self.detail_summary(RAW_TOOL_OUTPUT_WIDTH) {
            lines.push(Line::from(detail));
        }
        lines
    }

    fn transcript_presentation(&self) -> HistoryCellPresentation {
        if self.has_detail() {
            HistoryCellPresentation::two_state(astral_tui::DisplayMode::Collapsed).with_groupable()
        } else {
            HistoryCellPresentation::fixed(astral_tui::DisplayMode::Expanded)
        }
    }

    fn transcript_hyperlink_lines_for_presentation(
        &self,
        width: u16,
        mode: astral_tui::DisplayMode,
    ) -> Vec<HyperlinkLine> {
        let mut lines = self.invocation_lines(width);
        if mode != astral_tui::DisplayMode::Collapsed {
            let details = self.full_detail_lines(width.saturating_sub(4).max(1));
            if !details.is_empty() {
                lines.push(Line::default());
                lines.extend(prefix_lines(details, "  │ ".dim(), "  │ ".dim()));
            }
        }
        plain_hyperlink_lines(lines)
    }

    fn transcript_viewer_document(
        &self,
        width: u16,
        mode: astral_tui::BlockViewerMode,
    ) -> Option<astral_tui::BlockViewerDocument> {
        let astral_tui::BlockViewerMode::Rich = mode else {
            return None;
        };
        viewer_document_from_lines(
            format_core_tool_invocation(&self.tool, &self.arguments),
            self.full_detail_lines(width),
            width,
        )
    }
}

impl CoreToolCallCell {
    fn invocation_lines(&self, width: u16) -> Vec<Line<'static>> {
        let status = self.succeeded();
        let bullet = match status {
            Some(true) => "•".green().bold(),
            Some(false) => "•".red().bold(),
            None => "•".dim(),
        };
        let header_text = if status.is_some() {
            "Called"
        } else {
            "Calling"
        };
        let invocation_line = Line::from(format_core_tool_invocation(&self.tool, &self.arguments));
        let mut compact_spans = vec![bullet.clone(), " ".into(), header_text.bold(), " ".into()];
        let mut compact_header = Line::from(compact_spans.clone());
        let reserved = compact_header.width();
        let inline_invocation =
            invocation_line.width() <= (width as usize).saturating_sub(reserved);

        let mut lines = Vec::new();
        if inline_invocation {
            compact_header.extend(invocation_line.spans.clone());
            lines.push(compact_header);
        } else {
            compact_spans.pop();
            lines.push(Line::from(compact_spans));
            let opts = RtOptions::new((width as usize).saturating_sub(4).max(1))
                .initial_indent("".into())
                .subsequent_indent("    ".into());
            let wrapped = adaptive_wrap_line(&invocation_line, opts);
            let body_lines = wrapped.iter().map(line_to_static).collect::<Vec<_>>();
            lines.extend(prefix_lines(body_lines, "  └ ".dim(), "    ".into()));
        }

        lines
    }

    fn has_detail(&self) -> bool {
        self.error
            .as_deref()
            .is_some_and(|error| !error.trim().is_empty())
            || self
                .result
                .as_deref()
                .is_some_and(|result| !result.trim().is_empty())
    }

    fn detail_text(&self) -> Option<Cow<'_, str>> {
        if let Some(error) = self
            .error
            .as_deref()
            .filter(|error| !error.trim().is_empty())
        {
            return Some(Cow::Owned(format!("Error: {error}")));
        }
        self.result
            .as_deref()
            .filter(|result| !result.trim().is_empty())
            .map(Cow::Borrowed)
    }

    fn full_detail_lines(&self, width: u16) -> Vec<Line<'static>> {
        let Some(detail) = self.detail_text() else {
            return Vec::new();
        };
        let is_error = self.error.is_some();
        let options = RtOptions::new(usize::from(width.max(1)));
        detail
            .lines()
            .flat_map(|line| {
                let mut line = ansi_escape_line(line);
                if is_error {
                    for span in &mut line.spans {
                        span.style = span.style.patch(Style::default().red());
                    }
                }
                adaptive_wrap_line(&line, options.clone())
                    .iter()
                    .map(line_to_static)
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

impl CoreToolCallCell {
    fn detail_summary(&self, width: usize) -> Option<String> {
        if let Some(error) = &self.error {
            return Some(format_and_truncate_tool_result(
                &format!("Error: {error}"),
                TOOL_CALL_MAX_LINES,
                width,
            ));
        }
        let result = self.result.as_deref()?;
        let trimmed = result.trim();
        if trimmed.is_empty() {
            return None;
        }
        let summary = match self.tool.as_str() {
            "Read" => line_count_summary(trimmed, "line"),
            "Glob" => line_count_summary(trimmed, "file"),
            "Grep" => line_count_summary(trimmed, "match"),
            "TodoWrite" => summarize_first_line(trimmed),
            "Write"
            | "Edit"
            | "ReadTaskOutput"
            | "SendTaskInput"
            | "ListBackgroundTasks"
            | "StopBackgroundTask" => summarize_first_line(trimmed),
            _ => summarize_first_line(trimmed),
        };
        let with_duration = match self.duration_ms {
            Some(duration_ms) if duration_ms > 0 => format!("{summary} in {duration_ms}ms"),
            _ => summary,
        };
        Some(format_and_truncate_tool_result(
            &with_duration,
            TOOL_CALL_MAX_LINES,
            width,
        ))
    }
}

pub(crate) fn new_core_tool_call_cell(
    item: codex_app_server_protocol::ThreadItem,
) -> Option<CoreToolCallCell> {
    CoreToolCallCell::from_item(item)
}

fn format_core_tool_invocation(tool: &str, arguments: &serde_json::Value) -> String {
    match tool {
        "Read" => format!("{tool}({})", string_arg(arguments, &["file_path", "path"])),
        "Write" | "Edit" => format!("{tool}({})", string_arg(arguments, &["file_path", "path"])),
        "Glob" => format_search_invocation(tool, arguments, "pattern"),
        "Grep" => format_search_invocation(tool, arguments, "pattern"),
        "TodoWrite" => format!("{tool}({} todos)", array_len(arguments, "todos")),
        "ReadTaskOutput" | "SendTaskInput" | "StopBackgroundTask" => {
            format!("{tool}(task_id={})", string_arg(arguments, &["task_id"]))
        }
        "ListBackgroundTasks" => format!("{tool}()"),
        _ => format!("{tool}({})", compact_json(arguments)),
    }
}

fn format_search_invocation(tool: &str, arguments: &serde_json::Value, key: &str) -> String {
    let pattern = string_arg(arguments, &[key]);
    match optional_string_arg(arguments, "path") {
        Some(path) => format!("{tool}({pattern}, path={path})"),
        None => format!("{tool}({pattern})"),
    }
}

fn string_arg(arguments: &serde_json::Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(value) = optional_string_arg(arguments, key) {
            return value;
        }
    }
    "-".to_string()
}

fn optional_string_arg(arguments: &serde_json::Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn array_len(arguments: &serde_json::Value, key: &str) -> usize {
    arguments
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len)
}

fn compact_json(value: &serde_json::Value) -> String {
    let text = value.to_string();
    truncate_text(&text, 120)
}

fn summarize_first_line(text: &str) -> String {
    text.lines().next().unwrap_or(text).to_string()
}

fn line_count_summary(text: &str, singular: &str) -> String {
    let count = text.lines().filter(|line| !line.trim().is_empty()).count();
    if count == 0 {
        return "no results".to_string();
    }
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {singular}s")
    }
}
