use astral_tui_scrollback::EntryBlock;
use astral_tui_scrollback::EntryRenderOptions;
use astral_tui_scrollback::MarkdownLine;
use astral_tui_scrollback::ReasoningVisibility;
use astral_tui_scrollback::render_entry;
use astral_tui_scrollback::render_literal_with_metadata;
use astral_tui_scrollback::render_markdown_with_metadata;
use codex_app_server_protocol::ThreadItem;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::ConversationState;
use crate::ModalShortcut;
use crate::ModalSizing;
use crate::ModalWindowConfig;

use super::BlockViewerHost;
use super::COPY_SHORTCUT;
use super::RAW_SHORTCUT;
use super::find_entry;

impl BlockViewerHost {
    /// Paint the Grok-style modal from the latest canonical entry.
    /// Returns `false` when the entry disappeared or the terminal is too small.
    pub fn render(
        &mut self,
        buffer: &mut Buffer,
        area: Rect,
        conversation: &ConversationState,
    ) -> bool {
        if !self.reconcile(conversation) {
            return false;
        }
        let Some(entry) = find_entry(conversation, self.entry_id) else {
            return false;
        };
        let title = viewer_title(entry.item());
        let shortcuts = self.shortcuts(conversation);
        let config = ModalWindowConfig::new(&title)
            .with_shortcuts(&shortcuts)
            .with_sizing(ModalSizing::large());
        let Some(frame) = self.modal.render(buffer, area, &config) else {
            return false;
        };

        let render_width = frame.content.width.max(1);
        let document = self.document(conversation, render_width);
        let Some(document) = document.filter(|lines| !lines.is_empty()) else {
            return false;
        };

        self.row_count = document.len();
        self.content_height = frame.content.height;
        self.content_width = render_width;
        let maximum = self.maximum_scroll();
        if self.follow_bottom {
            self.scroll_offset = maximum;
        } else {
            self.scroll_offset = self.scroll_offset.min(maximum);
        }
        let visible = document
            .iter()
            .skip(self.scroll_offset)
            .take(usize::from(frame.content.height))
            .map(|line| line.line.clone())
            .collect::<Vec<Line<'static>>>();
        Paragraph::new(visible).render(frame.content, buffer);
        true
    }

    pub(super) fn copy_text(&mut self, conversation: &ConversationState) -> Option<String> {
        if !self.reconcile(conversation) {
            return None;
        }
        let lines = self.document(conversation, self.content_width.max(1))?;
        let mut text = String::new();
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                text.push_str(line.joiner_to_previous.as_str());
            }
            text.push_str(&line.line.to_string());
        }
        (!text.is_empty()).then_some(text)
    }

    fn shortcuts(&self, conversation: &ConversationState) -> Vec<ModalShortcut<'static>> {
        let mut shortcuts = vec![
            ModalShortcut::hint("Esc close"),
            ModalShortcut::action(COPY_SHORTCUT, "y copy"),
        ];
        if self.supports_raw(conversation) {
            shortcuts.push(ModalShortcut::action(RAW_SHORTCUT, "r raw"));
        }
        shortcuts.push(ModalShortcut::hint("j/k scroll"));
        shortcuts
    }

    fn document(&self, conversation: &ConversationState, width: u16) -> Option<Vec<MarkdownLine>> {
        let entry = find_entry(conversation, self.entry_id)?;
        let block = EntryBlock::from_entry(entry);
        let options = EntryRenderOptions::new(width);
        match &block {
            EntryBlock::Assistant { markdown, .. } | EntryBlock::ProposedPlan { markdown, .. } => {
                return Some(if self.raw {
                    render_literal_with_metadata(markdown, width, options.markdown_style.text)
                } else {
                    render_markdown_with_metadata(markdown, width, options.markdown_style)
                });
            }
            EntryBlock::Reasoning(reasoning) => {
                let visibility = if self.raw {
                    ReasoningVisibility::Raw
                } else {
                    ReasoningVisibility::Summary
                };
                let source = reasoning
                    .visible_parts(visibility)
                    .iter()
                    .map(|part| part.trim())
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                return (!source.is_empty()).then(|| {
                    render_markdown_with_metadata(&source, width, options.markdown_style)
                });
            }
            EntryBlock::User { .. }
            | EntryBlock::ContextCompaction(_)
            | EntryBlock::CollabAgentToolCall(_)
            | EntryBlock::DynamicToolCall(_)
            | EntryBlock::McpToolCall(_)
            | EntryBlock::WebSearch(_)
            | EntryBlock::ProtocolItem { .. } => {}
        }
        let mut display = conversation.entry_display_state(self.entry_id)?;
        display.expand(&block);
        if display.raw() != self.raw && !display.toggle_raw(&block) {
            return None;
        }
        render_entry(&block, display, options).map(astral_tui_scrollback::RenderedEntry::into_lines)
    }
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
