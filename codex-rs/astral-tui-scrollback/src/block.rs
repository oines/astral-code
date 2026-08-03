use std::borrow::Cow;

use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::UserInput;

use crate::EntryLifecycle;
use crate::LiveItem;
use crate::TranscriptEntry;
use crate::WebSearchBlock;

/// Lossless renderer-facing view of one transcript entry.
///
/// Conversation content gets a typed view so live deltas can be rendered with
/// the item they belong to. All other protocol items remain intact until their
/// exact renderer is implemented; this layer never classifies tools by name.
#[derive(Debug, PartialEq)]
pub enum EntryBlock<'a> {
    User {
        content: &'a [UserInput],
    },
    Assistant {
        markdown: Cow<'a, str>,
        running: bool,
    },
    ProposedPlan {
        markdown: Cow<'a, str>,
        running: bool,
    },
    Reasoning(ReasoningBlock<'a>),
    ContextCompaction(ContextCompactionBlock),
    WebSearch(WebSearchBlock<'a>),
    ProtocolItem {
        item: &'a ThreadItem,
        live: &'a LiveItem,
    },
}

impl<'a> EntryBlock<'a> {
    pub fn from_entry(entry: &'a TranscriptEntry) -> Self {
        Self::from_parts(entry.item(), entry.live(), entry.lifecycle())
    }

    pub(crate) fn from_parts(
        item: &'a ThreadItem,
        live: &'a LiveItem,
        lifecycle: EntryLifecycle,
    ) -> Self {
        let running = matches!(lifecycle, EntryLifecycle::Running { .. });
        match item {
            ThreadItem::UserMessage { content, .. } => Self::User { content },
            ThreadItem::AgentMessage { text, .. } => Self::Assistant {
                markdown: merge_text(text, live_agent_message(live)),
                running,
            },
            ThreadItem::Plan { text, .. } => Self::ProposedPlan {
                markdown: merge_text(text, live_plan(live)),
                running,
            },
            ThreadItem::Reasoning {
                summary, content, ..
            } => {
                let (live_summary, live_content) = live_reasoning(live);
                Self::Reasoning(ReasoningBlock {
                    summary: merge_parts(summary, live_summary),
                    content: merge_parts(content, live_content),
                    running,
                    elapsed_ms: lifecycle_elapsed_ms(lifecycle),
                })
            }
            ThreadItem::ContextCompaction { .. } => {
                Self::ContextCompaction(ContextCompactionBlock {
                    running,
                    elapsed_ms: lifecycle_elapsed_ms(lifecycle),
                })
            }
            ThreadItem::WebSearch { query, action, .. } => Self::WebSearch(
                WebSearchBlock::from_parts(query, action.as_ref(), lifecycle),
            ),
            ThreadItem::HookPrompt { .. }
            | ThreadItem::CommandExecution { .. }
            | ThreadItem::FileChange { .. }
            | ThreadItem::McpToolCall { .. }
            | ThreadItem::DynamicToolCall { .. }
            | ThreadItem::CoreToolCall { .. }
            | ThreadItem::CollabAgentToolCall { .. }
            | ThreadItem::ImageView { .. }
            | ThreadItem::ImageGeneration { .. }
            | ThreadItem::EnteredReviewMode { .. }
            | ThreadItem::ExitedReviewMode { .. } => Self::ProtocolItem { item, live },
        }
    }
}

/// Display data for one canonical app-server context-compaction item.
///
/// The protocol intentionally carries no status field: its `item/started` and
/// `item/completed` lifecycle is the status authority. Restored items represent
/// completed history and therefore never re-enter the running state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextCompactionBlock {
    running: bool,
    elapsed_ms: Option<i64>,
}

impl ContextCompactionBlock {
    pub fn running(self) -> bool {
        self.running
    }

    pub fn elapsed_ms(self) -> Option<i64> {
        self.elapsed_ms
    }
}

/// Which persisted reasoning representation the user has chosen to inspect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningVisibility {
    Summary,
    Raw,
}

/// Reasoning parts stay structured so summary and opaque/raw content never get
/// accidentally concatenated into an assistant message.
#[derive(Debug, PartialEq)]
pub struct ReasoningBlock<'a> {
    summary: Vec<Cow<'a, str>>,
    content: Vec<Cow<'a, str>>,
    running: bool,
    elapsed_ms: Option<i64>,
}

impl<'a> ReasoningBlock<'a> {
    pub fn summary(&self) -> &[Cow<'a, str>] {
        &self.summary
    }

    pub fn content(&self) -> &[Cow<'a, str>] {
        &self.content
    }

    pub fn running(&self) -> bool {
        self.running
    }

    pub fn elapsed_ms(&self) -> Option<i64> {
        self.elapsed_ms
    }

    /// Return only content that is actually available for display.
    ///
    /// Raw mode follows the classic Codex behavior: use raw reasoning when it
    /// exists, otherwise fall back to the summary. An opaque completed item
    /// therefore has no viewer body instead of opening an empty panel.
    pub fn visible_parts(&self, visibility: ReasoningVisibility) -> &[Cow<'a, str>] {
        match visibility {
            ReasoningVisibility::Raw if has_text(&self.content) => &self.content,
            ReasoningVisibility::Raw | ReasoningVisibility::Summary => &self.summary,
        }
    }

    pub fn has_visible_body(&self, visibility: ReasoningVisibility) -> bool {
        has_text(self.visible_parts(visibility))
    }
}

fn live_agent_message(live: &LiveItem) -> &str {
    match live {
        LiveItem::AgentMessage(text) => text,
        LiveItem::None
        | LiveItem::Plan(_)
        | LiveItem::Reasoning { .. }
        | LiveItem::Command { .. }
        | LiveItem::FileChange { .. } => "",
    }
}

fn live_plan(live: &LiveItem) -> &str {
    match live {
        LiveItem::Plan(text) => text,
        LiveItem::None
        | LiveItem::AgentMessage(_)
        | LiveItem::Reasoning { .. }
        | LiveItem::Command { .. }
        | LiveItem::FileChange { .. } => "",
    }
}

fn live_reasoning(live: &LiveItem) -> (&[String], &[String]) {
    match live {
        LiveItem::Reasoning { summary, content } => (summary, content),
        LiveItem::None
        | LiveItem::AgentMessage(_)
        | LiveItem::Plan(_)
        | LiveItem::Command { .. }
        | LiveItem::FileChange { .. } => (&[], &[]),
    }
}

fn merge_text<'a>(persisted: &'a str, live: &'a str) -> Cow<'a, str> {
    if live.is_empty() {
        Cow::Borrowed(persisted)
    } else if persisted.is_empty() {
        Cow::Borrowed(live)
    } else {
        Cow::Owned(format!("{persisted}{live}"))
    }
}

fn merge_parts<'a>(persisted: &'a [String], live: &'a [String]) -> Vec<Cow<'a, str>> {
    let count = persisted.len().max(live.len());
    (0..count)
        .map(|index| {
            let persisted = persisted.get(index).map_or("", String::as_str);
            let live = live.get(index).map_or("", String::as_str);
            merge_text(persisted, live)
        })
        .collect()
}

fn has_text(parts: &[Cow<'_, str>]) -> bool {
    parts.iter().any(|part| !part.trim().is_empty())
}

pub(crate) fn lifecycle_elapsed_ms(lifecycle: EntryLifecycle) -> Option<i64> {
    match lifecycle {
        EntryLifecycle::Completed {
            started_at_ms: Some(started_at_ms),
            completed_at_ms,
        } => Some(completed_at_ms.saturating_sub(started_at_ms).max(0)),
        EntryLifecycle::Restored
        | EntryLifecycle::Running { .. }
        | EntryLifecycle::Completed {
            started_at_ms: None,
            ..
        } => None,
    }
}

#[cfg(test)]
#[path = "block_tests.rs"]
mod tests;
