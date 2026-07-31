use codex_app_server_protocol::ConfigReadResponse;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use serde_json::Value;
use serde_json::json;

use super::Category;
use super::SettingsData;
use super::SettingsFocus;
use super::SettingsInput;
use super::SettingsState;
use super::handle_key;
use super::handle_paste;
use super::render;
use crate::view::AstralTheme;
use crate::view::AstralThemeId;

#[test]
fn settings_navigation_surfaces_snapshot() {
    let mut state = state();
    let root = render_state(&mut state, 96, 26);

    state.apply_focus(SettingsFocus::Category(Category::Tools));
    let tools = render_state(&mut state, 96, 26);

    state.apply_focus(SettingsFocus::Search);
    let search = render_state(&mut state, 96, 26);

    state.apply_focus(SettingsFocus::SessionMemoryTemplates);
    let memory = render_state(&mut state, 96, 26);

    state.apply_focus(SettingsFocus::Root);
    let narrow = render_state(&mut state, 52, 16);

    insta::assert_snapshot!(format!(
        "ROOT\n{root}\n\nTOOLS\n{tools}\n\nSEARCH\n{search}\n\nMEMORY\n{memory}\n\nNARROW\n{narrow}"
    ));
}

#[test]
fn search_secret_replacement_is_atomic_and_never_writes_redaction_marker() {
    let mut state = state();
    state.apply_focus(SettingsFocus::Search);

    press(&mut state, KeyCode::Down);
    press(&mut state, KeyCode::Enter);
    press(&mut state, KeyCode::Down);
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        handle_paste(&mut state, "new-secret"),
        SettingsInput::Redraw
    );
    press(&mut state, KeyCode::Enter);
    press(&mut state, KeyCode::End);
    let SettingsInput::Write { write, .. } = handle_key(
        &mut state,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    ) else {
        panic!("save should produce one config write");
    };

    assert_eq!(write.params.edits.len(), 1);
    let edit = &write.params.edits[0];
    assert_eq!(edit.key_path, "tools.web_search.api_key");
    assert_eq!(edit.value, Value::String("new-secret".to_string()));
    assert_ne!(edit.value, Value::String("[redacted]".to_string()));
}

#[test]
fn session_memory_inline_source_clears_the_file_source_atomically() {
    let mut state = state();
    state.apply_focus(SettingsFocus::SessionMemoryTemplates);

    press(&mut state, KeyCode::Enter);
    press(&mut state, KeyCode::Up);
    press(&mut state, KeyCode::Enter);
    press(&mut state, KeyCode::Enter);
    press(&mut state, KeyCode::Down);
    press(&mut state, KeyCode::Enter);
    press(&mut state, KeyCode::Down);
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        handle_paste(&mut state, "Summarize decisions and current work."),
        SettingsInput::Redraw
    );
    press(&mut state, KeyCode::Enter);
    press(&mut state, KeyCode::End);
    let SettingsInput::Write { write, .. } = handle_key(
        &mut state,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    ) else {
        panic!("save should produce one atomic config write");
    };

    assert_eq!(
        write
            .params
            .edits
            .iter()
            .map(|edit| (&edit.key_path, &edit.value))
            .collect::<Vec<_>>(),
        vec![
            (
                &"session_memory_template".to_string(),
                &Value::String("Summarize decisions and current work.".to_string()),
            ),
            (
                &"experimental_session_memory_template_file".to_string(),
                &Value::Null,
            ),
        ]
    );
}

#[test]
fn choosing_a_default_provider_resets_the_model_override_in_the_same_write() {
    let mut state = state();
    state.apply_focus(SettingsFocus::Category(Category::Models));

    press(&mut state, KeyCode::Down);
    press(&mut state, KeyCode::Enter);
    let SettingsInput::Write { write, .. } = handle_key(
        &mut state,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    ) else {
        panic!("provider selection should write config");
    };

    assert_eq!(
        write
            .params
            .edits
            .iter()
            .map(|edit| (&edit.key_path, &edit.value))
            .collect::<Vec<_>>(),
        vec![
            (
                &"model_provider".to_string(),
                &Value::String("deepseek".to_string()),
            ),
            (&"model".to_string(), &Value::Null),
        ]
    );
}

fn press(state: &mut SettingsState, code: KeyCode) {
    let _ = handle_key(state, KeyEvent::new(code, KeyModifiers::NONE));
}

fn state() -> SettingsState {
    SettingsState::new(
        1,
        SettingsData {
            config: config(),
            models: Vec::new(),
            features: Vec::new(),
            permission_profiles: Vec::new(),
            requirements: None,
        },
        "deepseek".to_string(),
        "deepseek-chat".to_string(),
        AstralThemeId::Day,
    )
}

fn config() -> ConfigReadResponse {
    serde_json::from_value(json!({
        "config": {
            "model_provider": "deepseek",
            "model": "deepseek-chat",
            "model_reasoning_effort": "high",
            "model_providers": {
                "deepseek": {
                    "name": "DeepSeek",
                    "base_url": "https://api.deepseek.com/v1",
                    "env_key": "DEEPSEEK_API_KEY",
                    "wire_api": "chat_completions"
                }
            },
            "web_search": "live",
            "tools": {
                "surface": "claude",
                "web_search": {
                    "provider": "tavily",
                    "api_key": "[redacted]",
                    "context_size": "medium",
                    "allowed_domains": ["docs.rs", "github.com"],
                    "location": {
                        "country": "SG",
                        "city": "Singapore",
                        "timezone": "Asia/Singapore"
                    }
                }
            },
            "experimental_session_memory_compact": false,
            "session_memory_minimum_message_tokens_to_init": 100000,
            "session_memory_minimum_tokens_between_update": 20000,
            "session_memory_tool_calls_between_updates": 10,
            "session_memory_template": "Keep decisions and active work.",
            "memories": {
                "compact_memory": "enqueue",
                "generate_memories": true,
                "use_memories": true
            },
            "tui": {
                "theme": "day",
                "animations": true
            }
        },
        "origins": {
            "tools.surface": {
                "name": {
                    "type": "project",
                    "dotCodexFolder": "/workspace/.astral-code"
                },
                "version": "project-v1"
            }
        },
        "layers": [
            {
                "name": {
                    "type": "user",
                    "file": "/Users/test/.astral-code/config.toml",
                    "profile": null
                },
                "version": "user-v1",
                "config": {
                    "model_provider": "deepseek",
                    "model": "deepseek-chat",
                    "model_providers": {
                        "deepseek": {
                            "name": "DeepSeek",
                            "base_url": "https://api.deepseek.com/v1",
                            "wire_api": "chat_completions"
                        }
                    },
                    "tools": {
                        "surface": "claude",
                        "web_search": {
                            "provider": "tavily",
                            "api_key": "[redacted]",
                            "context_size": "medium",
                            "allowed_domains": ["docs.rs", "github.com"],
                            "location": {
                                "country": "SG",
                                "city": "Singapore",
                                "timezone": "Asia/Singapore"
                            }
                        }
                    },
                    "session_memory_template": "Keep decisions and active work."
                }
            },
            {
                "name": {
                    "type": "project",
                    "dotCodexFolder": "/workspace/.astral-code"
                },
                "version": "project-v1",
                "config": {
                    "tools": {
                        "surface": "codex"
                    }
                }
            }
        ]
    }))
    .expect("valid config response")
}

fn render_state(state: &mut SettingsState, width: u16, height: u16) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    render(
        state,
        area,
        &mut buffer,
        AstralTheme::for_id(AstralThemeId::Day),
    );
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}
