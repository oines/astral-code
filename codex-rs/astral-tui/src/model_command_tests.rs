use codex_app_server_protocol::Model;
use codex_app_server_protocol::ReasoningEffortOption;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;

use super::ModelCatalog;
use super::ModelResolveError;
use super::ModelSelection;

#[test]
fn model_phase_filters_and_marks_the_current_model() {
    let catalog = catalog();
    let suggestions = catalog.suggestions("cod");
    assert_eq!(suggestions[0].display, "Codex 5.2 (current)");
    assert_eq!(suggestions[0].insert_text, "/model Codex 5.2 ");
}

#[test]
fn effort_phase_chains_from_the_selected_model() {
    let suggestions = catalog().suggestions("Codex 5.2 xh");
    assert_eq!(suggestions[0].insert_text, "/model Codex 5.2 xhigh");
}

#[test]
fn resolver_accepts_display_name_model_id_and_effort() {
    let catalog = catalog();
    assert_eq!(
        catalog.resolve("codex 5.2 xhigh"),
        Ok(selection(ReasoningEffort::XHigh))
    );
    assert_eq!(
        catalog.resolve("gpt-5.2"),
        Ok(selection(ReasoningEffort::High))
    );
    assert_eq!(
        catalog.resolve("Codex 5.2 impossible"),
        Err(ModelResolveError::UnsupportedEffort {
            model: "Codex 5.2".to_string(),
            effort: "impossible".to_string(),
        })
    );
}

fn selection(effort: ReasoningEffort) -> ModelSelection {
    ModelSelection {
        model: "gpt-5.2".to_string(),
        model_provider: "openai".to_string(),
        display_name: "Codex 5.2".to_string(),
        effort,
    }
}

fn catalog() -> ModelCatalog {
    let mut catalog = ModelCatalog::default();
    catalog.replace(
        vec![model(
            "gpt-5.2",
            "Codex 5.2",
            vec![ReasoningEffort::High, ReasoningEffort::XHigh],
        )],
        "gpt-5.2",
        "openai",
    );
    catalog
}

fn model(model: &str, display_name: &str, efforts: Vec<ReasoningEffort>) -> Model {
    Model {
        model_provider: "openai".to_string(),
        model_provider_name: "OpenAI".to_string(),
        id: model.to_string(),
        model: model.to_string(),
        upgrade: None,
        upgrade_info: None,
        availability_nux: None,
        display_name: display_name.to_string(),
        description: "General coding model".to_string(),
        hidden: false,
        supported_reasoning_efforts: efforts
            .into_iter()
            .map(|reasoning_effort| ReasoningEffortOption {
                reasoning_effort,
                description: "Reasoning level".to_string(),
            })
            .collect(),
        default_reasoning_effort: ReasoningEffort::High,
        input_modalities: Vec::new(),
        supports_personality: true,
        additional_speed_tiers: Vec::new(),
        service_tiers: Vec::new(),
        default_service_tier: None,
        is_default: true,
    }
}
