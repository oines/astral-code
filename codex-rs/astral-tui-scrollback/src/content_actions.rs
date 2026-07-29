//! Content actions exposed by Grok-style scrollback selection.
//!
//! These helpers stay on the renderer-facing `PresentationBlock`; runtime and
//! app-server items remain untouched.

use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::PatchChangeKind;

use crate::MarkdownStyle;
use crate::PresentationBlock;
use crate::ToolKind;
use crate::render_markdown;

const UNWRAPPED_RENDER_WIDTH: u16 = u16::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockTextMode {
    Rendered,
    Raw,
}

impl PresentationBlock {
    pub fn supports_raw(&self) -> bool {
        matches!(self, Self::Assistant { .. } | Self::Thinking { .. })
    }

    pub fn supports_copy(&self) -> bool {
        match self {
            Self::User { .. } | Self::Assistant { .. } | Self::Thinking { .. } => true,
            Self::Tool(tool) => matches!(
                tool.kind,
                ToolKind::Execute
                    | ToolKind::Background
                    | ToolKind::Read
                    | ToolKind::Edit
                    | ToolKind::WebFetch
                    | ToolKind::WebSearch
            ),
            Self::Plan { .. } | Self::Todo(_) | Self::Subagent(_) | Self::System { .. } => false,
        }
    }

    pub fn copy_text(&self, mode: BlockTextMode) -> Option<String> {
        match self {
            Self::User { text, .. } => Some(text.clone()),
            Self::Assistant { text } | Self::Thinking { text, .. } => Some(match mode {
                BlockTextMode::Rendered => rendered_markdown_text(text),
                BlockTextMode::Raw => text.clone(),
            }),
            Self::Tool(tool)
                if matches!(
                    tool.kind,
                    ToolKind::Execute
                        | ToolKind::Background
                        | ToolKind::Read
                        | ToolKind::WebFetch
                        | ToolKind::WebSearch
                ) =>
            {
                tool.output.clone()
            }
            Self::Tool(tool) if tool.kind == ToolKind::Edit => {
                Some(file_changes_patch(&tool.changes))
            }
            Self::Plan { .. }
            | Self::Todo(_)
            | Self::Tool(_)
            | Self::Subagent(_)
            | Self::System { .. } => None,
        }
    }

    pub fn copy_meta(&self) -> Option<String> {
        let Self::Tool(tool) = self else {
            return None;
        };
        match tool.kind {
            ToolKind::Execute | ToolKind::Background => Some(tool.title.clone()),
            ToolKind::Read | ToolKind::Search | ToolKind::WebFetch | ToolKind::WebSearch => {
                Some(tool.title.clone())
            }
            ToolKind::Edit => {
                let paths = tool
                    .changes
                    .iter()
                    .map(|change| change.path.as_str())
                    .collect::<Vec<_>>();
                (!paths.is_empty()).then(|| paths.join("\n"))
            }
            ToolKind::BackgroundPoll
            | ToolKind::BackgroundInput
            | ToolKind::BackgroundList
            | ToolKind::BackgroundStop
            | ToolKind::List
            | ToolKind::Mcp
            | ToolKind::Skill
            | ToolKind::Collab
            | ToolKind::ImageView
            | ToolKind::ImageGeneration
            | ToolKind::Todo
            | ToolKind::Other => None,
        }
    }

    pub fn copy_meta_label(&self) -> Option<&'static str> {
        let Self::Tool(tool) = self else {
            return None;
        };
        match tool.kind {
            ToolKind::Execute | ToolKind::Background => Some("copy cmd"),
            ToolKind::Read | ToolKind::Edit => Some("copy path"),
            ToolKind::WebFetch => Some("copy url"),
            ToolKind::WebSearch => Some("copy query"),
            ToolKind::Search => Some("copy pattern"),
            ToolKind::BackgroundPoll
            | ToolKind::BackgroundInput
            | ToolKind::BackgroundList
            | ToolKind::BackgroundStop
            | ToolKind::List
            | ToolKind::Mcp
            | ToolKind::Skill
            | ToolKind::Collab
            | ToolKind::ImageView
            | ToolKind::ImageGeneration
            | ToolKind::Todo
            | ToolKind::Other => None,
        }
    }

    /// Plain text indexed by transcript search.
    ///
    /// Markdown-backed blocks use their rendered text so matching follows what
    /// the user sees. Structured blocks retain their meaningful labels,
    /// details, output, paths, and agent messages without involving the
    /// app-server projection.
    pub fn searchable_text(&self) -> Option<String> {
        let parts = match self {
            Self::User { text, attachments } => std::iter::once(Some(text.clone()))
                .chain(attachments.iter().cloned().map(Some))
                .collect(),
            Self::Assistant { text } | Self::Thinking { text, .. } | Self::Plan { text, .. } => {
                vec![Some(rendered_markdown_text(text))]
            }
            Self::Todo(todo) => std::iter::once(todo.explanation.clone())
                .chain(todo.items.iter().map(|item| Some(item.text.clone())))
                .collect(),
            Self::Tool(tool) => {
                let mut parts = vec![Some(tool.name.clone()), Some(tool.title.clone())];
                parts.extend(tool.details.iter().cloned().map(Some));
                parts.push(tool.output.clone());
                for change in &tool.changes {
                    parts.push(Some(change.path.to_string()));
                    parts.push(Some(change.diff.clone()));
                }
                parts
            }
            Self::Subagent(subagent) => {
                let mut parts = vec![
                    subagent.prompt.clone(),
                    subagent.model.clone(),
                    subagent.reasoning_effort.clone(),
                ];
                parts.extend(subagent.thread_ids.iter().cloned().map(Some));
                for agent in &subagent.agents {
                    parts.push(Some(agent.thread_id.clone()));
                    parts.push(agent.message.clone());
                }
                parts
            }
            Self::System { title, detail } => vec![Some(title.clone()), detail.clone()],
        };
        join_searchable(parts)
    }
}

fn rendered_markdown_text(source: &str) -> String {
    render_markdown(source, UNWRAPPED_RENDER_WIDTH, MarkdownStyle::default())
        .iter()
        .map(|line| line.to_string().trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn join_searchable(parts: Vec<Option<String>>) -> Option<String> {
    let text = parts
        .into_iter()
        .flatten()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

fn file_changes_patch(changes: &[FileUpdateChange]) -> String {
    changes
        .iter()
        .map(file_change_patch)
        .collect::<Vec<_>>()
        .join("\n")
}

fn file_change_patch(change: &FileUpdateChange) -> String {
    match &change.kind {
        PatchChangeKind::Update { move_path } => {
            if change.diff.starts_with("--- ") {
                return change.diff.clone();
            }
            let destination = move_path
                .as_ref()
                .map_or(change.path.as_str().to_string(), |path| {
                    path.display().to_string()
                });
            format!(
                "--- a/{}\n+++ b/{destination}\n{}",
                change.path, change.diff
            )
        }
        PatchChangeKind::Add => {
            source_patch(change, "/dev/null", &format!("b/{}", change.path), '+')
        }
        PatchChangeKind::Delete => {
            source_patch(change, &format!("a/{}", change.path), "/dev/null", '-')
        }
    }
}

fn source_patch(change: &FileUpdateChange, old_path: &str, new_path: &str, marker: char) -> String {
    let line_count = change.diff.lines().count();
    let range = if marker == '+' {
        format!("@@ -0,0 +1,{line_count} @@")
    } else {
        format!("@@ -1,{line_count} +0,0 @@")
    };
    let body = change
        .diff
        .lines()
        .map(|line| format!("{marker}{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("--- {old_path}\n+++ {new_path}\n{range}\n{body}\n")
}
