use super::*;
use crate::ModelsManagerConfig;
use codex_protocol::openai_models::InputModality;
use pretty_assertions::assert_eq;

#[test]
fn reasoning_summaries_override_true_enables_support() {
    let model = model_info_from_slug("unknown-model");
    let config = ModelsManagerConfig {
        model_supports_reasoning_summaries: Some(true),
        ..Default::default()
    };

    let updated = with_config_overrides(model.clone(), &config);
    let mut expected = model;
    expected.supports_reasoning_summaries = true;

    assert_eq!(updated, expected);
}

#[test]
fn reasoning_summaries_override_false_does_not_disable_support() {
    let mut model = model_info_from_slug("unknown-model");
    model.supports_reasoning_summaries = true;
    let config = ModelsManagerConfig {
        model_supports_reasoning_summaries: Some(false),
        ..Default::default()
    };

    let updated = with_config_overrides(model.clone(), &config);

    assert_eq!(updated, model);
}

#[test]
fn reasoning_summaries_override_false_is_noop_when_model_is_false() {
    let model = model_info_from_slug("unknown-model");
    let config = ModelsManagerConfig {
        model_supports_reasoning_summaries: Some(false),
        ..Default::default()
    };

    let updated = with_config_overrides(model.clone(), &config);

    assert_eq!(updated, model);
}

#[test]
fn model_context_window_override_clamps_to_max_context_window() {
    let mut model = model_info_from_slug("unknown-model");
    model.context_window = Some(273_000);
    model.max_context_window = Some(400_000);
    let config = ModelsManagerConfig {
        model_context_window: Some(500_000),
        ..Default::default()
    };

    let updated = with_config_overrides(model.clone(), &config);
    let mut expected = model;
    expected.context_window = Some(400_000);

    assert_eq!(updated, expected);
}

#[test]
fn model_context_window_uses_model_value_without_override() {
    let mut model = model_info_from_slug("unknown-model");
    model.context_window = Some(273_000);
    model.max_context_window = Some(400_000);
    let config = ModelsManagerConfig::default();

    let updated = with_config_overrides(model.clone(), &config);

    assert_eq!(updated, model);
}

#[test]
fn unknown_model_defaults_to_text_only_input() {
    let model = model_info_from_slug("unknown-model");

    assert_eq!(model.input_modalities, vec![InputModality::Text]);
}

#[test]
fn unknown_model_does_not_guess_context_window() {
    let model = model_info_from_slug("unknown-model");

    assert_eq!(model.context_window, None);
    assert_eq!(model.max_context_window, None);
}

#[test]
fn unknown_model_uses_short_astral_prompt() {
    let model = model_info_from_slug("unknown-model");

    assert!(
        model
            .base_instructions
            .starts_with(DEFAULT_PERSONALITY_HEADER)
    );
    assert!(model.base_instructions.contains("Work from evidence."));
    assert!(model.base_instructions.contains("Report honestly."));
    assert!(!model.base_instructions.contains("Native Tool Flavor"));
    assert!(
        !model
            .base_instructions
            .contains("Use Astral's native tool surface naturally:")
    );
    assert!(!model.base_instructions.contains("Claude-ish"));
    assert!(!model.base_instructions.contains("Codex subagent"));
    assert!(
        !model
            .base_instructions
            .contains("You are a coding agent running in astral-code")
    );
}

#[test]
fn model_input_modalities_override_sets_declared_capabilities() {
    let model = model_info_from_slug("unknown-model");
    let config = ModelsManagerConfig {
        model_input_modalities: Some(vec![InputModality::Text, InputModality::Image]),
        ..Default::default()
    };

    let updated = with_config_overrides(model.clone(), &config);
    let mut expected = model;
    expected.input_modalities = vec![InputModality::Text, InputModality::Image];

    assert_eq!(updated, expected);
}
