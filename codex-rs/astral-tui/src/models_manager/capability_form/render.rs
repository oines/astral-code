use codex_protocol::openai_models::ToolMode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::modal::ModalPointerState;
use crate::modal::ModalRowHit;
use crate::view::AstralTheme;
use crate::view::ModalHeight;
use crate::view::modal_choice_style;
use crate::view::render_modal_close_button;
use crate::view::render_modal_frame_with_geometry;

use super::CapabilityField;
use super::CapabilityFormState;
use super::config_key;

pub(in crate::models_manager) fn render(
    form: &CapabilityFormState,
    pointer: &mut ModalPointerState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    let title = if form.editing_id.is_some() {
        format!("Model overrides · {}", form.model_id)
    } else {
        format!("Add model · {}", form.provider_name)
    };
    let Some(frame) = render_modal_frame_with_geometry(
        area,
        buffer,
        theme,
        &title,
        "↑/↓ fields · ←/→ choices · Enter next/save · Esc back",
        ModalHeight::FullViewport,
    ) else {
        return;
    };
    render_modal_close_button(buffer, frame.close_button, theme, pointer.close_hovered());
    let fields = form.fields();
    let visible_rows = usize::from(frame.content.height.saturating_sub(2));
    let start = form.selected.saturating_add(1).saturating_sub(visible_rows);
    let mut hits = Vec::new();
    for (index, field) in fields
        .into_iter()
        .enumerate()
        .skip(start)
        .take(visible_rows)
    {
        let y = frame.content.y + u16::try_from(index.saturating_sub(start)).unwrap_or(u16::MAX);
        let row = Rect::new(frame.content.x, y, frame.content.width, 1);
        let selected = form.selected == index || pointer.hovered_row() == Some(index);
        let style = modal_choice_style(theme, selected);
        buffer.set_style(row, style);
        let marker = if selected { "❯ " } else { "  " };
        buffer.set_stringn(
            row.x,
            row.y,
            format!("{marker}{:<22}", field_label(*field)),
            24,
            style,
        );
        buffer.set_stringn(
            row.x.saturating_add(24),
            row.y,
            display_value(form, *field, selected),
            usize::from(row.width.saturating_sub(24)),
            style,
        );
        hits.push(ModalRowHit {
            id: index,
            area: row,
        });
    }
    let note_y = frame.content.bottom().saturating_sub(1);
    buffer.set_stringn(
        frame.content.x,
        note_y,
        "Blank/Inherit keeps discovered metadata; entered values are manual overrides.",
        usize::from(frame.content.width),
        Style::default().fg(theme.gray).bg(theme.bg_base),
    );
    if let Some(error) = form.error.as_deref() {
        buffer.set_stringn(
            frame.content.x,
            note_y,
            error,
            usize::from(frame.content.width),
            Style::default().fg(theme.accent_error).bg(theme.bg_base),
        );
    }
    pointer.observe_frame(frame.popup, frame.close_button, hits);
}

fn display_value(form: &CapabilityFormState, field: CapabilityField, selected: bool) -> String {
    match field {
        CapabilityField::ModelId if form.editing_id.is_some() => {
            format!("{}  (read-only)", form.model_id)
        }
        CapabilityField::ModelId => editor_value(form, &form.model_id, selected, None),
        CapabilityField::ContextWindow => editor_value(
            form,
            form.draft(field),
            selected,
            form.effective.context_window,
        ),
        CapabilityField::MaxContextWindow => editor_value(
            form,
            form.draft(field),
            selected,
            form.effective.max_context_window,
        ),
        CapabilityField::MaxOutputTokens => editor_value(
            form,
            form.draft(field),
            selected,
            form.effective.max_output_tokens,
        ),
        CapabilityField::ToolMode => choice_value(
            tool_mode_label(
                form.raw
                    .get(config_key(field))
                    .and_then(serde_json::Value::as_str),
            ),
            form.effective.tool_mode.map(effective_tool_mode),
        ),
        CapabilityField::SupportsTools => bool_value(
            form.raw
                .get(config_key(field))
                .and_then(serde_json::Value::as_bool),
            form.effective.supports_tools,
        ),
        CapabilityField::SupportsParallelTools => bool_value(
            form.raw
                .get(config_key(field))
                .and_then(serde_json::Value::as_bool),
            form.effective.supports_parallel_tools,
        ),
        CapabilityField::SupportsVision => bool_value(
            form.raw
                .get(config_key(field))
                .and_then(serde_json::Value::as_bool),
            form.effective.supports_vision,
        ),
        CapabilityField::SupportsPromptCache => bool_value(
            form.raw
                .get(config_key(field))
                .and_then(serde_json::Value::as_bool),
            form.effective.supports_prompt_cache,
        ),
        CapabilityField::SupportsReasoning => bool_value(
            form.raw
                .get(config_key(field))
                .and_then(serde_json::Value::as_bool),
            form.effective.supports_reasoning,
        ),
        CapabilityField::SupportsNativeStreaming => bool_value(
            form.raw
                .get(config_key(field))
                .and_then(serde_json::Value::as_bool),
            form.effective.supports_native_streaming,
        ),
        CapabilityField::LiteLlmProvider => editor_value(form, form.draft(field), selected, None),
        CapabilityField::Mode => editor_value(form, form.draft(field), selected, None),
        CapabilityField::SupportedEndpoints => {
            editor_value(form, form.draft(field), selected, None)
        }
        CapabilityField::Advanced => {
            if form.advanced {
                "Hide advanced fields".to_string()
            } else {
                "Show advanced fields".to_string()
            }
        }
        CapabilityField::Save => "Write manual overrides to user config".to_string(),
    }
}

fn editor_value(
    form: &CapabilityFormState,
    committed: &str,
    selected: bool,
    effective: Option<i64>,
) -> String {
    if selected {
        let value = form.editor.text();
        let cursor = form.editor.cursor().min(value.len());
        return format!("{}▏{}", &value[..cursor], &value[cursor..]);
    }
    if !committed.is_empty() {
        return committed.to_string();
    }
    effective.map_or_else(
        || "Inherit".to_string(),
        |effective| format!("Inherit (effective: {effective})"),
    )
}

fn bool_value(value: Option<bool>, effective: Option<bool>) -> String {
    match value {
        Some(true) => "Yes  ←/→".to_string(),
        Some(false) => "No  ←/→".to_string(),
        None => choice_value(
            "Inherit",
            effective.map(|value| if value { "yes" } else { "no" }),
        ),
    }
}

fn choice_value(value: &str, effective: Option<&str>) -> String {
    effective.map_or_else(
        || format!("{value}  ←/→"),
        |effective| format!("{value} (effective: {effective})  ←/→"),
    )
}

fn tool_mode_label(value: Option<&str>) -> &'static str {
    match value {
        Some("direct") => "Direct",
        Some("code_mode") => "Code mode",
        Some("code_mode_only") => "Code mode only",
        Some(_) | None => "Inherit",
    }
}

fn effective_tool_mode(value: ToolMode) -> &'static str {
    match value {
        ToolMode::Direct => "direct",
        ToolMode::CodeMode => "code mode",
        ToolMode::CodeModeOnly => "code mode only",
    }
}

fn field_label(field: CapabilityField) -> &'static str {
    match field {
        CapabilityField::ModelId => "Model ID",
        CapabilityField::ContextWindow => "Context window",
        CapabilityField::MaxOutputTokens => "Max output tokens",
        CapabilityField::ToolMode => "Tool mode",
        CapabilityField::SupportsTools => "Tools",
        CapabilityField::SupportsVision => "Vision",
        CapabilityField::SupportsReasoning => "Reasoning",
        CapabilityField::MaxContextWindow => "Max context window",
        CapabilityField::SupportsParallelTools => "Parallel tools",
        CapabilityField::SupportsPromptCache => "Prompt cache",
        CapabilityField::SupportsNativeStreaming => "Native streaming",
        CapabilityField::LiteLlmProvider => "LiteLLM provider",
        CapabilityField::Mode => "LiteLLM mode",
        CapabilityField::SupportedEndpoints => "Supported endpoints",
        CapabilityField::Advanced => "Advanced",
        CapabilityField::Save => "Save model",
    }
}
