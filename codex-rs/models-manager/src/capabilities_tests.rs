use std::collections::BTreeMap;

use codex_protocol::openai_models::InputModality;
use pretty_assertions::assert_eq;

use super::LiteLlmModelHint;
use super::ModelCapabilitiesCache;
use crate::model_info::model_info_from_slug;

#[test]
fn litellm_registry_is_trimmed_to_astral_capability_cache() {
    let cache = ModelCapabilitiesCache::from_litellm_registry(
        "test".to_string(),
        42,
        BTreeMap::from([(
            "deepseek/deepseek-chat".to_string(),
            LiteLlmModelHint {
                litellm_provider: Some("deepseek".to_string()),
                max_input_tokens: Some(131_072),
                max_output_tokens: Some(8_192),
                supports_function_calling: Some(true),
                supports_parallel_function_calling: Some(true),
                supports_prompt_caching: Some(true),
                supported_endpoints: Some(vec!["/v1/chat/completions".to_string()]),
                ..LiteLlmModelHint::default()
            },
        )]),
    );

    assert_eq!(cache.version, 1);
    assert_eq!(cache.source, "test");
    assert_eq!(cache.generated_at_unix_seconds, 42);
    assert_eq!(
        cache
            .lookup("deepseek-chat")
            .and_then(|cap| cap.max_context_window),
        Some(131_072)
    );
}

#[test]
fn capability_cache_updates_fallback_model_metadata() {
    let cache = ModelCapabilitiesCache::from_litellm_registry(
        "test".to_string(),
        42,
        BTreeMap::from([(
            "provider/vision-model".to_string(),
            LiteLlmModelHint {
                max_input_tokens: Some(65_536),
                supports_vision: Some(true),
                supports_parallel_function_calling: Some(true),
                supports_reasoning: Some(true),
                ..LiteLlmModelHint::default()
            },
        )]),
    );
    let mut model = model_info_from_slug("vision-model");

    cache
        .lookup("vision-model")
        .expect("capability exists")
        .apply_to_model_info(&mut model);

    assert_eq!(model.context_window, Some(65_536));
    assert_eq!(model.max_context_window, Some(65_536));
    assert_eq!(
        model.input_modalities,
        vec![InputModality::Text, InputModality::Image]
    );
    assert!(model.supports_parallel_tool_calls);
    assert!(!model.supported_reasoning_levels.is_empty());
}
