use std::sync::Arc;

use codex_app_server_protocol::Model;
use codex_app_server_protocol::ModelServiceTier;
use codex_app_server_protocol::ModelUpgradeInfo;
use codex_app_server_protocol::ReasoningEffortOption;
use codex_core::ThreadManager;
use codex_models_manager::manager::RefreshStrategy;
use codex_models_manager::manager::SharedModelsManager;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ReasoningEffortPreset;

pub async fn supported_models(
    thread_manager: Arc<ThreadManager>,
    model_provider: String,
    model_provider_name: String,
    include_hidden: bool,
) -> Vec<Model> {
    supported_models_from_manager(
        thread_manager.get_models_manager(),
        model_provider,
        model_provider_name,
        include_hidden,
        RefreshStrategy::Offline,
    )
    .await
}

pub async fn supported_models_from_manager(
    models_manager: SharedModelsManager,
    model_provider: String,
    model_provider_name: String,
    include_hidden: bool,
    refresh_strategy: RefreshStrategy,
) -> Vec<Model> {
    models_manager
        .list_models(refresh_strategy)
        .await
        .into_iter()
        .filter(|preset| include_hidden || preset.show_in_picker)
        .map(|preset| model_from_preset(preset, &model_provider, &model_provider_name))
        .collect()
}

fn model_from_preset(
    preset: ModelPreset,
    model_provider: &str,
    model_provider_name: &str,
) -> Model {
    Model {
        model_provider: preset
            .model_provider
            .unwrap_or_else(|| model_provider.to_string()),
        model_provider_name: preset
            .model_provider_name
            .unwrap_or_else(|| model_provider_name.to_string()),
        id: preset.id.to_string(),
        model: preset.model.to_string(),
        upgrade: preset.upgrade.as_ref().map(|upgrade| upgrade.id.clone()),
        upgrade_info: preset.upgrade.as_ref().map(|upgrade| ModelUpgradeInfo {
            model: upgrade.id.clone(),
            upgrade_copy: upgrade.upgrade_copy.clone(),
            model_link: upgrade.model_link.clone(),
            migration_markdown: upgrade.migration_markdown.clone(),
        }),
        availability_nux: preset.availability_nux.map(Into::into),
        display_name: preset.display_name.to_string(),
        description: preset.description.to_string(),
        hidden: !preset.show_in_picker,
        supported_reasoning_efforts: reasoning_efforts_from_preset(
            preset.supported_reasoning_efforts,
        ),
        default_reasoning_effort: preset.default_reasoning_effort,
        input_modalities: preset.input_modalities,
        supports_personality: preset.supports_personality,
        additional_speed_tiers: preset.additional_speed_tiers,
        service_tiers: preset
            .service_tiers
            .into_iter()
            .map(|service_tier| ModelServiceTier {
                id: service_tier.id,
                name: service_tier.name,
                description: service_tier.description,
            })
            .collect(),
        default_service_tier: preset.default_service_tier,
        is_default: preset.is_default,
    }
}

fn reasoning_efforts_from_preset(
    efforts: Vec<ReasoningEffortPreset>,
) -> Vec<ReasoningEffortOption> {
    efforts
        .into_iter()
        .map(|preset| ReasoningEffortOption {
            reasoning_effort: preset.effort,
            description: preset.description,
        })
        .collect()
}
