use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::Model;
use codex_app_server_protocol::ModelCapabilities;
use codex_app_server_protocol::ModelCapabilitySource;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ToolMode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use serde_json::json;

use super::ModelsManagerState;
use super::render;
use crate::view::AstralTheme;

#[test]
fn provider_hierarchy_snapshot() {
    let config: ConfigReadResponse = serde_json::from_value(json!({
        "config": {
            "model_providers": {
                "deepseek": {
                    "name": "DeepSeek",
                    "base_url": "https://api.deepseek.com/v1",
                    "env_key": "DEEPSEEK_API_KEY",
                    "wire_api": "chat_completions"
                },
                "anthropic": {
                    "name": "Anthropic",
                    "base_url": "https://api.anthropic.com",
                    "env_key": "ANTHROPIC_API_KEY",
                    "wire_api": "anthropic_messages"
                }
            }
        },
        "origins": {},
        "layers": null
    }))
    .expect("valid config response");
    let models = vec![Model {
        model_provider: "deepseek".to_string(),
        model_provider_name: "DeepSeek".to_string(),
        id: "deepseek-chat".to_string(),
        model: "deepseek-chat".to_string(),
        upgrade: None,
        upgrade_info: None,
        availability_nux: None,
        display_name: "DeepSeek Chat".to_string(),
        description: "General-purpose chat model".to_string(),
        hidden: false,
        supported_reasoning_efforts: Vec::new(),
        default_reasoning_effort: ReasoningEffort::High,
        input_modalities: vec![InputModality::Text],
        supports_personality: false,
        additional_speed_tiers: Vec::new(),
        service_tiers: Vec::new(),
        default_service_tier: None,
        capabilities: ModelCapabilities {
            context_window: Some(128_000),
            max_context_window: Some(128_000),
            max_output_tokens: Some(8_192),
            tool_mode: Some(ToolMode::Direct),
            supports_tools: Some(true),
            supports_parallel_tools: Some(true),
            supports_vision: Some(false),
            supports_prompt_cache: Some(true),
            supports_reasoning: Some(false),
            supports_native_streaming: Some(true),
            supported_endpoints: vec!["/v1/chat/completions".to_string()],
            sources: vec![ModelCapabilitySource::Provider],
        },
        is_default: true,
    }];
    let mut state = ModelsManagerState::new(
        1,
        config,
        models,
        "deepseek".to_string(),
        "deepseek-chat".to_string(),
    );

    let collapsed = render_state(&mut state);
    let _ = state.activate(0);
    let expanded = render_state(&mut state);

    insta::assert_snapshot!(format!("COLLAPSED\n{collapsed}\n\nEXPANDED\n{expanded}"));
}

fn render_state(state: &mut ModelsManagerState) -> String {
    let area = Rect::new(0, 0, 100, 28);
    let mut buffer = Buffer::empty(area);
    render(state, area, &mut buffer, AstralTheme::default());
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
