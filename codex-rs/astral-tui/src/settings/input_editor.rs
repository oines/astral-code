use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;
use serde_json::Number;
use serde_json::Value;

use crate::modal::ModalPointerAction;
use crate::view::AstralThemeId;

use super::Category;
use super::SettingKind;
use super::SettingsConfirmAction;
use super::SettingsEditor;
use super::SettingsFocus;
use super::SettingsInput;
use super::SettingsState;

pub(super) fn handle_mouse(state: &mut SettingsState, mouse: MouseEvent) -> SettingsInput {
    match state.pointer.handle_mouse(mouse) {
        ModalPointerAction::Ignored => SettingsInput::None,
        ModalPointerAction::Redraw | ModalPointerAction::Hover(_) => SettingsInput::Redraw,
        ModalPointerAction::Close => cancel(state),
        ModalPointerAction::Activate(index) => activate_row(state, index),
        ModalPointerAction::Scroll(delta) => {
            if let Some(SettingsEditor::Picker {
                options, selected, ..
            }) = state.editor.as_mut()
            {
                *selected = selected
                    .saturating_add_signed(delta)
                    .min(options.len().saturating_sub(1));
                preview_picker(state)
            } else {
                SettingsInput::None
            }
        }
    }
}

pub(super) fn handle_key(state: &mut SettingsState, key: KeyEvent) -> SettingsInput {
    match state.editor.as_mut() {
        Some(SettingsEditor::Text { input, .. }) => match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => cancel(state),
            (KeyCode::Enter, KeyModifiers::NONE) => commit_text(state),
            _ if input.edit_key(key) => {
                state.notice = None;
                state.notice_is_error = false;
                SettingsInput::Redraw
            }
            _ => SettingsInput::None,
        },
        Some(SettingsEditor::Picker {
            options, selected, ..
        }) => match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => cancel(state),
            (KeyCode::Up | KeyCode::Char('k'), _) => {
                *selected = selected.saturating_sub(1);
                preview_picker(state)
            }
            (KeyCode::Down | KeyCode::Char('j'), _) => {
                *selected = (*selected + 1).min(options.len().saturating_sub(1));
                preview_picker(state)
            }
            (KeyCode::Enter | KeyCode::Char(' '), KeyModifiers::NONE) => commit_picker(state),
            _ => SettingsInput::None,
        },
        Some(SettingsEditor::Confirm { .. }) => match key.code {
            KeyCode::Esc | KeyCode::Char('n') => cancel(state),
            KeyCode::Enter | KeyCode::Char('y') => confirm_action(state),
            _ => SettingsInput::None,
        },
        None => SettingsInput::None,
    }
}

fn activate_row(state: &mut SettingsState, index: usize) -> SettingsInput {
    match state.editor.as_mut() {
        Some(SettingsEditor::Picker {
            options, selected, ..
        }) => {
            let next = index.min(options.len().saturating_sub(1));
            if *selected == next {
                commit_picker(state)
            } else {
                *selected = next;
                preview_picker(state)
            }
        }
        Some(SettingsEditor::Confirm { .. }) => {
            if index == 0 {
                confirm_action(state)
            } else {
                cancel(state)
            }
        }
        Some(SettingsEditor::Text { .. }) => {
            if index == 0 {
                commit_text(state)
            } else {
                cancel(state)
            }
        }
        None => SettingsInput::Redraw,
    }
}

fn commit_text(state: &mut SettingsState) -> SettingsInput {
    let Some(SettingsEditor::Text { definition, input }) = state.editor.as_ref() else {
        return SettingsInput::None;
    };
    let value = match definition.kind {
        SettingKind::Integer => {
            let Ok(value) = input.text().trim().parse::<i64>() else {
                state.set_error("Enter a valid whole number");
                return SettingsInput::Redraw;
            };
            if let Some((minimum, maximum)) = integer_range(definition.key)
                && !(minimum..=maximum).contains(&value)
            {
                state.set_error(format!(
                    "Enter a value from {minimum} to {maximum} for {}",
                    definition.label
                ));
                return SettingsInput::Redraw;
            }
            Value::Number(Number::from(value))
        }
        SettingKind::Text => Value::String(input.text().to_string()),
        SettingKind::Bool
        | SettingKind::DefaultProvider
        | SettingKind::DefaultModel
        | SettingKind::Enum(_)
        | SettingKind::Theme
        | SettingKind::PermissionProfile
        | SettingKind::Subpage(_) => return SettingsInput::None,
    };
    let Some(write) = state.store.write_value(
        definition.key,
        value,
        SettingsFocus::Key(definition.key.to_string()),
    ) else {
        return SettingsInput::Notice("User config is not writable".to_string());
    };
    SettingsInput::Write {
        write,
        selected_theme: None,
    }
}

fn integer_range(key: &str) -> Option<(i64, i64)> {
    match key {
        "session_memory_minimum_message_tokens_to_init"
        | "session_memory_minimum_tokens_between_update"
        | "session_memory_tool_calls_between_updates" => Some((1, i64::MAX)),
        "memories.max_raw_memories_for_consolidation" => Some((1, 4096)),
        "memories.max_unused_days" => Some((0, 365)),
        "memories.max_rollout_age_days" => Some((0, 90)),
        "memories.max_rollouts_per_startup" => Some((1, 128)),
        "memories.min_rollout_idle_hours" => Some((1, 48)),
        "memories.min_rate_limit_remaining_percent" => Some((0, 100)),
        _ => None,
    }
}

fn commit_picker(state: &mut SettingsState) -> SettingsInput {
    let Some(SettingsEditor::Picker {
        definition,
        feature_index,
        options,
        selected,
        original_theme: _,
    }) = state.editor.as_ref()
    else {
        return SettingsInput::None;
    };
    let Some(option) = options.get(*selected) else {
        return SettingsInput::None;
    };
    let (key, focus, selected_theme) = if let Some(definition) = definition {
        (
            definition.key.to_string(),
            SettingsFocus::Key(definition.key.to_string()),
            matches!(definition.kind, SettingKind::Theme)
                .then(|| option.value.as_str())
                .flatten()
                .and_then(AstralThemeId::from_name),
        )
    } else if let Some(index) = feature_index {
        (
            format!("features.{}", state.store.data().features[*index].name),
            SettingsFocus::Category(Category::Features),
            None,
        )
    } else {
        return SettingsInput::None;
    };
    let write = if definition
        .is_some_and(|definition| matches!(definition.kind, SettingKind::DefaultProvider))
    {
        state.store.write_edits(
            vec![
                codex_app_server_protocol::ConfigEdit {
                    key_path: key,
                    value: option.value.clone(),
                    merge_strategy: codex_app_server_protocol::MergeStrategy::Replace,
                },
                codex_app_server_protocol::ConfigEdit {
                    key_path: "model".to_string(),
                    value: Value::Null,
                    merge_strategy: codex_app_server_protocol::MergeStrategy::Replace,
                },
            ],
            focus,
        )
    } else {
        state.store.write_value(key, option.value.clone(), focus)
    };
    let Some(write) = write else {
        return SettingsInput::Notice("User config is not writable".to_string());
    };
    if option.value.as_str().is_some_and(|value| {
        matches!(
            value,
            "danger-full-access" | "danger_full_access" | ":danger-full-access"
        )
    }) {
        let (title, message) = if definition
            .is_some_and(|definition| definition.key == "memories.phase2_sandbox")
        {
            (
                "Allow unrestricted memory consolidation?",
                "Memory phase 2 will run without filesystem sandbox restrictions. Only continue if this is intentional.",
            )
        } else {
            (
                "Enable danger-full-access?",
                "This removes filesystem sandbox restrictions for new sessions. Only continue if you understand the risk.",
            )
        };
        state.editor = Some(SettingsEditor::Confirm {
            title: title.to_string(),
            message: message.to_string(),
            confirm_label: "Enable".to_string(),
            action: SettingsConfirmAction::Write {
                write,
                selected_theme,
            },
        });
        return SettingsInput::Redraw;
    }
    SettingsInput::Write {
        write,
        selected_theme,
    }
}

fn preview_picker(state: &mut SettingsState) -> SettingsInput {
    let Some(SettingsEditor::Picker {
        definition: Some(definition),
        options,
        selected,
        ..
    }) = state.editor.as_ref()
    else {
        return SettingsInput::Redraw;
    };
    if !matches!(definition.kind, SettingKind::Theme) {
        return SettingsInput::Redraw;
    }
    options
        .get(*selected)
        .and_then(|option| option.value.as_str())
        .and_then(AstralThemeId::from_name)
        .map_or(SettingsInput::Redraw, SettingsInput::PreviewTheme)
}

fn confirm_action(state: &mut SettingsState) -> SettingsInput {
    let Some(SettingsEditor::Confirm { action, .. }) = state.editor.clone() else {
        return SettingsInput::None;
    };
    match action {
        SettingsConfirmAction::Write {
            write,
            selected_theme,
        } => SettingsInput::Write {
            write,
            selected_theme,
        },
        SettingsConfirmAction::DiscardAndBack { destination } => {
            let page = state.page;
            state.discard_page_draft(page);
            state.editor = None;
            state.enter_page(destination);
            SettingsInput::Redraw
        }
        SettingsConfirmAction::DiscardModelsPanel => {
            state.models.discard_active_form();
            state.editor = None;
            SettingsInput::Redraw
        }
        SettingsConfirmAction::DiscardAndClose => SettingsInput::Close,
    }
}

fn cancel(state: &mut SettingsState) -> SettingsInput {
    state
        .cancel_editor()
        .map_or(SettingsInput::Redraw, |theme| {
            SettingsInput::RestoreTheme(theme)
        })
}
