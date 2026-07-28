// Derived from Grok Build's fullscreen block viewer at commit
// 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Modified to render Astral's provider-neutral PresentationBlock with the same
// renderer and theme roles used by the surrounding transcript.

use std::borrow::Cow;

use astral_tui_scrollback::BlockTextMode;
use astral_tui_scrollback::DisplayMode;
use astral_tui_scrollback::PresentationBlock;
use astral_tui_scrollback::render_block;
use astral_tui_scrollback::wrap_styled_line_with_metadata;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

use crate::block_viewer::BlockViewerState;
use crate::block_viewer::ViewerWrapMode;

use super::AstralTheme;
use super::ModalHeight;
use super::ScrollbackPane;
use super::ScrollbackViewport;
use super::markdown_content::render_markdown_content;
use super::render_modal_close_button;
use super::render_modal_frame_with_geometry;
use super::transcript::render_options;

const LOGICAL_LINE_WIDTH: u16 = 500;

struct ViewerItem {
    line: Line<'static>,
    plain: String,
    background: Option<Color>,
}

struct ViewerRow {
    line: Line<'static>,
    item: usize,
    plain: String,
    background: Option<Color>,
}

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

        let query_bar_visible = self.state.query_bar_visible();
        let body_width = frame.content.width.saturating_sub(2).max(1);
        let body_height = frame
            .content
            .height
            .saturating_sub(u16::from(query_bar_visible));
        let body_area = Rect::new(frame.content.x, frame.content.y, body_width, body_height);
        let scrollbar_area = Rect::new(
            frame.content.right().saturating_sub(1),
            frame.content.y,
            1,
            body_height,
        );
        let items = render_viewer_items(self.block, theme, self.text_mode);
        let rows = render_viewer_rows(&items, body_width, self.state.wrap_mode());
        let logical_lines = items.iter().map(|item| item.plain.clone()).collect();
        let row_items = rows.iter().map(|row| row.item).collect();
        let rendered_rows = rows.iter().map(|row| row.plain.clone()).collect();
        self.state.observe_frame(
            frame.popup,
            body_area,
            frame.close_button,
            logical_lines,
            row_items,
            rendered_rows,
        );
        let rows = self
            .state
            .visible_row_indices()
            .iter()
            .filter_map(|index| rows.get(*index))
            .collect::<Vec<_>>();
        let lines = rows.iter().map(|row| row.line.clone()).collect::<Vec<_>>();
        let viewport = ScrollbackViewport::from_first(
            lines.len(),
            usize::from(body_area.height),
            self.state.scroll_offset(),
        );
        render_row_backgrounds(&rows, body_area, viewport, buffer);
        ScrollbackPane {
            lines: &lines,
            viewport,
        }
        .render(body_area, scrollbar_area, buffer, theme);
        render_visual_selection(self.state, body_area, viewport, buffer, theme);
        for selected in viewport.first_visible_line..viewport.end_visible_line {
            if !self.state.row_is_selected(selected) {
                continue;
            }
            let row = body_area.y.saturating_add(
                u16::try_from(selected.saturating_sub(viewport.first_visible_line))
                    .unwrap_or(u16::MAX),
            );
            buffer.set_style(
                Rect::new(body_area.x, row, body_area.width, 1),
                selection_style(theme).add_modifier(Modifier::BOLD),
            );
        }
        render_matches(self.state, body_area, viewport, buffer, theme);
        if query_bar_visible {
            render_query_bar(
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

fn render_viewer_items(
    block: &PresentationBlock,
    theme: AstralTheme,
    text_mode: BlockTextMode,
) -> Vec<ViewerItem> {
    let lines = match block {
        PresentationBlock::Assistant { text } | PresentationBlock::Thinking { text, .. } => {
            render_markdown_content(text, LOGICAL_LINE_WIDTH, theme, text_mode, "")
                .into_iter()
                .map(|line| line.line)
                .collect()
        }
        _ => {
            render_block(
                block,
                render_options(LOGICAL_LINE_WIDTH, DisplayMode::Expanded, theme)
                    .with_max_output_lines(usize::MAX),
            )
            .lines
        }
    };
    lines.into_iter().map(viewer_item).collect()
}

fn viewer_item(mut line: Line<'static>) -> ViewerItem {
    let background = line
        .style
        .bg
        .or_else(|| line.spans.iter().find_map(|span| span.style.bg));
    while let Some(last) = line.spans.last_mut() {
        let trimmed = last.content.trim_end_matches(char::is_whitespace);
        if trimmed.is_empty() {
            line.spans.pop();
        } else {
            if trimmed.len() != last.content.len() {
                last.content = Cow::Owned(trimmed.to_string());
            }
            break;
        }
    }
    let plain = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    ViewerItem {
        line,
        plain,
        background,
    }
}

fn render_viewer_rows(
    items: &[ViewerItem],
    width: u16,
    wrap_mode: ViewerWrapMode,
) -> Vec<ViewerRow> {
    items
        .iter()
        .enumerate()
        .flat_map(|(item, logical)| {
            let lines = match wrap_mode {
                ViewerWrapMode::Wrap => wrap_styled_line_with_metadata(&logical.line, width)
                    .into_iter()
                    .map(|line| line.line)
                    .collect(),
                ViewerWrapMode::NoWrap => vec![logical.line.clone()],
            };
            lines.into_iter().map(move |line| ViewerRow {
                plain: line
                    .spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect(),
                line,
                item,
                background: logical.background,
            })
        })
        .collect()
}

fn render_row_backgrounds(
    rows: &[&ViewerRow],
    area: Rect,
    viewport: ScrollbackViewport,
    buffer: &mut Buffer,
) {
    for row in viewport.first_visible_line..viewport.end_visible_line {
        let Some(background) = rows.get(row).and_then(|row| row.background) else {
            continue;
        };
        let y = area.y.saturating_add(
            u16::try_from(row.saturating_sub(viewport.first_visible_line)).unwrap_or(u16::MAX),
        );
        buffer.set_style(
            Rect::new(area.x, y, area.width, 1),
            Style::default().bg(background),
        );
    }
}

fn block_viewer_footer(block: &PresentationBlock) -> String {
    let mut hints = vec![
        "Esc close".to_string(),
        "/ search".to_string(),
        "f filter".to_string(),
        "v select".to_string(),
        "w wrap".to_string(),
    ];
    if block.supports_raw() {
        hints.push("r raw".to_string());
    }
    if let Some(label) = block.copy_meta_label() {
        hints.push(format!("Y {label}"));
    }
    hints.join(" · ")
}

fn render_visual_selection(
    state: &BlockViewerState,
    area: Rect,
    viewport: ScrollbackViewport,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    for line in viewport.first_visible_line..viewport.end_visible_line {
        if !state.row_is_in_visual_selection(line) {
            continue;
        }
        let row = area.y.saturating_add(
            u16::try_from(line.saturating_sub(viewport.first_visible_line)).unwrap_or(u16::MAX),
        );
        buffer.set_style(
            Rect::new(area.x, row, area.width, 1),
            selection_style(theme),
        );
    }
}

fn selection_style(theme: AstralTheme) -> Style {
    if theme.panel_selected == Color::Reset {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().bg(theme.panel_selected)
    }
}

fn render_matches(
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
        let Some(text) = state.rendered_row(line) else {
            continue;
        };
        for range in state.match_ranges(line) {
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

fn render_query_bar(state: &BlockViewerState, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
    if area.width == 0 {
        return;
    }
    buffer.set_style(area, Style::default().bg(theme.panel_background));
    if state.query_input_active() {
        let label = if state.query_is_error() {
            format!("{}! ", state.query_label())
        } else {
            format!("{}: ", state.query_label())
        };
        buffer.set_string(
            area.x,
            area.y,
            &label,
            Style::default()
                .fg(if state.query_is_error() {
                    theme.accent_error
                } else {
                    theme.prompt_border_active
                })
                .bg(theme.panel_background)
                .add_modifier(Modifier::BOLD),
        );
        let label_width = UnicodeWidthStr::width(label.as_str());
        let available = usize::from(area.width).saturating_sub(label_width);
        let query = state.query_text();
        let (visible, cursor_width) = query_input_window(query, state.query_cursor(), available);
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
            "[{}: {} · {} matches]",
            state.query_label(),
            state.query_text(),
            state.match_count()
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

fn query_input_window(query: &str, cursor: usize, width: usize) -> (String, usize) {
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
