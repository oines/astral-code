use astral_tui_scrollback::render_literal_with_metadata;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::actions;
use crate::actions::When;
use crate::modal::ModalRowHit;
use crate::shortcuts::ShortcutHelpState;
use crate::shortcuts::ShortcutRow;

use super::AstralTheme;
use super::modal::ModalSizing;
use super::modal::render_modal_frame_with_sizing;
use super::modal_choice_style;
use super::render_modal_close_button;

pub(crate) struct ShortcutHelp<'a> {
    pub(crate) state: &'a mut ShortcutHelpState,
}

impl ShortcutHelp<'_> {
    pub(crate) fn render(self, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
        let footer = if self.state.detail().is_some() {
            "↑/↓ scroll · Esc back"
        } else if self.state.search_active() {
            "type · ↑/↓ nav · Enter open · Esc clear"
        } else {
            "↑/↓ nav · e expand · Enter details · Esc close"
        };
        let Some(frame) = render_modal_frame_with_sizing(
            area,
            buffer,
            theme,
            "Keyboard Shortcuts",
            footer,
            ModalSizing::shortcuts(),
        ) else {
            return;
        };
        render_modal_close_button(
            buffer,
            frame.close_button,
            theme,
            self.state.pointer.close_hovered(),
        );
        if let Some(definition) = self.state.detail() {
            render_detail(self.state, definition, frame.content, buffer, theme);
            self.state
                .pointer
                .observe_frame(frame.popup, frame.close_button, Vec::new());
        } else {
            let rows = render_browser(self.state, frame.content, buffer, theme);
            self.state
                .pointer
                .observe_frame(frame.popup, frame.close_button, rows);
        }
    }
}

fn render_browser(
    state: &mut ShortcutHelpState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) -> Vec<ModalRowHit> {
    if area.is_empty() {
        return Vec::new();
    }
    let search_style = Style::default()
        .fg(if state.search_active() {
            theme.text_primary
        } else {
            theme.gray
        })
        .bg(theme.bg_base);
    let filter = if state.hide_inactive() {
        " · active only"
    } else {
        ""
    };
    let query = if state.query().is_empty() && !state.search_active() {
        "/ to filter"
    } else {
        state.query()
    };
    buffer.set_stringn(
        area.x,
        area.y,
        format!("Search: {query}{filter}"),
        usize::from(area.width),
        search_style,
    );
    if area.height > 1 {
        buffer.set_stringn(
            area.x,
            area.y.saturating_add(1),
            "─".repeat(usize::from(area.width)),
            usize::from(area.width),
            Style::default().fg(theme.gray_dim).bg(theme.bg_base),
        );
    }

    let list = Rect::new(
        area.x,
        area.y.saturating_add(2),
        area.width,
        area.height.saturating_sub(2),
    );
    let visible = state.visible_rows();
    let key_width = visible
        .iter()
        .filter_map(|row| match row {
            ShortcutRow::Action(index) => actions::definitions()
                .get(*index)
                .map(|definition| Line::from(definition.key_display()).width()),
            ShortcutRow::Category(_) => None,
        })
        .max()
        .unwrap_or(0)
        .min(22)
        .min(usize::from(list.width.saturating_sub(8)));
    let mut lines = Vec::new();
    for (row_id, row) in visible.iter().copied().enumerate() {
        match row {
            ShortcutRow::Category(category) => {
                let marker = if state.query().is_empty() && state.is_collapsed(category) {
                    "▸"
                } else {
                    "▾"
                };
                lines.push(BrowserLine {
                    row_id: Some(row_id),
                    line: vec![
                        format!("{marker} ").into(),
                        category.label().to_string().into(),
                    ]
                    .into(),
                    selected: state.selected() == row_id
                        || state.pointer.hovered_row() == Some(row_id),
                    inactive: false,
                });
            }
            ShortcutRow::Action(index) => {
                let Some(definition) = actions::definitions().get(index) else {
                    continue;
                };
                let key = definition.key_display();
                let key_padding = key_width.saturating_sub(Line::from(key.as_str()).width());
                lines.push(BrowserLine {
                    row_id: Some(row_id),
                    line: vec![
                        "  ".into(),
                        key.into(),
                        " ".repeat(key_padding + 2).into(),
                        definition.description.into(),
                    ]
                    .into(),
                    selected: state.selected() == row_id
                        || state.pointer.hovered_row() == Some(row_id),
                    inactive: !state.is_active(definition),
                });
                if state.is_expanded(definition.id) {
                    let help = definition.long_help.unwrap_or(definition.description);
                    let width = list.width.saturating_sub(4).max(1);
                    lines.extend(
                        render_literal_with_metadata(
                            help,
                            width,
                            Style::default().fg(theme.gray).bg(theme.bg_base),
                        )
                        .into_iter()
                        .map(|wrapped| {
                            let mut spans = vec!["    ".into()];
                            spans.extend(wrapped.line.spans);
                            BrowserLine {
                                row_id: None,
                                line: Line::from(spans),
                                selected: false,
                                inactive: false,
                            }
                        }),
                    );
                }
            }
        }
    }
    keep_selection_visible(state, &lines, list.height);
    let mut hits = Vec::new();
    for (rendered_index, line) in lines
        .iter()
        .skip(state.scroll_offset)
        .take(usize::from(list.height))
        .enumerate()
    {
        let y = list.y + u16::try_from(rendered_index).unwrap_or(u16::MAX);
        let row_area = Rect::new(list.x, y, list.width, 1);
        let row_style = if line.selected {
            modal_choice_style(theme, /* selected */ true)
        } else if line.inactive {
            Style::default()
                .fg(theme.gray_dim)
                .bg(theme.bg_base)
                .add_modifier(Modifier::DIM)
        } else {
            Style::default().fg(theme.text_primary).bg(theme.bg_base)
        };
        buffer.set_style(row_area, row_style);
        buffer.set_line(list.x, y, &line.line, list.width);
        if let Some(row_id) = line.row_id {
            hits.push(ModalRowHit {
                id: row_id,
                area: row_area,
            });
        }
    }
    hits
}

fn render_detail(
    state: &mut ShortcutHelpState,
    definition: &actions::ActionDef,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    let context = match definition.context {
        When::Always => "prompt and transcript",
        When::PromptFocused => "prompt",
        When::ScrollbackFocused => "transcript",
    };
    let help = definition.long_help.unwrap_or(definition.description);
    let mut lines = vec![
        Line::from(Span::styled(
            definition.description,
            Style::default()
                .fg(theme.text_primary)
                .bg(theme.bg_base)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("Key: ", Style::default().fg(theme.gray).bg(theme.bg_base)),
            Span::styled(
                definition.key_display(),
                Style::default()
                    .fg(theme.text_primary)
                    .bg(theme.bg_base)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "Available in: ",
                Style::default().fg(theme.gray).bg(theme.bg_base),
            ),
            Span::styled(
                context,
                Style::default().fg(theme.text_secondary).bg(theme.bg_base),
            ),
        ]),
    ];
    if help != definition.description {
        lines.push(Line::default());
        lines.extend(
            render_literal_with_metadata(
                help,
                area.width.max(1),
                Style::default().fg(theme.text_primary).bg(theme.bg_base),
            )
            .into_iter()
            .map(|wrapped| wrapped.line),
        );
    }
    let max_scroll = lines.len().saturating_sub(usize::from(area.height));
    state.detail_scroll = state.detail_scroll.min(max_scroll);
    for (index, line) in lines
        .iter()
        .skip(state.detail_scroll)
        .take(usize::from(area.height))
        .enumerate()
    {
        buffer.set_line(
            area.x,
            area.y + u16::try_from(index).unwrap_or(u16::MAX),
            line,
            area.width,
        );
    }
}

fn keep_selection_visible(state: &mut ShortcutHelpState, lines: &[BrowserLine], height: u16) {
    let Some(selected_line) = lines.iter().position(|line| line.selected) else {
        return;
    };
    let height = usize::from(height).max(1);
    if selected_line < state.scroll_offset {
        state.scroll_offset = selected_line;
    } else if selected_line >= state.scroll_offset.saturating_add(height) {
        state.scroll_offset = selected_line.saturating_add(1).saturating_sub(height);
    }
    state.scroll_offset = state.scroll_offset.min(lines.len().saturating_sub(height));
}

struct BrowserLine {
    row_id: Option<usize>,
    line: Line<'static>,
    selected: bool,
    inactive: bool,
}
