use codex_app_server_protocol::Model;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;

use crate::modal::ModalRowHit;
use crate::view::AstralTheme;
use crate::view::ModalSizing;
use crate::view::modal_choice_style;
use crate::view::render_modal_close_button;
use crate::view::render_modal_frame_with_sizing;

use super::BrowserRow;
use super::BrowserScroll;
use super::ModelsManagerState;
use super::ProviderLoad;
use super::SEARCH_ROW_ID;
use super::capability_form;
use super::capability_sources;
use super::provider_form;

pub(crate) fn render(
    state: &mut ModelsManagerState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
    notice: Option<(&str, bool)>,
) {
    if let Some(form) = state.capability_form.clone() {
        capability_form::render(&form, &mut state.pointer, area, buffer, theme, notice);
    } else if let Some(form) = state.provider_form.clone() {
        provider_form::render(&form, &mut state.pointer, area, buffer, theme, notice);
    } else if let Some(model) = state.detail.clone() {
        render_detail(state, &model, area, buffer, theme, notice);
    } else {
        render_browser(state, area, buffer, theme, notice);
    }
}

fn render_browser(
    state: &mut ModelsManagerState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
    notice: Option<(&str, bool)>,
) {
    let footer = if state.search_focused() {
        "Type · ↑/↓ results · Enter · Esc clear"
    } else {
        "Type · j/k move · h/l fold · Enter · Esc"
    };
    let Some(frame) = render_modal_frame_with_sizing(
        area,
        buffer,
        theme,
        "Settings / Models & Providers",
        footer,
        ModalSizing::settings(),
    ) else {
        return;
    };
    render_modal_close_button(
        buffer,
        frame.close_button,
        theme,
        state.pointer.close_hovered(),
    );
    let content = frame.content;
    if content.is_empty() {
        state
            .pointer
            .observe_frame(frame.popup, frame.close_button, Vec::new());
        return;
    }
    let search_area = Rect::new(content.x, content.y, content.width, 1);
    let search_selected =
        state.search_focused() || state.pointer.hovered_row() == Some(SEARCH_ROW_ID);
    let search_style = modal_choice_style(theme, search_selected);
    buffer.set_style(search_area, search_style);
    let search = render_search(state, search_selected);
    buffer.set_stringn(
        search_area.x,
        search_area.y,
        search,
        usize::from(search_area.width),
        if search_selected {
            search_style
        } else {
            Style::default().fg(theme.gray).bg(theme.bg_base)
        },
    );
    let list = Rect::new(
        content.x,
        content.y.saturating_add(2),
        content.width,
        content
            .height
            .saturating_sub(2)
            .saturating_sub(u16::from(notice.is_some())),
    );
    let rows = state.rows();
    ensure_selection_visible(state, rows.len(), usize::from(list.height));
    state.provider_toggle_hits = vec![None; rows.len()];
    let mut hits = vec![ModalRowHit {
        id: SEARCH_ROW_ID,
        area: search_area,
    }];
    for (row_index, row) in rows
        .iter()
        .enumerate()
        .skip(state.scroll_offset)
        .take(usize::from(list.height))
    {
        let y = list.y
            + u16::try_from(row_index.saturating_sub(state.scroll_offset)).unwrap_or(u16::MAX);
        let area = Rect::new(list.x, y, list.width, 1);
        let selected = (!state.search_focused() && state.selected == row_index)
            || state.pointer.hovered_row() == Some(row_index);
        state.provider_toggle_hits[row_index] =
            render_browser_row(state, row, area, buffer, theme, selected);
        hits.push(ModalRowHit {
            id: row_index,
            area,
        });
    }
    if rows.is_empty() {
        buffer.set_stringn(
            list.x,
            list.y,
            "No matching providers or models",
            usize::from(list.width),
            Style::default().fg(theme.gray).bg(theme.bg_base),
        );
    }
    render_notice(list.bottom(), content, buffer, theme, notice);
    state
        .pointer
        .observe_frame(frame.popup, frame.close_button, hits);
}

fn render_search(state: &ModelsManagerState, selected: bool) -> String {
    let marker = if selected { "❯ " } else { "  " };
    if state.query.text().is_empty() {
        return if state.search_focused() {
            format!("{marker}Search  ▏")
        } else {
            format!("{marker}Search providers and loaded models…")
        };
    }
    if !state.search_focused() {
        return format!("{marker}Search  {}", state.query.text());
    }
    let query = state.query.text();
    let cursor = state.query.cursor().min(query.len());
    format!("{marker}Search  {}▏{}", &query[..cursor], &query[cursor..])
}

fn render_browser_row(
    state: &ModelsManagerState,
    row: &BrowserRow,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
    selected: bool,
) -> Option<Rect> {
    let style = modal_choice_style(theme, selected);
    buffer.set_style(area, style);
    let (label, description) = match row {
        BrowserRow::AddProvider => (
            "＋ Add provider".to_string(),
            "Configure a custom endpoint".to_string(),
        ),
        BrowserRow::AddModel { provider_index } => (
            "    ＋ Add model".to_string(),
            format!("Add to {}", state.providers[*provider_index].name),
        ),
        BrowserRow::Provider { provider_index } => {
            let provider = &state.providers[*provider_index];
            let marker = if provider.expanded { "▾" } else { "▸" };
            let current = (provider.id == state.current_provider).then_some("active session");
            let default = (state.default_provider.as_deref() == Some(provider.id.as_str()))
                .then_some("new-session default");
            let detail = [
                current,
                default,
                provider.wire_api.as_deref(),
                provider.base_url.as_deref(),
                Some(provider.source_label.as_str()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
            (
                format!("{marker} {}  [{}]", provider.name, provider.id),
                detail,
            )
        }
        BrowserRow::EditProvider { provider_index } => (
            "    ◇ Settings".to_string(),
            format!("Edit {}", state.providers[*provider_index].name),
        ),
        BrowserRow::DeleteProvider { provider_index } => {
            let provider = &state.providers[*provider_index];
            if super::is_built_in_provider(&provider.id) {
                (
                    "    ↺ Restore bundled provider".to_string(),
                    format!("Reset {}", provider.name),
                )
            } else {
                (
                    "    − Remove provider".to_string(),
                    format!("Delete {}", provider.name),
                )
            }
        }
        BrowserRow::Status { provider_index } => {
            let provider = &state.providers[*provider_index];
            let status = match &provider.load {
                ProviderLoad::NotLoaded => "Enter to discover models",
                ProviderLoad::Loading => "Discovering models…",
                ProviderLoad::Loaded => "No models returned",
                ProviderLoad::Failed(error) => error,
            };
            ("    ◇ Models".to_string(), status.to_string())
        }
        BrowserRow::Model {
            provider_index,
            model_index,
        } => {
            let model = &state.providers[*provider_index].models[*model_index];
            let marker = if model.model_provider == state.current_provider
                && model.model == state.current_model
            {
                "●"
            } else {
                "○"
            };
            let capabilities = compact_capabilities(model);
            (format!("    {marker} {}", model.display_name), capabilities)
        }
    };
    let prefix = if selected { "❯ " } else { "  " };
    render_label_and_description(
        area,
        buffer,
        theme,
        style,
        format!("{prefix}{label}"),
        description,
        selected,
    );
    matches!(row, BrowserRow::Provider { .. })
        .then(|| Rect::new(area.x.saturating_add(2), area.y, 1, 1))
}

fn render_detail(
    state: &mut ModelsManagerState,
    model: &Model,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
    notice: Option<(&str, bool)>,
) {
    let editable = state.detail_can_edit();
    let Some(frame) = render_modal_frame_with_sizing(
        area,
        buffer,
        theme,
        &model.display_name,
        if editable {
            "j/k scroll · Enter edit · Esc back"
        } else {
            "j/k scroll · Esc back"
        },
        ModalSizing::settings(),
    ) else {
        return;
    };
    render_modal_close_button(
        buffer,
        frame.close_button,
        theme,
        state.pointer.close_hovered(),
    );
    let capabilities = &model.capabilities;
    let rows = [
        (
            "Provider",
            format!("{} [{}]", model.model_provider_name, model.model_provider),
        ),
        ("Model ID", model.model.clone()),
        ("Description", model.description.clone()),
        ("Context", optional_number(capabilities.context_window)),
        (
            "Max context",
            optional_number(capabilities.max_context_window),
        ),
        (
            "Max output",
            optional_number(capabilities.max_output_tokens),
        ),
        (
            "Tool mode",
            capabilities
                .tool_mode
                .map(|mode| match mode {
                    codex_protocol::openai_models::ToolMode::Direct => "direct",
                    codex_protocol::openai_models::ToolMode::CodeMode => "code mode",
                    codex_protocol::openai_models::ToolMode::CodeModeOnly => "code mode only",
                })
                .map(str::to_string)
                .unwrap_or_else(|| "unknown".to_string()),
        ),
        ("Tools", optional_bool(capabilities.supports_tools)),
        (
            "Parallel tools",
            optional_bool(capabilities.supports_parallel_tools),
        ),
        ("Vision", optional_bool(capabilities.supports_vision)),
        (
            "Web search",
            optional_bool(capabilities.supports_web_search),
        ),
        (
            "Image generation",
            optional_bool(capabilities.supports_image_generation),
        ),
        (
            "Prompt cache",
            optional_bool(capabilities.supports_prompt_cache),
        ),
        ("Reasoning", optional_bool(capabilities.supports_reasoning)),
        (
            "Native streaming",
            optional_bool(capabilities.supports_native_streaming),
        ),
        (
            "Endpoints",
            if capabilities.supported_endpoints.is_empty() {
                "unknown".to_string()
            } else {
                capabilities.supported_endpoints.join(", ")
            },
        ),
        ("Sources", capability_sources(capabilities)),
    ];
    let label_width = rows
        .iter()
        .map(|(label, _)| Line::from(*label).width())
        .max()
        .unwrap_or_default();
    let mut hits = Vec::new();
    let details_y = if editable {
        let action = Rect::new(frame.content.x, frame.content.y, frame.content.width, 1);
        let hovered = state.pointer.hovered_row() == Some(0);
        let style = modal_choice_style(theme, hovered);
        buffer.set_style(action, style);
        buffer.set_stringn(
            action.x,
            action.y,
            format!(
                "{} ◆ Edit manual overrides",
                if hovered { "❯" } else { " " }
            ),
            usize::from(action.width),
            style,
        );
        hits.push(ModalRowHit {
            id: 0,
            area: action,
        });
        frame.content.y.saturating_add(2)
    } else {
        frame.content.y
    };
    let details_height = usize::from(
        frame
            .content
            .bottom()
            .saturating_sub(details_y)
            .saturating_sub(u16::from(notice.is_some())),
    );
    state.detail_scroll_offset = state
        .detail_scroll_offset
        .min(rows.len().saturating_sub(details_height.max(1)));
    for (visible_index, (label, value)) in rows
        .into_iter()
        .skip(state.detail_scroll_offset)
        .take(details_height)
        .enumerate()
    {
        let y = details_y + u16::try_from(visible_index).unwrap_or(u16::MAX);
        buffer.set_stringn(
            frame.content.x,
            y,
            label,
            usize::from(frame.content.width),
            Style::default().fg(theme.gray).bg(theme.bg_base),
        );
        let value_x = frame.content.x + u16::try_from(label_width).unwrap_or(u16::MAX) + 2;
        if value_x < frame.content.right() {
            buffer.set_stringn(
                value_x,
                y,
                value,
                usize::from(frame.content.right().saturating_sub(value_x)),
                Style::default().fg(theme.text_primary).bg(theme.bg_base),
            );
        }
    }
    render_notice(
        details_y.saturating_add(u16::try_from(details_height).unwrap_or(u16::MAX)),
        frame.content,
        buffer,
        theme,
        notice,
    );
    state
        .pointer
        .observe_frame(frame.popup, frame.close_button, hits);
}

fn render_notice(
    y: u16,
    content: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
    notice: Option<(&str, bool)>,
) {
    let Some((message, is_error)) = notice else {
        return;
    };
    if y >= content.bottom() {
        return;
    }
    buffer.set_stringn(
        content.x,
        y,
        message,
        usize::from(content.width),
        Style::default()
            .fg(if is_error {
                theme.accent_error
            } else {
                theme.accent_running
            })
            .bg(theme.bg_base),
    );
}

fn render_label_and_description(
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
    style: Style,
    label: String,
    description: String,
    selected: bool,
) {
    let label_width = u16::try_from(Line::from(label.as_str()).width()).unwrap_or(u16::MAX);
    let description_width =
        u16::try_from(Line::from(description.as_str()).width()).unwrap_or(u16::MAX);
    let show_description = label_width
        .saturating_add(description_width)
        .saturating_add(2)
        < area.width;
    let description_x = if show_description {
        area.right()
            .saturating_sub(description_width)
            .saturating_sub(1)
    } else {
        area.right()
    };
    buffer.set_stringn(
        area.x,
        area.y,
        label,
        usize::from(description_x.saturating_sub(area.x).saturating_sub(1)),
        style,
    );
    if show_description {
        buffer.set_stringn(
            description_x,
            area.y,
            description,
            usize::from(area.right().saturating_sub(description_x)),
            Style::default()
                .fg(if selected {
                    theme.text_primary
                } else {
                    theme.gray
                })
                .bg(if selected {
                    theme.panel_selected
                } else {
                    theme.bg_base
                }),
        );
    }
}

fn ensure_selection_visible(state: &mut ModelsManagerState, len: usize, height: usize) {
    if height == 0 {
        state.scroll_offset = state.selected;
        return;
    }
    if state.browser_scroll == BrowserScroll::FollowSelection {
        if state.selected < state.scroll_offset {
            state.scroll_offset = state.selected;
        } else if state.selected >= state.scroll_offset.saturating_add(height) {
            state.scroll_offset = state.selected.saturating_add(1).saturating_sub(height);
        }
    }
    state.scroll_offset = state.scroll_offset.min(len.saturating_sub(height));
}

fn compact_capabilities(model: &Model) -> String {
    let mut labels = Vec::new();
    if model.capabilities.supports_tools == Some(true) {
        labels.push("tools");
    }
    if model.capabilities.supports_vision == Some(true) {
        labels.push("vision");
    }
    if model.capabilities.supports_web_search == Some(true) {
        labels.push("web");
    }
    if model.capabilities.supports_image_generation == Some(true) {
        labels.push("image gen");
    }
    if model.capabilities.supports_reasoning == Some(true) {
        labels.push("reasoning");
    }
    if let Some(context) = model.capabilities.context_window {
        labels.push(if context >= 1_000_000 {
            "1M+ context"
        } else if context >= 1_000 {
            "context"
        } else {
            "small context"
        });
    }
    if labels.is_empty() {
        model.model.clone()
    } else {
        labels.join(" · ")
    }
}

fn optional_number(value: Option<i64>) -> String {
    value.map_or_else(|| "unknown".to_string(), |value| value.to_string())
}

fn optional_bool(value: Option<bool>) -> String {
    value.map_or_else(
        || "unknown".to_string(),
        |value| if value { "yes" } else { "no" }.to_string(),
    )
}
