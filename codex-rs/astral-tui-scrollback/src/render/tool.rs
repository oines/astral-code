//! Exact protocol-item renderers.

use codex_app_server_protocol::ThreadItem;

use crate::EntryDisplayState;
use crate::LiveItem;
use crate::MarkdownLine;

use super::EntryRenderOptions;

#[path = "tool/command.rs"]
mod command;

pub(super) fn render_protocol_item(
    item: &ThreadItem,
    live: &LiveItem,
    state: EntryDisplayState,
    options: EntryRenderOptions,
) -> Option<Vec<MarkdownLine>> {
    match item {
        ThreadItem::CommandExecution { .. } => command::render(item, live, state, options),
        ThreadItem::UserMessage { .. }
        | ThreadItem::HookPrompt { .. }
        | ThreadItem::AgentMessage { .. }
        | ThreadItem::Plan { .. }
        | ThreadItem::Reasoning { .. }
        | ThreadItem::FileChange { .. }
        | ThreadItem::McpToolCall { .. }
        | ThreadItem::DynamicToolCall { .. }
        | ThreadItem::CoreToolCall { .. }
        | ThreadItem::CollabAgentToolCall { .. }
        | ThreadItem::WebSearch { .. }
        | ThreadItem::ImageView { .. }
        | ThreadItem::ImageGeneration { .. }
        | ThreadItem::EnteredReviewMode { .. }
        | ThreadItem::ExitedReviewMode { .. }
        | ThreadItem::ContextCompaction { .. } => None,
    }
}
