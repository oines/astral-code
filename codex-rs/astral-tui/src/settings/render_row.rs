use codex_app_server_protocol::ExperimentalFeatureStage;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::view::AstralTheme;
use crate::view::modal_choice_style;

use super::SettingKind;
use super::SettingsRow;
use super::SettingsState;

pub(super) struct RowGeometry {
    pub(super) expand: Option<Rect>,
    pub(super) value: Option<Rect>,
}

pub(super) fn render_row(
    state: &SettingsState,
    row: SettingsRow,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
    selected: bool,
) -> RowGeometry {
    let style = modal_choice_style(theme, selected);
    buffer.set_style(area, style);
    let (marker, label, value, description, key, source, effect) = match row {
        SettingsRow::Category(category) => (
            "›",
            category.label().to_string(),
            state.category_value_label(category),
            category.description().to_string(),
            String::new(),
            String::new(),
            String::new(),
        ),
        SettingsRow::Definition(definition) => {
            let marker = match definition.kind {
                SettingKind::Subpage(_) => "›",
                SettingKind::Bool
                | SettingKind::Integer
                | SettingKind::Text
                | SettingKind::DefaultProvider
                | SettingKind::DefaultModel
                | SettingKind::Enum(_)
                | SettingKind::Theme
                | SettingKind::PermissionProfile => {
                    if state.row_expanded(row) {
                        "▾"
                    } else {
                        "▸"
                    }
                }
            };
            (
                marker,
                definition.label.to_string(),
                state.value_label(definition),
                definition.description.to_string(),
                definition.key.to_string(),
                if !definition.key.is_empty() {
                    state.store.source_label(definition.key)
                } else {
                    Default::default()
                },
                definition.takes_effect.to_string(),
            )
        }
        SettingsRow::Feature(index) => {
            let feature = &state.store.data().features[index];
            (
                if state.row_expanded(row) {
                    "▾"
                } else {
                    "▸"
                },
                feature
                    .display_name
                    .as_deref()
                    .unwrap_or(feature.name.as_str())
                    .to_string(),
                state.feature_value_label(index).to_string(),
                feature
                    .description
                    .as_deref()
                    .or(feature.announcement.as_deref())
                    .unwrap_or("Feature reported by app-server")
                    .to_string(),
                format!("features.{}", feature.name),
                state
                    .store
                    .source_label(&format!("features.{}", feature.name)),
                feature_stage_label(&feature.stage).to_string(),
            )
        }
    };
    let prefix = if selected { "❯" } else { " " };
    let label_x = area.x.saturating_add(4);
    let stack_value = area.width < 56 && !value.is_empty();
    let max_value_width = usize::from(area.width / 2);
    let value = truncate_to_width(&value, max_value_width);
    let value_width = Line::from(value.as_str()).width();
    let value_x = (!value.is_empty() && !stack_value).then(|| {
        area.right()
            .saturating_sub(u16::try_from(value_width).unwrap_or(area.width))
    });
    let label_width = value_x.map_or_else(
        || usize::from(area.right().saturating_sub(label_x)),
        |value_x| usize::from(value_x.saturating_sub(label_x).saturating_sub(1)),
    );
    let label = truncate_to_width(&label, label_width);
    buffer.set_stringn(
        area.x,
        area.y,
        format!("{prefix} {marker} {label}"),
        usize::from(area.width),
        style,
    );
    let mut value_hit = if let Some(x) = value_x {
        buffer.set_stringn(
            x,
            area.y,
            value.as_str(),
            value_width,
            Style::default()
                .fg(if selected {
                    theme.accent_running
                } else {
                    theme.gray
                })
                .bg(style.bg.unwrap_or(theme.bg_base)),
        );
        Some(Rect::new(
            x,
            area.y,
            u16::try_from(value_width).unwrap_or(area.width),
            1,
        ))
    } else {
        None
    };
    if stack_value && area.height > 1 {
        let value_area = Rect::new(
            area.x.saturating_add(4),
            area.y.saturating_add(1),
            area.width.saturating_sub(4),
            1,
        );
        buffer.set_stringn(
            value_area.x,
            value_area.y,
            truncate_to_width(&value, usize::from(area.width.saturating_sub(4))),
            usize::from(area.width.saturating_sub(4)),
            Style::default()
                .fg(if selected {
                    theme.accent_running
                } else {
                    theme.gray
                })
                .bg(style.bg.unwrap_or(theme.bg_base)),
        );
        value_hit = Some(value_area);
    }
    let detail_start = if stack_value { 2 } else { 1 };
    if area.height > detail_start {
        let description_area = Rect::new(
            area.x.saturating_add(4),
            area.y.saturating_add(detail_start),
            area.width.saturating_sub(4),
            1,
        );
        render_wrapped_line(
            description_area,
            buffer,
            &description,
            Style::default()
                .fg(theme.gray)
                .bg(style.bg.unwrap_or(theme.bg_base)),
        );
    }
    if area.height > detail_start.saturating_add(1) && !key.is_empty() {
        let override_note = if state.store.is_overridden_above_user(&key) {
            " · user value overridden"
        } else {
            Default::default()
        };
        let metadata = format!("{key} · {source} · {effect}{override_note}");
        let metadata_area = Rect::new(
            area.x.saturating_add(4),
            area.y.saturating_add(detail_start.saturating_add(1)),
            area.width.saturating_sub(4),
            area.height.saturating_sub(detail_start.saturating_add(1)),
        );
        Paragraph::new(metadata.dim())
            .wrap(ratatui::widgets::Wrap { trim: false })
            .render(metadata_area, buffer);
    }
    if let Some(reason) = state.row_disabled_reason(row) {
        let disabled = Rect::new(
            area.x.saturating_add(4),
            area.bottom().saturating_sub(1),
            area.width.saturating_sub(4),
            1,
        );
        buffer.set_stringn(
            disabled.x,
            disabled.y,
            reason,
            usize::from(disabled.width),
            Style::default()
                .fg(theme.accent_error)
                .bg(style.bg.unwrap_or(theme.bg_base)),
        );
    }
    RowGeometry {
        expand: row_is_expandable(row).then(|| Rect::new(area.x.saturating_add(2), area.y, 1, 1)),
        value: value_hit,
    }
}

pub(super) fn ensure_selection_visible(
    state: &mut SettingsState,
    rows: &[SettingsRow],
    height: u16,
    width: u16,
) {
    if rows.is_empty() {
        state.scroll_offset = 0;
        state.selected = 0;
        return;
    }
    state.selected = state.selected.min(rows.len() - 1);
    if state.selected < state.scroll_offset {
        state.scroll_offset = state.selected;
    }
    while visible_height(state, rows, state.scroll_offset, state.selected, width) > height
        && state.scroll_offset < state.selected
    {
        state.scroll_offset += 1;
    }
}

pub(super) fn render_wrapped_line(area: Rect, buffer: &mut Buffer, text: &str, style: Style) {
    if area.is_empty() {
        return;
    }
    let width = usize::from(area.width).max(1);
    if area.height == 1 {
        buffer.set_stringn(area.x, area.y, truncate_to_width(text, width), width, style);
        return;
    }
    let lines = textwrap::wrap(text, width)
        .into_iter()
        .take(usize::from(area.height))
        .map(|line| Line::from(line.into_owned()).style(style))
        .collect::<Vec<_>>();
    Paragraph::new(lines).render(area, buffer);
}

pub(super) fn truncate_to_width(text: &str, max_width: usize) -> String {
    if Line::from(text).width() <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    let target = max_width.saturating_sub(1);
    let mut result = String::new();
    for character in text.chars() {
        let candidate = format!("{result}{character}");
        if Line::from(candidate.as_str()).width() > target {
            break;
        }
        result.push(character);
    }
    result.push('…');
    result
}

pub(super) fn row_height(state: &SettingsState, row: SettingsRow, width: u16) -> u16 {
    if matches!(row, SettingsRow::Category(_)) {
        2
    } else if state.row_expanded(row) || state.row_disabled_reason(row).is_some() {
        if width < 56 { 5 } else { 4 }
    } else if width < 56 {
        2
    } else {
        1
    }
}

fn row_is_expandable(row: SettingsRow) -> bool {
    match row {
        SettingsRow::Category(_) => false,
        SettingsRow::Definition(definition) => !matches!(definition.kind, SettingKind::Subpage(_)),
        SettingsRow::Feature(_) => true,
    }
}

fn visible_height(
    state: &SettingsState,
    rows: &[SettingsRow],
    start: usize,
    end: usize,
    width: u16,
) -> u16 {
    rows.iter()
        .enumerate()
        .skip(start)
        .take(end.saturating_sub(start).saturating_add(1))
        .map(|(_, row)| row_height(state, *row, width))
        .fold(0_u16, u16::saturating_add)
}

fn feature_stage_label(stage: &ExperimentalFeatureStage) -> &'static str {
    match stage {
        ExperimentalFeatureStage::Stable => "Stable · immediately",
        ExperimentalFeatureStage::Beta => "Beta · next request",
        ExperimentalFeatureStage::UnderDevelopment => "Under development · next request",
        ExperimentalFeatureStage::Deprecated | ExperimentalFeatureStage::Removed => "Hidden",
    }
}
