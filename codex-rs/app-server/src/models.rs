use std::sync::Arc;

use codex_app_server_protocol::Model;
use codex_app_server_protocol::ModelServiceTier;
use codex_app_server_protocol::ModelUpgradeInfo;
use codex_app_server_protocol::ReasoningEffortOption;
use codex_core::ThreadManager;
use codex_core::config::Config;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ReasoningEffortPreset;
use std::collections::BTreeSet;
use toml::Value as TomlValue;

pub async fn configured_models(
    thread_manager: Arc<ThreadManager>,
    config: &Config,
    model_provider_filter: Option<&str>,
) -> Vec<Model> {
    let mut specs = configured_model_specs(config);
    if let Some(model_provider_filter) = model_provider_filter {
        specs.retain(|(provider_id, _)| provider_id == model_provider_filter);
    }

    let manager_config = config.to_models_manager_config();
    let mut models = Vec::new();
    for (provider_id, model_name) in specs {
        let Some(provider) = config.model_providers.get(&provider_id) else {
            continue;
        };
        let model_info_key = format!("{provider_id}/{model_name}");
        let model_info = thread_manager
            .get_models_manager()
            .get_model_info(&model_info_key, &manager_config)
            .await;
        let provider_name = provider_name(&provider_id, provider.name.as_str());
        let mut preset = ModelPreset::from(model_info);
        preset.id.clone_from(&model_name);
        preset.model.clone_from(&model_name);
        preset.model_provider = Some(provider_id.clone());
        preset.model_provider_name = Some(provider_name.clone());
        preset.is_default = config.model.as_deref() == Some(model_name.as_str())
            && provider_id == config.model_provider_id;
        preset.show_in_picker = true;
        models.push(model_from_preset(
            preset,
            provider_id.as_str(),
            provider_name.as_str(),
        ));
    }

    models.sort_by(|left, right| {
        right.is_default.cmp(&left.is_default).then_with(|| {
            left.model_provider_name
                .cmp(&right.model_provider_name)
                .then_with(|| left.model_provider.cmp(&right.model_provider))
                .then_with(|| left.model.cmp(&right.model))
        })
    });
    models
}

fn configured_model_specs(config: &Config) -> Vec<(String, String)> {
    let mut specs = BTreeSet::new();
    if let Some(model_name) = config.model.as_ref() {
        specs.insert((config.model_provider_id.clone(), model_name.clone()));
    }

    if let Some(TomlValue::Table(model_capabilities)) = config
        .config_layer_stack
        .effective_config()
        .get("model_capabilities")
    {
        for model_key in model_capabilities.keys() {
            if let Some(spec) =
                configured_model_spec_from_key(model_key, config.model_provider_id.as_str())
            {
                specs.insert(spec);
            }
        }
    }

    specs.into_iter().collect()
}

fn configured_model_spec_from_key(
    model_key: &str,
    default_provider_id: &str,
) -> Option<(String, String)> {
    let (provider_id, model_name) = model_key.split_once('/').map_or(
        (default_provider_id, model_key),
        |(provider_id, model_name)| (provider_id, model_name),
    );
    let provider_id = provider_id.trim();
    let model_name = model_name.trim();
    if provider_id.is_empty() || model_name.is_empty() {
        return None;
    }
    Some((provider_id.to_string(), model_name.to_string()))
}

fn provider_name(provider_id: &str, configured_name: &str) -> String {
    if configured_name.is_empty() {
        provider_id.to_string()
    } else {
        configured_name.to_string()
    }
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
