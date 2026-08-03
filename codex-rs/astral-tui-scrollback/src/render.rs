//! Shared rendering for source-preserving conversation entries.

use codex_app_server_protocol::UserInput;
use ratatui::style::Color;
use ratatui::style::Stylize;
use ratatui::text::Line;
use unicode_width::UnicodeWidthStr;

use crate::DisplayMode;
use crate::EntryBlock;
use crate::EntryDisplayState;
use crate::LineJoiner;
use crate::MarkdownLine;
use crate::MarkdownStyle;
use crate::ReasoningBlock;
use crate::ReasoningVisibility;
use crate::render_literal_with_metadata;
use crate::render_markdown_with_metadata;

#[path = "render/tool.rs"]
mod tool;
#[path = "render/verb_group.rs"]
mod verb_group;
#[path = "render/web_search.rs"]
mod web_search;

use tool::render_protocol_item;
use verb_group::render_header as render_verb_group_header_lines;

const USER_COLLAPSED_MAX_LINES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryRenderOptions {
    pub width: u16,
    pub max_truncated_lines: usize,
    pub markdown_style: MarkdownStyle,
    pub diff_style: DiffStyle,
}

/// Palette roles for structured file changes. The active TUI can replace the
/// terminal-safe default with its day/night theme without changing diff
/// semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffStyle {
    pub path: Color,
    pub gutter: Color,
    pub insert_foreground: Color,
    pub delete_foreground: Color,
    pub insert_background: Option<Color>,
    pub delete_background: Option<Color>,
    pub equal_foreground: Color,
    pub syntax_theme: crate::MarkdownSyntaxTheme,
}

impl Default for DiffStyle {
    fn default() -> Self {
        Self {
            path: Color::Cyan,
            gutter: Color::DarkGray,
            insert_foreground: Color::Green,
            delete_foreground: Color::Red,
            insert_background: None,
            delete_background: None,
            equal_foreground: Color::DarkGray,
            syntax_theme: crate::MarkdownSyntaxTheme::Terminal,
        }
    }
}

impl EntryRenderOptions {
    pub fn new(width: u16) -> Self {
        Self {
            width: width.max(1),
            max_truncated_lines: 3,
            markdown_style: MarkdownStyle::default(),
            diff_style: DiffStyle::default(),
        }
    }

    pub fn with_max_truncated_lines(mut self, max_truncated_lines: usize) -> Self {
        self.max_truncated_lines = max_truncated_lines;
        self
    }

    pub fn with_markdown_style(mut self, markdown_style: MarkdownStyle) -> Self {
        self.markdown_style = markdown_style;
        self
    }

    pub fn with_diff_style(mut self, diff_style: DiffStyle) -> Self {
        self.diff_style = diff_style;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedEntry {
    lines: Vec<MarkdownLine>,
}

impl RenderedEntry {
    pub fn lines(&self) -> &[MarkdownLine] {
        &self.lines
    }

    pub fn into_lines(self) -> Vec<MarkdownLine> {
        self.lines
    }
}

/// Render one typed transcript entry. Unsupported protocol items return
/// `None`; tool renderers match exact protocol variants and never guess by
/// tool name.
pub fn render_entry(
    block: &EntryBlock<'_>,
    state: EntryDisplayState,
    options: EntryRenderOptions,
) -> Option<RenderedEntry> {
    let lines = match block {
        EntryBlock::User { content } => render_user(content, state.mode(), options.width),
        EntryBlock::Assistant { markdown, .. } => {
            render_markdown_source(markdown, state.raw(), options)
        }
        EntryBlock::ProposedPlan { markdown, .. } => render_plan(markdown, state.raw(), options),
        EntryBlock::Reasoning(reasoning) => render_reasoning(reasoning, state, options),
        EntryBlock::ContextCompaction(compaction) => render_context_compaction(*compaction),
        EntryBlock::WebSearch(search) => web_search::render(*search, state, options),
        EntryBlock::ProtocolItem { item, live } => {
            render_protocol_item(item, live, state, options)?
        }
    };
    Some(RenderedEntry { lines })
}

fn render_context_compaction(compaction: crate::ContextCompactionBlock) -> Vec<MarkdownLine> {
    let line = if compaction.running() {
        vec!["◇ ".magenta(), "Compacting context…".bold()].into()
    } else if let Some(elapsed_ms) = compaction.elapsed_ms() {
        vec![
            "◆ ".dim(),
            "Context compacted".bold(),
            format!(" in {}", format_elapsed(elapsed_ms)).dim(),
        ]
        .into()
    } else {
        vec!["◆ ".dim(), "Context compacted".bold()].into()
    };
    vec![MarkdownLine {
        line,
        joiner_to_previous: LineJoiner::HardBreak,
        links: Vec::new(),
    }]
}

/// Render the synthetic header for one Grok-style verb group.
pub fn render_verb_group_header(
    group: &crate::VerbGroupSpan,
    options: EntryRenderOptions,
) -> RenderedEntry {
    RenderedEntry {
        lines: render_verb_group_header_lines(group, options.width),
    }
}

fn render_user(content: &[UserInput], mode: DisplayMode, width: u16) -> Vec<MarkdownLine> {
    let mut text = String::new();
    let mut image_count = 0usize;
    for input in content {
        match input {
            UserInput::Text { text: part, .. } => text.push_str(part),
            UserInput::Image { .. } | UserInput::LocalImage { .. } => {
                image_count = image_count.saturating_add(1);
            }
            UserInput::Skill { .. } | UserInput::Mention { .. } => {}
        }
    }

    let body_width = width.saturating_sub(2).max(1);
    let mut lines = render_literal_with_metadata(&text, body_width, Default::default());
    prefix_lines(&mut lines, Line::from("› ".bold()), Line::from("  "));
    if mode != DisplayMode::Expanded && lines.len() > USER_COLLAPSED_MAX_LINES {
        lines.truncate(USER_COLLAPSED_MAX_LINES);
        if let Some(last) = lines.last_mut() {
            truncate_with_ellipsis(last, width);
        }
    }
    lines.extend((1..=image_count).map(|index| MarkdownLine {
        line: vec!["  ↳ ".dim(), format!("[Image #{index}]").dim()].into(),
        joiner_to_previous: LineJoiner::HardBreak,
        links: Vec::new(),
    }));
    lines
}

fn render_markdown_source(
    markdown: &str,
    raw: bool,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    if raw {
        render_literal_with_metadata(markdown, options.width, options.markdown_style.text)
    } else {
        render_markdown_with_metadata(markdown, options.width, options.markdown_style)
    }
}

fn render_plan(markdown: &str, raw: bool, options: EntryRenderOptions) -> Vec<MarkdownLine> {
    let mut lines = vec![MarkdownLine {
        line: vec!["• ".dim(), "Proposed Plan".bold()].into(),
        joiner_to_previous: LineJoiner::HardBreak,
        links: Vec::new(),
    }];
    if markdown.trim().is_empty() {
        return lines;
    }
    lines.push(blank_line());
    let mut body = if raw {
        render_literal_with_metadata(
            markdown,
            options.width.saturating_sub(2),
            options.markdown_style.text,
        )
    } else {
        render_markdown_with_metadata(
            markdown,
            options.width.saturating_sub(2),
            options.markdown_style,
        )
    };
    prefix_lines(&mut body, Line::from("  "), Line::from("  "));
    lines.extend(body);
    lines
}

fn render_reasoning(
    reasoning: &ReasoningBlock<'_>,
    state: EntryDisplayState,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    let mut lines = vec![reasoning_header(reasoning)];
    let visibility = if state.raw() {
        ReasoningVisibility::Raw
    } else {
        ReasoningVisibility::Summary
    };
    if state.mode() == DisplayMode::Collapsed || !reasoning.has_visible_body(visibility) {
        return lines;
    }

    let source = reasoning
        .visible_parts(visibility)
        .iter()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut body = render_markdown_with_metadata(
        &source,
        options.width.saturating_sub(2),
        options.markdown_style,
    );
    for line in &mut body {
        line.line = line.line.clone().dim();
    }
    if state.mode() == DisplayMode::Truncated && body.len() > options.max_truncated_lines {
        let keep_from = body.len().saturating_sub(options.max_truncated_lines);
        body.drain(..keep_from);
        body.insert(
            0,
            MarkdownLine {
                line: Line::from("…".dim()),
                joiner_to_previous: LineJoiner::HardBreak,
                links: Vec::new(),
            },
        );
    }
    prefix_lines(&mut body, Line::from("  "), Line::from("  "));
    lines.push(blank_line());
    lines.extend(body);
    lines
}

fn reasoning_header(reasoning: &ReasoningBlock<'_>) -> MarkdownLine {
    let line = if reasoning.running() {
        vec!["◇ ".magenta(), "Thinking…".bold()].into()
    } else if let Some(elapsed_ms) = reasoning.elapsed_ms() {
        vec![
            "◆ ".dim(),
            "Thought".bold(),
            format!(" for {}", format_elapsed(elapsed_ms)).dim(),
        ]
        .into()
    } else {
        vec!["◆ ".dim(), "Thought".bold()].into()
    };
    MarkdownLine {
        line,
        joiner_to_previous: LineJoiner::HardBreak,
        links: Vec::new(),
    }
}

pub(super) fn format_elapsed(elapsed_ms: i64) -> String {
    let seconds = elapsed_ms.max(0) as f64 / 1000.0;
    if seconds < 60.0 {
        format!("{seconds:.1}s")
    } else {
        let minutes = (seconds / 60.0).floor() as u64;
        let remaining = seconds - minutes as f64 * 60.0;
        format!("{minutes}m{remaining:.0}s")
    }
}

pub(super) fn prefix_lines(
    lines: &mut [MarkdownLine],
    initial: Line<'static>,
    subsequent: Line<'static>,
) {
    for (index, line) in lines.iter_mut().enumerate() {
        let prefix = if index == 0 {
            initial.clone()
        } else {
            subsequent.clone()
        };
        let prefix_width = u16::try_from(prefix.width()).unwrap_or(u16::MAX);
        for link in &mut line.links {
            link.columns = link.columns.start.saturating_add(prefix_width)
                ..link.columns.end.saturating_add(prefix_width);
        }
        line.line.spans.splice(0..0, prefix.spans);
    }
}

pub(super) fn truncate_with_ellipsis(line: &mut MarkdownLine, width: u16) {
    const ELLIPSIS: &str = " …";
    let available = usize::from(width).saturating_sub(UnicodeWidthStr::width(ELLIPSIS));
    let mut text = line.line.to_string();
    while UnicodeWidthStr::width(text.as_str()) > available {
        if text.pop().is_none() {
            break;
        }
    }
    line.line = format!("{}{ELLIPSIS}", text.trim_end()).into();
    line.links.clear();
}

fn blank_line() -> MarkdownLine {
    MarkdownLine {
        line: Line::default(),
        joiner_to_previous: LineJoiner::HardBreak,
        links: Vec::new(),
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
