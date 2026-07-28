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
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

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

        let search_bar_visible = self.state.search_bar_visible();
        let body_width = frame.content.width.saturating_sub(2).max(1);
        let body_height = frame
            .content
            .height
            .saturating_sub(u16::from(search_bar_visible));
        let body_area = Rect::new(frame.content.x, frame.content.y, body_width, body_height);
        let scrollbar_area = Rect::new(
            frame.content.right().saturating_sub(1),
            frame.content.y,
            1,
            body_height,
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
        let plain_lines = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect();
        self.state
            .observe_frame(frame.popup, body_area, frame.close_button, plain_lines);
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
        if let Some(selected) = self.state.selected_line()
            && viewport.first_visible_line <= selected
            && selected < viewport.end_visible_line
        {
            let row = body_area.y.saturating_add(
                u16::try_from(selected.saturating_sub(viewport.first_visible_line))
                    .unwrap_or(u16::MAX),
            );
            let selection_style = if theme.panel_selected == Color::Reset {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default().bg(theme.panel_selected)
            };
            buffer.set_style(
                Rect::new(body_area.x, row, body_area.width, 1),
                selection_style,
            );
        }
        render_search_matches(self.state, body_area, viewport, buffer, theme);
        if search_bar_visible {
            render_search_bar(
                self.state,
                Rect::new(
                    frame.content.x,
                    frame.content.bottom().saturating_sub(1),
                    frame.content.width,
                    1,
                ),
                buffer,
                theme,
            );
        }
    }
}

fn block_viewer_footer(block: &PresentationBlock) -> String {
    let mut hints = vec![
        "Esc close".to_string(),
        "/ search".to_string(),
        "n/N match".to_string(),
        "j/k navigate".to_string(),
    ];
    if block.supports_raw() {
        hints.push("r raw".to_string());
    }
    if block.supports_copy() {
        hints.push("y copy".to_string());
    }
    if let Some(label) = block.copy_meta_label() {
        hints.push(format!("Y {label}"));
    }
    hints.join(" · ")
}

fn render_search_matches(
    state: &BlockViewerState,
    area: Rect,
    viewport: ScrollbackViewport,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    let style = Style::default()
        .fg(theme.accent_running)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    for line in viewport.first_visible_line..viewport.end_visible_line {
        let row = area.y.saturating_add(
            u16::try_from(line.saturating_sub(viewport.first_visible_line)).unwrap_or(u16::MAX),
        );
        let Some(text) = state.rendered_line(line) else {
            continue;
        };
        for range in state.search_match_ranges(line) {
            let start = UnicodeWidthStr::width(&text[..range.start]);
            let width = UnicodeWidthStr::width(&text[range.clone()]);
            if width == 0 || start >= usize::from(area.width) {
                continue;
            }
            buffer.set_style(
                Rect::new(
                    area.x
                        .saturating_add(u16::try_from(start).unwrap_or(u16::MAX)),
                    row,
                    u16::try_from(width).unwrap_or(u16::MAX).min(
                        area.width
                            .saturating_sub(u16::try_from(start).unwrap_or(u16::MAX)),
                    ),
                    1,
                ),
                style,
            );
        }
    }
}

fn render_search_bar(
    state: &BlockViewerState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    if area.width == 0 {
        return;
    }
    buffer.set_style(area, Style::default().bg(theme.panel_background));
    if state.search_input_active() {
        let label = if state.search_is_error() {
            "search! "
        } else {
            "search: "
        };
        buffer.set_string(
            area.x,
            area.y,
            label,
            Style::default()
                .fg(if state.search_is_error() {
                    theme.accent_error
                } else {
                    theme.prompt_border_active
                })
                .bg(theme.panel_background)
                .add_modifier(Modifier::BOLD),
        );
        let label_width = UnicodeWidthStr::width(label);
        let available = usize::from(area.width).saturating_sub(label_width);
        let query = state.search_query();
        let (visible, cursor_width) = search_input_window(query, state.search_cursor(), available);
        buffer.set_stringn(
            area.x
                .saturating_add(u16::try_from(label_width).unwrap_or(u16::MAX)),
            area.y,
            &visible,
            available,
            Style::default()
                .fg(theme.text_primary)
                .bg(theme.panel_background),
        );
        if available > 0 {
            buffer[(
                area.x
                    .saturating_add(u16::try_from(label_width).unwrap_or(u16::MAX))
                    .saturating_add(u16::try_from(cursor_width).unwrap_or(u16::MAX)),
                area.y,
            )]
                .modifier
                .insert(Modifier::REVERSED);
        }
    } else {
        let status = format!(
            "[search: {} · {} matches]",
            state.search_query(),
            state.search_match_count()
        );
        let width = UnicodeWidthStr::width(status.as_str());
        let x = area
            .right()
            .saturating_sub(u16::try_from(width).unwrap_or(u16::MAX));
        buffer.set_stringn(
            x.max(area.x),
            area.y,
            status,
            usize::from(area.width),
            Style::default()
                .fg(theme.gray)
                .bg(theme.panel_background)
                .add_modifier(Modifier::DIM),
        );
    }
}

fn search_input_window(query: &str, cursor: usize, width: usize) -> (String, usize) {
    if width == 0 {
        return (String::new(), 0);
    }
    let mut start = cursor;
    let cursor_limit = width.saturating_sub(1);
    while start > 0 {
        let previous = query[..start]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index);
        if UnicodeWidthStr::width(&query[previous..cursor]) > cursor_limit {
            break;
        }
        start = previous;
    }
    let cursor_column = UnicodeWidthStr::width(&query[start..cursor]).min(cursor_limit);
    let mut end = start;
    for (offset, character) in query[start..].char_indices() {
        let candidate = start + offset + character.len_utf8();
        if UnicodeWidthStr::width(&query[start..candidate]) > width {
            break;
        }
        end = candidate;
    }
    (query[start..end].to_string(), cursor_column)
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
