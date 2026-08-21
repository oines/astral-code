use codex_app_server_protocol::Model;
use codex_app_server_protocol::ModelCapabilities;
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
    assert_eq!(suggestions[0].display, "Codex 5.2 · OpenAI (current)");
    assert_eq!(suggestions[0].insert_text, "/model openai/gpt-5.2 ");
}

#[test]
fn effort_phase_chains_from_the_selected_model() {
    let catalog = catalog();
    let suggestions = catalog.suggestions("Codex 5.2 xh");
    assert_eq!(suggestions[0].insert_text, "/model openai/gpt-5.2 xhigh");
    assert!(!catalog.is_complete_selection("Codex 5.2"));
    assert!(!catalog.is_complete_selection("Codex 5.2 "));
    assert!(catalog.is_complete_selection("Codex 5.2 xhigh"));

    let suggestions = catalog.effort_suggestions("hi");
    assert_eq!(suggestions[0].display, "high (active)");
    assert_eq!(suggestions[0].insert_text, "/effort high");
    assert_eq!(
        catalog.resolve_effort("xhigh"),
        Ok(selection(ReasoningEffort::XHigh))
    );
}

#[test]
fn current_reasoning_effort_restores_missing_provider_options() {
    let mut current = model("deepseek-v4-pro", "DeepSeek V4 Pro", Vec::new());
    current.default_reasoning_effort = ReasoningEffort::None;
    current.capabilities.supports_reasoning = Some(false);
    let mut flash = model("deepseek-v4-flash", "DeepSeek V4 Flash", Vec::new());
    flash.default_reasoning_effort = ReasoningEffort::None;
    flash.capabilities.supports_reasoning = Some(false);
    let mut catalog = ModelCatalog::default();
    catalog.replace(
        vec![current, flash],
        "deepseek-v4-pro",
        "openai",
        Some(ReasoningEffort::XHigh),
    );

    assert_eq!(
        catalog.suggestions(""),
        vec![
            super::ModelSuggestion {
                display: "DeepSeek V4 Pro · OpenAI (current)".to_string(),
                description: "General coding model".to_string(),
                insert_text: "/model openai/deepseek-v4-pro ".to_string(),
            },
            super::ModelSuggestion {
                display: "DeepSeek V4 Flash · OpenAI".to_string(),
                description: "General coding model".to_string(),
                insert_text: "/model openai/deepseek-v4-flash ".to_string(),
            },
        ]
    );
    assert_eq!(
        catalog
            .suggestions("DeepSeek V4 Pro ")
            .into_iter()
            .map(|suggestion| (suggestion.display, suggestion.insert_text))
            .collect::<Vec<_>>(),
        vec![
            (
                "xhigh (active)".to_string(),
                "/model openai/deepseek-v4-pro xhigh".to_string(),
            ),
            (
                "high".to_string(),
                "/model openai/deepseek-v4-pro high".to_string(),
            ),
            (
                "medium".to_string(),
                "/model openai/deepseek-v4-pro medium".to_string(),
            ),
            (
                "low".to_string(),
                "/model openai/deepseek-v4-pro low".to_string(),
            ),
        ]
    );
    assert_eq!(
        catalog
            .effort_suggestions("")
            .into_iter()
            .map(|suggestion| (suggestion.display, suggestion.insert_text))
            .collect::<Vec<_>>(),
        vec![
            ("xhigh (active)".to_string(), "/effort xhigh".to_string()),
            ("high".to_string(), "/effort high".to_string()),
            ("medium".to_string(), "/effort medium".to_string()),
            ("low".to_string(), "/effort low".to_string()),
        ]
    );
    assert_eq!(
        catalog.resolve_effort("high"),
        Ok(ModelSelection {
            model: "deepseek-v4-pro".to_string(),
            model_provider: "openai".to_string(),
            display_name: "DeepSeek V4 Pro".to_string(),
            effort: ReasoningEffort::High,
        })
    );

    catalog.update_current("deepseek-v4-flash", "openai", Some(ReasoningEffort::None));
    assert_eq!(
        catalog
            .suggestions("DeepSeek V4 Flash ")
            .into_iter()
            .map(|suggestion| (suggestion.display, suggestion.insert_text))
            .collect::<Vec<_>>(),
        vec![(
            "none (active)".to_string(),
            "/model openai/deepseek-v4-flash none".to_string(),
        )]
    );
    assert_eq!(
        catalog
            .suggestions("DeepSeek V4 Pro ")
            .into_iter()
            .map(|suggestion| suggestion.insert_text)
            .collect::<Vec<_>>(),
        vec![
            "/model openai/deepseek-v4-pro xhigh".to_string(),
            "/model openai/deepseek-v4-pro high".to_string(),
            "/model openai/deepseek-v4-pro medium".to_string(),
            "/model openai/deepseek-v4-pro low".to_string(),
        ]
    );
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

#[test]
fn duplicate_model_names_require_provider_qualification() {
    let openai = model("shared-model", "Shared Model", vec![ReasoningEffort::High]);
    let mut codex = openai.clone();
    codex.model_provider = "codex".to_string();
    codex.model_provider_name = "Codex".to_string();
    let mut catalog = ModelCatalog::default();
    catalog.replace(
        vec![openai, codex],
        "shared-model",
        "openai",
        Some(ReasoningEffort::High),
    );

    assert_eq!(
        catalog.resolve("shared-model"),
        Err(ModelResolveError::AmbiguousModel(
            "shared-model".to_string()
        ))
    );
    assert_eq!(
        catalog.resolve("codex/shared-model"),
        Ok(ModelSelection {
            model: "shared-model".to_string(),
            model_provider: "codex".to_string(),
            display_name: "Shared Model".to_string(),
            effort: ReasoningEffort::High,
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
        Some(ReasoningEffort::High),
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
        capabilities: ModelCapabilities::default(),
        is_default: true,
    }
}
