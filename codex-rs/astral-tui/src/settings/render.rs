use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;

use crate::modal::ModalRowHit;
use crate::view::AstralTheme;
use crate::view::ModalSizing;
use crate::view::modal_choice_style;
use crate::view::render_modal_close_button;
use crate::view::render_modal_frame_with_sizing;

use super::BACK_ROW_ID;
use super::SEARCH_ROW_ID;
use super::SettingsPage;
use super::SettingsState;
use super::render_row::ensure_selection_visible;
use super::render_row::render_row;
use super::render_row::row_height;

pub(crate) fn render(
    state: &mut SettingsState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    if state.editor.is_some() {
        super::render_editor::render(state, area, buffer, theme);
    } else if state.page == SettingsPage::Models {
        let notice = state.notice().map(str::to_string);
        let notice_is_error = state.notice_is_error();
        super::pages::models::render(
            &mut state.models,
            area,
            buffer,
            theme,
            notice.as_deref().map(|message| (message, notice_is_error)),
        );
    } else if matches!(
        state.page,
        SettingsPage::Search | SettingsPage::SessionMemoryTemplates
    ) {
        super::pages::render(state, area, buffer, theme);
    } else {
        render_browser(state, area, buffer, theme);
    }
}

fn render_browser(state: &mut SettingsState, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
    let footer = if state.search_focused {
        "Type · ↑/↓ results · Enter · Esc clear"
    } else if state.page == SettingsPage::Root {
        "j/k · Enter open · / search · Esc close"
    } else {
        "j/k · Enter · l info · d reset · Esc back"
    };
    let title = if state.query.text().is_empty() {
        page_title(state.page)
    } else {
        "Settings / Search"
    };
    let Some(frame) =
        render_modal_frame_with_sizing(area, buffer, theme, title, footer, ModalSizing::settings())
    else {
        return;
    };
    render_modal_close_button(
        buffer,
        frame.close_button,
        theme,
        state.pointer.close_hovered(),
    );
    let mut y = frame.content.y;
    let mut hits = Vec::new();
    if state.page != SettingsPage::Root && state.query.text().is_empty() {
        let breadcrumb = Rect::new(frame.content.x, y, frame.content.width, 1);
        let hovered = state.pointer.hovered_row() == Some(BACK_ROW_ID);
        let style = if hovered {
            Style::default()
                .fg(theme.accent_running)
                .bg(theme.bg_base)
                .underlined()
        } else {
            Style::default().fg(theme.gray).bg(theme.bg_base)
        };
        buffer.set_stringn(
            breadcrumb.x,
            breadcrumb.y,
            format!("‹ Settings  /  {}", page_title(state.page)),
            usize::from(breadcrumb.width),
            style,
        );
        hits.push(ModalRowHit {
            id: BACK_ROW_ID,
            area: breadcrumb,
        });
        y = y.saturating_add(2);
    }
    if y >= frame.content.bottom() {
        state
            .pointer
            .observe_frame(frame.popup, frame.close_button, hits);
        return;
    }
    let search = Rect::new(frame.content.x, y, frame.content.width, 1);
    let search_selected =
        state.search_focused || state.pointer.hovered_row() == Some(SEARCH_ROW_ID);
    let search_style = modal_choice_style(theme, search_selected);
    buffer.set_style(search, search_style);
    buffer.set_stringn(
        search.x,
        search.y,
        render_search(state, search_selected),
        usize::from(search.width),
        search_style,
    );
    hits.push(ModalRowHit {
        id: SEARCH_ROW_ID,
        area: search,
    });
    y = y.saturating_add(2);
    let notice_height = u16::from(state.notice().is_some());
    let list = Rect::new(
        frame.content.x,
        y,
        frame.content.width,
        frame
            .content
            .bottom()
            .saturating_sub(y)
            .saturating_sub(notice_height),
    );
    let rows = state.rows();
    ensure_selection_visible(state, &rows, list.height, list.width);
    state.row_expand_hits = vec![None; rows.len()];
    state.row_value_hits = vec![None; rows.len()];
    let mut row_y = list.y;
    for (index, row) in rows.iter().enumerate().skip(state.scroll_offset) {
        let selected = (!state.search_focused && index == state.selected)
            || state.pointer.hovered_row() == Some(index);
        let height = row_height(state, *row, list.width);
        if row_y.saturating_add(height) > list.bottom() {
            break;
        }
        let row_area = Rect::new(list.x, row_y, list.width, height);
        let geometry = render_row(state, *row, row_area, buffer, theme, selected);
        state.row_expand_hits[index] = geometry.expand;
        state.row_value_hits[index] = geometry.value;
        hits.push(ModalRowHit {
            id: index,
            area: row_area,
        });
        row_y = row_y.saturating_add(height);
    }
    if rows.is_empty() && !list.is_empty() {
        buffer.set_stringn(
            list.x,
            list.y,
            "No matching settings",
            usize::from(list.width),
            Style::default().fg(theme.gray).bg(theme.bg_base),
        );
    }
    if let Some(notice) = state.notice() {
        let notice_area = Rect::new(list.x, list.bottom(), list.width, 1);
        buffer.set_stringn(
            notice_area.x,
            notice_area.y,
            notice,
            usize::from(notice_area.width),
            Style::default()
                .fg(if state.notice_is_error() {
                    theme.accent_error
                } else {
                    theme.accent_running
                })
                .bg(theme.bg_base),
        );
    }
    state
        .pointer
        .observe_frame(frame.popup, frame.close_button, hits);
}

fn render_search(state: &SettingsState, selected: bool) -> String {
    let marker = if selected { "❯ " } else { "  " };
    if state.query.text().is_empty() {
        return if state.search_focused {
            format!("{marker}Search settings  ▏")
        } else {
            format!("{marker}Search settings by name, description, or key…")
        };
    }
    if !state.search_focused {
        return format!("{marker}Search  {}", state.query.text());
    }
    let text = state.query.text();
    let cursor = state.query.cursor().min(text.len());
    format!("{marker}Search  {}▏{}", &text[..cursor], &text[cursor..])
}

fn page_title(page: SettingsPage) -> &'static str {
    match page {
        SettingsPage::Root => "Settings",
        SettingsPage::Category(category) => category.label(),
        SettingsPage::Models => "Models & Providers",
        SettingsPage::Search => "Search Provider",
        SettingsPage::SessionMemoryTemplates => "Session Memory Templates",
    }
}
