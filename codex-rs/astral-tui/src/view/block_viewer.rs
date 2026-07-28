// Derived from Grok Build's fullscreen block viewer at commit
// 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Modified to render Astral's provider-neutral PresentationBlock with the same
// renderer and theme roles used by the surrounding transcript.

use astral_tui_scrollback::BlockTextMode;
use astral_tui_scrollback::DisplayMode;
use astral_tui_scrollback::PresentationBlock;
use astral_tui_scrollback::render_block;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;

use crate::block_viewer::BlockViewerState;

use super::AstralTheme;
use super::ModalHeight;
use super::ScrollbackPane;
use super::ScrollbackViewport;
use super::markdown_content::render_markdown_content;
use super::render_modal_close_button;
use super::render_modal_frame_with_geometry;
use super::transcript::render_options;

pub(crate) struct BlockViewerPane<'a> {
    pub(crate) state: &'a mut BlockViewerState,
    pub(crate) block: &'a PresentationBlock,
    pub(crate) text_mode: BlockTextMode,
}

impl BlockViewerPane<'_> {
    pub(crate) fn render(self, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
        Clear.render(area, buffer);
        buffer.set_style(area, Style::default().bg(theme.bg_base));
        let title = block_title(self.block);
        let footer = block_viewer_footer(self.block);
        let Some(frame) = render_modal_frame_with_geometry(
            area,
            buffer,
            theme,
            &title,
            &footer,
            ModalHeight::FullViewport,
        ) else {
            return;
        };
        render_modal_close_button(
            buffer,
            frame.close_button,
            theme,
            self.state.close_hovered(),
        );

        let body_width = frame.content.width.saturating_sub(2).max(1);
        let body_area = Rect::new(
            frame.content.x,
            frame.content.y,
            body_width,
            frame.content.height,
        );
        let scrollbar_area = Rect::new(
            frame.content.right().saturating_sub(1),
            frame.content.y,
            1,
            frame.content.height,
        );
        let lines = match self.block {
            PresentationBlock::Assistant { text } | PresentationBlock::Thinking { text, .. } => {
                render_markdown_content(text, body_width, theme, self.text_mode, "")
                    .into_iter()
                    .map(|line| line.line)
                    .collect()
            }
            _ => {
                render_block(
                    self.block,
                    render_options(body_width, DisplayMode::Expanded, theme)
                        .with_max_output_lines(usize::MAX),
                )
                .lines
            }
        };
        self.state
            .observe_frame(frame.popup, frame.content, frame.close_button, lines.len());
        let viewport = ScrollbackViewport::from_first(
            lines.len(),
            usize::from(body_area.height),
            self.state.scroll_offset(),
        );
        ScrollbackPane {
            lines: &lines,
            viewport,
        }
        .render(body_area, scrollbar_area, buffer, theme);
    }
}

fn block_viewer_footer(block: &PresentationBlock) -> String {
    let mut hints = vec!["↑/↓ scroll".to_string()];
    if block.supports_raw() {
        hints.push("r raw".to_string());
    }
    if block.supports_copy() {
        hints.push("y copy".to_string());
    }
    if let Some(label) = block.copy_meta_label() {
        hints.push(format!("Y {label}"));
    }
    hints.push("Esc/q/Ctrl+F close".to_string());
    hints.join(" · ")
}

fn block_title(block: &PresentationBlock) -> String {
    match block {
        PresentationBlock::User { .. } => "Prompt".to_string(),
        PresentationBlock::Assistant { .. } => "Response".to_string(),
        PresentationBlock::Thinking { .. } => "Thought".to_string(),
        PresentationBlock::Plan { .. } => "Plan".to_string(),
        PresentationBlock::Todo(_) => "Todo".to_string(),
        PresentationBlock::Tool(tool) => {
            let title = tool.title.trim();
            if title.is_empty() {
                tool.name.clone()
            } else {
                title.to_string()
            }
        }
        PresentationBlock::Subagent(_) => "Subagent".to_string(),
        PresentationBlock::System { title, .. } => title.clone(),
    }
}
