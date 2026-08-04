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
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
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
        self.content_area = None;
        self.scrollbar_area = None;
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

        let full_width = frame.content.width.max(1);
        let mut document = self.document(conversation, full_width);
        let needs_scrollbar = document
            .as_ref()
            .is_some_and(|lines| lines.len() > usize::from(frame.content.height))
            && full_width > 1;
        let render_width = full_width.saturating_sub(u16::from(needs_scrollbar)).max(1);
        if needs_scrollbar {
            document = self.document(conversation, render_width);
        }
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
        let content_area = Rect::new(
            frame.content.x,
            frame.content.y,
            render_width,
            frame.content.height,
        );
        self.content_area = Some(content_area);
        self.scrollbar_area = needs_scrollbar.then(|| {
            Rect::new(
                frame.content.right().saturating_sub(1),
                frame.content.y,
                1,
                frame.content.height,
            )
        });
        let visible = document
            .iter()
            .skip(self.scroll_offset)
            .take(usize::from(frame.content.height))
            .map(|line| line.line.clone())
            .collect::<Vec<Line<'static>>>();
        Paragraph::new(visible).render(content_area, buffer);
        if let Some(scrollbar) = self.scrollbar_area {
            paint_scrollbar(
                buffer,
                scrollbar,
                self.row_count,
                self.scroll_offset,
                usize::from(self.content_height),
            );
        }
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

fn paint_scrollbar(
    buffer: &mut Buffer,
    area: Rect,
    row_count: usize,
    scroll_offset: usize,
    viewport_height: usize,
) {
    if area.is_empty() || row_count <= viewport_height || viewport_height == 0 {
        return;
    }
    let track = Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::DIM);
    let thumb = Style::default().fg(Color::Gray);
    for y in area.y..area.bottom() {
        if let Some(cell) = buffer.cell_mut((area.x, y)) {
            cell.set_char('│').set_style(track);
        }
    }
    let thumb_height = viewport_height
        .saturating_mul(viewport_height)
        .div_ceil(row_count)
        .clamp(1, viewport_height);
    let travel = viewport_height.saturating_sub(thumb_height);
    let maximum = row_count.saturating_sub(viewport_height);
    let thumb_top = scroll_offset
        .min(maximum)
        .saturating_mul(travel)
        .checked_div(maximum)
        .unwrap_or(0);
    for offset in thumb_top..thumb_top.saturating_add(thumb_height) {
        if let Some(cell) = buffer.cell_mut((area.x, area.y.saturating_add(offset as u16))) {
            cell.set_char('█').set_style(thumb);
        }
    }
}
