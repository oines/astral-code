use astral_tui_scrollback::EntryBlock;
use astral_tui_scrollback::EntryRenderOptions;
use astral_tui_scrollback::TranscriptEntry;
use astral_tui_scrollback::TranscriptEntryId;
use astral_tui_scrollback::render_entry;
use astral_tui_scrollback::render_literal_with_metadata;
use astral_tui_scrollback::render_markdown_with_metadata;
use codex_app_server_protocol::ThreadItem;

use crate::ConversationState;

use super::BlockViewerDocument;
use super::BlockViewerMode;
use super::BlockViewerSource;

impl BlockViewerSource for ConversationState {
    fn block_viewer_document(
        &self,
        entry_id: TranscriptEntryId,
        width: u16,
        mode: BlockViewerMode,
    ) -> Option<BlockViewerDocument> {
        let entry = find_entry(self, entry_id)?;
        let block = EntryBlock::from_entry(entry);
        let options = EntryRenderOptions::new(width);
        let lines = match &block {
            EntryBlock::Assistant { markdown, .. } | EntryBlock::ProposedPlan { markdown, .. } => {
                match mode {
                    BlockViewerMode::Rich => {
                        render_markdown_with_metadata(markdown, width, options.markdown_style)
                    }
                    BlockViewerMode::Raw => {
                        render_literal_with_metadata(markdown, width, options.markdown_style.text)
                    }
                }
            }
            EntryBlock::Reasoning(reasoning) => {
                let parts = match mode {
                    BlockViewerMode::Rich => reasoning.summary(),
                    BlockViewerMode::Raw => reasoning.content(),
                };
                let source = parts
                    .iter()
                    .map(|part| part.trim())
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                if source.is_empty() {
                    return None;
                }
                render_markdown_with_metadata(&source, width, options.markdown_style)
            }
            // Unlike Grok's native search block, the app-server item does not
            // carry result content, citations, or an error body. Opening a
            // modal that only repeats the compact header is misleading.
            EntryBlock::WebSearch(_) => return None,
            EntryBlock::User { .. }
            | EntryBlock::ContextCompaction(_)
            | EntryBlock::CollabAgentToolCall(_)
            | EntryBlock::DynamicToolCall(_)
            | EntryBlock::McpToolCall(_)
            | EntryBlock::ProtocolItem { .. } => {
                let mut display = self.entry_display_state(entry_id)?;
                display.expand(&block);
                let wants_raw = mode == BlockViewerMode::Raw;
                if display.raw() != wants_raw && !display.toggle_raw(&block) {
                    return None;
                }
                render_entry(&block, display, options)?.into_lines()
            }
        };
        BlockViewerDocument::new(viewer_title(entry.item()), lines)
    }

    fn block_viewer_default_mode(&self, entry_id: TranscriptEntryId) -> BlockViewerMode {
        if self
            .entry_display_state(entry_id)
            .is_some_and(astral_tui_scrollback::EntryDisplayState::raw)
        {
            BlockViewerMode::Raw
        } else {
            BlockViewerMode::Rich
        }
    }

    fn block_viewer_follow_bottom(&self, entry_id: TranscriptEntryId) -> bool {
        find_entry(self, entry_id).is_some_and(|entry| {
            matches!(
                entry.lifecycle(),
                astral_tui_scrollback::EntryLifecycle::Running { .. }
            )
        })
    }
}

fn find_entry(
    conversation: &ConversationState,
    entry_id: TranscriptEntryId,
) -> Option<&TranscriptEntry> {
    conversation
        .transcript()
        .turns()
        .iter()
        .flat_map(astral_tui_scrollback::TranscriptTurn::entries)
        .find(|entry| entry.id() == entry_id)
}

fn viewer_title(item: &ThreadItem) -> String {
    match item {
        ThreadItem::UserMessage { .. } => "Prompt".to_string(),
        ThreadItem::HookPrompt { .. } => "Hook".to_string(),
        ThreadItem::AgentMessage { .. } => "Response".to_string(),
        ThreadItem::Plan { .. } => "Proposed Plan".to_string(),
        ThreadItem::Reasoning { .. } => "Thought".to_string(),
        ThreadItem::CommandExecution { .. } => "Command".to_string(),
        ThreadItem::FileChange { .. } => "File changes".to_string(),
        ThreadItem::McpToolCall { .. } => "MCP tool".to_string(),
        ThreadItem::DynamicToolCall { .. } => "Tool".to_string(),
        ThreadItem::CoreToolCall { tool, .. } => tool.clone(),
        ThreadItem::CollabAgentToolCall { .. } => "Subagent".to_string(),
        ThreadItem::WebSearch { .. } => "Web Search".to_string(),
        ThreadItem::ImageView { .. } => "Image".to_string(),
        ThreadItem::ImageGeneration { .. } => "Generated image".to_string(),
        ThreadItem::EnteredReviewMode { .. } | ThreadItem::ExitedReviewMode { .. } => {
            "Review".to_string()
        }
        ThreadItem::ContextCompaction { .. } => "Compaction".to_string(),
    }
}
