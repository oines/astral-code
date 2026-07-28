use codex_app_server_protocol::PatchChangeKind;
use ratatui::style::Color;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use textwrap::Options;

use crate::DisplayMode;
use crate::PresentationBlock;
use crate::SubagentAction;
use crate::SubagentAgent;
use crate::SubagentAgentStatus;
use crate::SubagentPresentation;
use crate::ToolKind;
use crate::ToolPresentation;
use crate::ToolStatus;
use crate::markdown::MarkdownStyle;
use crate::markdown::render_markdown;
use crate::todo::render_todo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    pub width: u16,
    pub mode: DisplayMode,
    pub max_output_lines: usize,
}

impl RenderOptions {
    pub fn for_mode(width: u16, mode: DisplayMode) -> Self {
        Self {
            width,
            mode,
            max_output_lines: 3,
        }
    }

    pub fn compact(width: u16) -> Self {
        Self::truncated(width)
    }

    pub fn collapsed(width: u16) -> Self {
        Self::for_mode(width, DisplayMode::Collapsed)
    }

    pub fn truncated(width: u16) -> Self {
        Self::for_mode(width, DisplayMode::Truncated)
    }

    pub fn expanded(width: u16) -> Self {
        Self::for_mode(width, DisplayMode::Expanded)
    }

    pub fn with_max_output_lines(mut self, max_output_lines: usize) -> Self {
        self.max_output_lines = max_output_lines;
        self
    }
}

pub fn render_block(block: &PresentationBlock, options: RenderOptions) -> Text<'static> {
    match block {
        PresentationBlock::User { text, attachments } => {
            render_user(text, attachments, options.width)
        }
        PresentationBlock::Assistant { text } => render_assistant(text, options.width),
        PresentationBlock::Thinking { text, running } => render_thinking(text, *running, options),
        PresentationBlock::Plan { text, running } => render_plan(text, *running, options),
        PresentationBlock::Todo(todo) => render_todo(todo, options.width),
        PresentationBlock::Tool(tool) => render_tool(tool, options),
        PresentationBlock::Subagent(subagent) => render_subagent(subagent, options),
        PresentationBlock::System { title, detail } => {
            let mut lines = vec![vec!["◆ ".dim(), title.clone().dim()].into()];
            if options.mode == DisplayMode::Expanded
                && let Some(detail) = detail
            {
                lines.extend(indented_lines(detail, options.width, "  ", true));
            }
            Text::from(lines)
        }
    }
}

fn render_subagent(subagent: &SubagentPresentation, options: RenderOptions) -> Text<'static> {
    let mut header = vec![
        status_marker(subagent.status).set_style(status_style(subagent.status)),
        "Subagent ".bold().dim(),
        subagent_summary(subagent, options.width).dim(),
    ];
    let meta = subagent_meta(subagent);
    if !meta.is_empty() {
        header.push(format!(" {meta}").dim());
    }
    let mut lines = vec![Line::from(header)];

    if options.mode == DisplayMode::Expanded
        && !matches!(subagent.action, SubagentAction::Spawn)
        && let Some(prompt) = subagent.prompt.as_deref()
    {
        lines.extend(indented_lines(prompt, options.width, "  ", true));
    }

    if options.mode == DisplayMode::Expanded || matches!(subagent.action, SubagentAction::Wait) {
        lines.extend(subagent.agents.iter().map(render_subagent_state));
    }
    Text::from(lines)
}

fn subagent_summary(subagent: &SubagentPresentation, width: u16) -> String {
    let target = match subagent.thread_ids.as_slice() {
        [] => "agent".to_string(),
        [thread_id] => thread_id.clone(),
        thread_ids => format!("{} agents", thread_ids.len()),
    };
    let prompt = subagent
        .prompt
        .as_deref()
        .map(|prompt| quoted_preview(prompt, usize::from(width).saturating_sub(24).max(12)));
    match (subagent.action, subagent.status) {
        (SubagentAction::Spawn, ToolStatus::Running) => {
            format!("running: {}", prompt.unwrap_or(target))
        }
        (SubagentAction::Spawn, ToolStatus::Success) => {
            format!("started: {}", prompt.unwrap_or(target))
        }
        (SubagentAction::Spawn, _) => format!("failed: {}", prompt.unwrap_or(target)),
        (SubagentAction::SendInput, ToolStatus::Running) => {
            format!("sending input to {target}")
        }
        (SubagentAction::SendInput, ToolStatus::Success) => {
            format!("input sent to {target}")
        }
        (SubagentAction::SendInput, _) => format!("input failed for {target}"),
        (SubagentAction::Resume, ToolStatus::Running) => format!("resuming {target}"),
        (SubagentAction::Resume, ToolStatus::Success) => format!("resumed {target}"),
        (SubagentAction::Resume, _) => format!("resume failed for {target}"),
        (SubagentAction::Wait, ToolStatus::Running) => format!("waiting for {target}"),
        (SubagentAction::Wait, ToolStatus::Success) => "wait completed".to_string(),
        (SubagentAction::Wait, _) => "wait finished with errors".to_string(),
        (SubagentAction::Close, ToolStatus::Running) => format!("closing {target}"),
        (SubagentAction::Close, ToolStatus::Success) => format!("closed {target}"),
        (SubagentAction::Close, _) => format!("close failed for {target}"),
    }
}

fn subagent_meta(subagent: &SubagentPresentation) -> String {
    match (
        subagent.model.as_deref().filter(|value| !value.is_empty()),
        subagent
            .reasoning_effort
            .as_deref()
            .filter(|value| !value.is_empty()),
    ) {
        (Some(model), Some(effort)) => format!("({model} {effort})"),
        (Some(model), None) => format!("({model})"),
        (None, Some(effort)) => format!("({effort})"),
        (None, None) => String::new(),
    }
}

fn render_subagent_state(agent: &SubagentAgent) -> Line<'static> {
    let (label, style) = match agent.status {
        SubagentAgentStatus::Pending => ("Pending", Color::Cyan),
        SubagentAgentStatus::Running => ("Running", Color::Cyan),
        SubagentAgentStatus::Interrupted => ("Interrupted", Color::Yellow),
        SubagentAgentStatus::Completed => ("Completed", Color::Green),
        SubagentAgentStatus::Failed => ("Failed", Color::Red),
        SubagentAgentStatus::Shutdown => ("Shutdown", Color::DarkGray),
        SubagentAgentStatus::Missing => ("Not found", Color::Red),
    };
    let mut spans = vec![
        "  └ ".dim(),
        agent.thread_id.clone().cyan(),
        ": ".dim(),
        Span::from(label).fg(style),
    ];
    if let Some(message) = agent
        .message
        .as_deref()
        .map(str::trim)
        .filter(|message| !message.is_empty())
    {
        spans.push(" — ".dim());
        spans.push(quoted_preview(message, 100).into());
    }
    spans.into()
}

fn quoted_preview(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = normalized.chars();
    let preview = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("“{preview}…”")
    } else {
        format!("“{preview}”")
    }
}

fn render_user(text: &str, attachments: &[String], width: u16) -> Text<'static> {
    let mut lines = prefixed_lines(text, width, "› ", "  ", false);
    lines.extend(
        attachments
            .iter()
            .flat_map(|attachment| prefixed_lines(attachment, width, "  ↳ ", "    ", true)),
    );
    Text::from(lines)
}

fn render_assistant(text: &str, width: u16) -> Text<'static> {
    Text::from(render_markdown(text, width, MarkdownStyle::default()))
}

fn render_thinking(text: &str, running: bool, options: RenderOptions) -> Text<'static> {
    let marker = if running { "◇ " } else { "◆ " };
    let mut lines = vec![vec![marker.magenta(), "Thinking".dim().italic()].into()];
    if options.mode != DisplayMode::Collapsed {
        let body = indented_lines(text, options.width, "  ", true);
        lines.extend(limit_lines(body, options));
    }
    Text::from(lines)
}

fn render_plan(text: &str, running: bool, options: RenderOptions) -> Text<'static> {
    let marker = if running { "◇ " } else { "◆ " };
    let mut lines = vec![vec![marker.cyan(), "Plan".cyan()].into()];
    if options.mode != DisplayMode::Collapsed {
        let body = indented_lines(text, options.width, "  ", false);
        lines.extend(limit_lines(body, options));
    }
    Text::from(lines)
}

fn render_tool(tool: &ToolPresentation, options: RenderOptions) -> Text<'static> {
    let marker = status_marker(tool.status);
    let mut header = vec![marker.set_style(status_style(tool.status))];
    if tool.kind == ToolKind::Mcp {
        header.extend(mcp_header(tool));
    } else {
        header.extend([
            format!("{} ", tool_label(tool.kind)).bold().dim(),
            tool.title.clone().dim(),
        ]);
    }
    if let Some(duration_ms) = tool.duration_ms {
        header.push(format!("  {}", duration_label(duration_ms)).dim());
    }
    let mut lines = vec![Line::from(header)];

    if options.mode == DisplayMode::Expanded {
        lines.extend(
            tool.details
                .iter()
                .flat_map(|detail| indented_lines(detail, options.width, "  ", true)),
        );
        for change in &tool.changes {
            let (added, removed) = diff_counts(&change.diff);
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
            lines.push(
                vec![
                    "  ".into(),
                    operation,
                    " ".dim(),
                    path.dim(),
                    format!("  +{added}").green(),
                    format!(" -{removed}").red(),
                ]
                .into(),
            );
        }
    }

    if options.mode != DisplayMode::Collapsed
        && let Some(output) = tool.output.as_deref()
    {
        let mut output_lines = indented_lines(output, options.width, "  │ ", true);
        let total = output_lines.len();
        if options.mode == DisplayMode::Truncated && total > options.max_output_lines {
            output_lines.truncate(options.max_output_lines);
            output_lines.push(
                vec![
                    "  └ ".dim(),
                    format!("{} more lines", total - options.max_output_lines).dim(),
                ]
                .into(),
            );
        }
        lines.extend(output_lines);
    }
    Text::from(lines)
}

fn limit_lines(mut lines: Vec<Line<'static>>, options: RenderOptions) -> Vec<Line<'static>> {
    if options.mode != DisplayMode::Truncated || lines.len() <= options.max_output_lines {
        return lines;
    }
    let hidden = lines.len() - options.max_output_lines;
    lines.truncate(options.max_output_lines);
    lines.push(vec!["  └ ".dim(), format!("{hidden} more lines").dim()].into());
    lines
}

fn mcp_header(tool: &ToolPresentation) -> Vec<Span<'static>> {
    let (server, action) = tool
        .name
        .split_once('/')
        .unwrap_or(("", tool.name.as_str()));
    let mut spans = Vec::new();
    if !server.is_empty() {
        spans.push(format!("{server} ").bold().dim());
    }
    spans.push(action.replace(['_', '-'], " ").dim());
    if tool.title != action {
        spans.push(" · ".dim());
        spans.push(tool.title.clone().dim());
    }
    spans
}

fn wrapped_lines(
    value: &str,
    width: u16,
    initial_indent: &str,
    subsequent_indent: &str,
    dim: bool,
) -> Vec<Line<'static>> {
    let width = usize::from(width).max(1);
    let options = Options::new(width)
        .initial_indent(initial_indent)
        .subsequent_indent(subsequent_indent)
        .word_separator(textwrap::WordSeparator::AsciiSpace)
        .word_splitter(textwrap::WordSplitter::NoHyphenation)
        .break_words(true);
    value
        .split('\n')
        .flat_map(|line| {
            if line.is_empty() {
                vec![Line::default()]
            } else {
                textwrap::wrap(line, &options)
                    .into_iter()
                    .map(|line| styled_line(line.into_owned(), dim))
                    .collect()
            }
        })
        .collect()
}

fn prefixed_lines(
    value: &str,
    width: u16,
    initial_indent: &str,
    subsequent_indent: &str,
    dim: bool,
) -> Vec<Line<'static>> {
    wrapped_lines(value, width, initial_indent, subsequent_indent, dim)
}

fn indented_lines(value: &str, width: u16, indent: &str, dim: bool) -> Vec<Line<'static>> {
    wrapped_lines(value, width, indent, indent, dim)
}

fn styled_line(value: String, dim: bool) -> Line<'static> {
    if dim {
        Line::from(value.dim())
    } else {
        Line::from(value)
    }
}

fn status_marker(status: ToolStatus) -> Span<'static> {
    match status {
        ToolStatus::Running => "◇ ".into(),
        ToolStatus::Success => "◆ ".into(),
        ToolStatus::Failed => "× ".into(),
        ToolStatus::Declined => "– ".into(),
        ToolStatus::Interrupted => "■ ".into(),
    }
}

fn status_style(status: ToolStatus) -> ratatui::style::Style {
    match status {
        ToolStatus::Running => ratatui::style::Style::default().fg(Color::Magenta),
        ToolStatus::Success => ratatui::style::Style::default().fg(Color::Green),
        ToolStatus::Failed => ratatui::style::Style::default().fg(Color::Red),
        ToolStatus::Declined => ratatui::style::Style::default().fg(Color::Yellow),
        ToolStatus::Interrupted => ratatui::style::Style::default().fg(Color::DarkGray),
    }
}

fn tool_label(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Execute | ToolKind::Background => "Run",
        ToolKind::BackgroundPoll => "Poll",
        ToolKind::BackgroundInput => "Input",
        ToolKind::BackgroundList => "List",
        ToolKind::BackgroundStop => "Stop",
        ToolKind::Read => "Read",
        ToolKind::Edit => "Edit",
        ToolKind::List => "List",
        ToolKind::Search | ToolKind::WebSearch => "Search",
        ToolKind::WebFetch => "Fetch",
        ToolKind::Mcp => "",
        ToolKind::Skill => "Skill",
        ToolKind::Collab => "Subagent",
        ToolKind::ImageView => "View",
        ToolKind::ImageGeneration => "Generate",
        ToolKind::Todo => "Todo",
        ToolKind::Other => "Use",
    }
}

fn duration_label(duration_ms: i64) -> String {
    if duration_ms < 1_000 {
        format!("{duration_ms}ms")
    } else {
        format!("{:.1}s", duration_ms as f64 / 1_000.0)
    }
}

fn diff_counts(diff: &str) -> (usize, usize) {
    diff.lines()
        .filter(|line| !line.starts_with("+++") && !line.starts_with("---"))
        .fold((0, 0), |(added, removed), line| {
            (
                added + usize::from(line.starts_with('+')),
                removed + usize::from(line.starts_with('-')),
            )
        })
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
