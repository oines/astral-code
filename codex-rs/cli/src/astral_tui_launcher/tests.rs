use std::path::PathBuf;

use codex_app_server_protocol::ThreadTokenUsage;
use codex_app_server_protocol::TokenUsageBreakdown;
use codex_app_server_protocol::UserInput;
use codex_config::types::UiVariant;
use pretty_assertions::assert_eq;

use super::initial_input;
use super::selected_ui_variant;
use super::token_usage_from_astral;

#[test]
fn explicit_ui_overrides_configured_variant() {
    assert_eq!(
        selected_ui_variant(Some(UiVariant::Astral), UiVariant::Classic),
        UiVariant::Astral
    );
    assert_eq!(
        selected_ui_variant(None, UiVariant::Classic),
        UiVariant::Classic
    );
}

#[test]
fn initial_input_keeps_images_before_prompt() {
    let input = initial_input(
        Some("explain these".to_string()),
        vec![PathBuf::from("first.png"), PathBuf::from("second.png")],
    );

    assert_eq!(
        input,
        vec![
            UserInput::LocalImage {
                path: PathBuf::from("first.png"),
                detail: None,
            },
            UserInput::LocalImage {
                path: PathBuf::from("second.png"),
                detail: None,
            },
            UserInput::Text {
                text: "explain these".to_string(),
                text_elements: Vec::new(),
            },
        ]
    );
}

#[test]
fn astral_token_usage_maps_to_cli_exit_summary() {
    let token_usage = token_usage_from_astral(&ThreadTokenUsage {
        total: TokenUsageBreakdown {
            total_tokens: 14_000,
            input_tokens: 10_000,
            cached_input_tokens: 4_000,
            output_tokens: 3_000,
            reasoning_output_tokens: 1_000,
        },
        last: TokenUsageBreakdown {
            total_tokens: 8_000,
            input_tokens: 6_000,
            cached_input_tokens: 2_000,
            output_tokens: 1_500,
            reasoning_output_tokens: 500,
        },
        model_context_window: Some(200_000),
    });

    assert_eq!(
        token_usage,
        codex_tui::TokenUsage {
            input_tokens: 10_000,
            cached_input_tokens: 4_000,
            output_tokens: 3_000,
            reasoning_output_tokens: 1_000,
            total_tokens: 14_000,
        }
    );
}
