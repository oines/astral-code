//! Exact protocol-item renderers.

use codex_app_server_protocol::ThreadItem;

use crate::EntryDisplayState;
use crate::LiveItem;
use crate::MarkdownLine;

use super::EntryRenderOptions;

#[path = "tool/command.rs"]
mod command;
#[path = "tool/file_change.rs"]
mod file_change;
#[path = "tool/read.rs"]
mod read;
#[path = "tool/search.rs"]
mod search;

pub(super) fn render_protocol_item(
    item: &ThreadItem,
    live: &LiveItem,
    state: EntryDisplayState,
    options: EntryRenderOptions,
) -> Option<Vec<MarkdownLine>> {
    match item {
        ThreadItem::CommandExecution { .. } => command::render(item, live, state, options),
        ThreadItem::FileChange { .. } => file_change::render(item, live, state, options),
        ThreadItem::CoreToolCall { .. } => {
            read::render(item, state, options).or_else(|| search::render(item, state, options))
        }
        ThreadItem::UserMessage { .. }
        | ThreadItem::HookPrompt { .. }
        | ThreadItem::AgentMessage { .. }
        | ThreadItem::Plan { .. }
        | ThreadItem::Reasoning { .. }
        | ThreadItem::McpToolCall { .. }
        | ThreadItem::DynamicToolCall { .. }
        | ThreadItem::CollabAgentToolCall { .. }
        | ThreadItem::WebSearch { .. }
        | ThreadItem::ImageView { .. }
        | ThreadItem::ImageGeneration { .. }
        | ThreadItem::EnteredReviewMode { .. }
        | ThreadItem::ExitedReviewMode { .. }
        | ThreadItem::ContextCompaction { .. } => None,
    }
}
