//! Dynamic tool-call history cells.

use super::*;

#[derive(Debug)]
pub(crate) struct DynamicToolCallCell {
    id: String,
    namespace: Option<String>,
    tool: String,
    arguments: serde_json::Value,
    status: codex_app_server_protocol::DynamicToolCallStatus,
    content_items: Option<Vec<codex_app_server_protocol::DynamicToolCallOutputContentItem>>,
    success: Option<bool>,
    duration_ms: Option<i64>,
    start_time: Instant,
    animations_enabled: bool,
}

impl DynamicToolCallCell {
    pub(crate) fn from_item(
        item: codex_app_server_protocol::ThreadItem,
        animations_enabled: bool,
    ) -> Option<Self> {
        let codex_app_server_protocol::ThreadItem::DynamicToolCall {
            id,
            namespace,
            tool,
            arguments,
            status,
            content_items,
            success,
            duration_ms,
        } = item
        else {
            return None;
        };

        if !should_render_dynamic_tool(namespace.as_deref(), &tool) {
            return None;
        }

        Some(Self {
            id,
            namespace,
            tool,
            arguments,
            status,
            content_items,
            success,
            duration_ms,
            start_time: Instant::now(),
            animations_enabled,
        })
    }

    pub(crate) fn call_id(&self) -> &str {
        &self.id
    }

    pub(crate) fn update_from_item(&mut self, item: codex_app_server_protocol::ThreadItem) {
        let Some(mut updated) = Self::from_item(item, self.animations_enabled) else {
            return;
        };
        updated.animations_enabled = self.animations_enabled;
        *self = updated;
    }

    fn succeeded(&self) -> Option<bool> {
        match self.status {
            codex_app_server_protocol::DynamicToolCallStatus::InProgress => None,
            codex_app_server_protocol::DynamicToolCallStatus::Completed => {
                Some(self.success.unwrap_or(true))
            }
            codex_app_server_protocol::DynamicToolCallStatus::Failed => Some(false),
        }
    }
}

impl HistoryCell for DynamicToolCallCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let status = self.succeeded();
        let bullet = match status {
            Some(true) => "•".dim(),
            Some(false) => "•".red().bold(),
            None => activity_indicator(
                Some(self.start_time),
                MotionMode::from_animations_enabled(self.animations_enabled),
                ReducedMotionIndicator::StaticBullet,
            )
            .unwrap_or_else(|| "•".dim()),
        };
        let header = self.header();
        let detail = self.invocation_detail();
        let text: Text<'static> = if detail.is_empty() {
            Line::from(vec![header.bold()]).into()
        } else {
            Line::from(vec![header.bold(), " ".into(), detail.into()]).into()
        };
        let mut lines = PrefixedWrappedHistoryCell::new(text, vec![bullet, " ".into()], "  ")
            .display_lines(width);

        if let Some(summary) = self.detail_summary(width as usize) {
            let summary_line = Line::from(summary.dim());
            let wrapped = adaptive_wrap_line(
                &summary_line,
                RtOptions::new((width as usize).saturating_sub(4).max(1))
                    .initial_indent("".into())
                    .subsequent_indent("    ".into()),
            );
            lines.extend(prefix_lines(
                wrapped.iter().map(line_to_static).collect::<Vec<_>>(),
                "  └ ".dim(),
                "    ".into(),
            ));
        }

        lines
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        let detail = self.invocation_detail();
        let mut lines = if detail.is_empty() {
            vec![Line::from(self.header())]
        } else {
            vec![Line::from(format!("{} {detail}", self.header()))]
        };
        if let Some(summary) = self.detail_summary(RAW_TOOL_OUTPUT_WIDTH) {
            lines.push(Line::from(summary));
        }
        lines
    }
}

impl DynamicToolCallCell {
    fn header(&self) -> &'static str {
        if self.is_web_fetch() {
            match self.succeeded() {
                Some(true) => "Fetched",
                Some(false) => "Failed to fetch",
                None => "Fetching",
            }
        } else {
            match self.succeeded() {
                Some(_) => "Called",
                None => "Calling",
            }
        }
    }

    fn invocation_detail(&self) -> String {
        if self.is_web_fetch() {
            return self
                .arguments
                .get("url")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
        }

        let name = self
            .namespace
            .as_ref()
            .map(|namespace| format!("{namespace}/{}", self.tool))
            .unwrap_or_else(|| self.tool.clone());
        format!(
            "{name}({})",
            truncate_text(&self.arguments.to_string(), 120)
        )
    }

    fn detail_summary(&self, width: usize) -> Option<String> {
        let summary = match self.succeeded() {
            Some(false) => self
                .first_text_content()?
                .lines()
                .next()
                .unwrap_or_default()
                .to_string(),
            Some(true) => match self.duration_ms {
                Some(duration_ms) if duration_ms > 0 => format!("completed in {duration_ms}ms"),
                _ => return None,
            },
            None => return None,
        };
        if summary.trim().is_empty() {
            return None;
        }
        Some(format_and_truncate_tool_result(
            &summary,
            TOOL_CALL_MAX_LINES,
            width,
        ))
    }

    fn first_text_content(&self) -> Option<&str> {
        self.content_items
            .as_ref()?
            .iter()
            .find_map(|item| match item {
                codex_app_server_protocol::DynamicToolCallOutputContentItem::InputText { text } => {
                    Some(text.as_str())
                }
                codex_app_server_protocol::DynamicToolCallOutputContentItem::InputImage {
                    ..
                } => None,
            })
    }

    fn is_web_fetch(&self) -> bool {
        matches!(self.namespace.as_deref(), Some("web")) && self.tool == "fetch"
    }
}

pub(crate) fn new_dynamic_tool_call_cell(
    item: codex_app_server_protocol::ThreadItem,
) -> Option<DynamicToolCallCell> {
    DynamicToolCallCell::from_item(item, /*animations_enabled*/ false)
}

pub(crate) fn new_active_dynamic_tool_call_cell(
    item: codex_app_server_protocol::ThreadItem,
    animations_enabled: bool,
) -> Option<DynamicToolCallCell> {
    DynamicToolCallCell::from_item(item, animations_enabled)
}

fn should_render_dynamic_tool(namespace: Option<&str>, tool: &str) -> bool {
    matches!((namespace, tool), (Some("web"), "fetch"))
}
