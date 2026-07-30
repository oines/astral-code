use std::sync::Arc;

use codex_app_server_protocol::Model;
use codex_app_server_protocol::ModelCapabilities;
use codex_app_server_protocol::ModelCapabilitySource;
use codex_app_server_protocol::ModelServiceTier;
use codex_app_server_protocol::ModelUpgradeInfo;
use codex_app_server_protocol::ReasoningEffortOption;
use codex_core::config::Config;
use codex_login::AuthManager;
use codex_model_provider::create_model_provider;
use codex_models_manager::capabilities::ModelCapability;
use codex_models_manager::manager::RefreshStrategy;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ReasoningEffortPreset;
use std::collections::BTreeSet;
use toml::Value as TomlValue;

pub async fn configured_models(
    config: &Config,
    auth_manager: Arc<AuthManager>,
    model_provider_filter: Option<&str>,
) -> Vec<Model> {
    let mut specs = configured_model_specs(config);
    if let Some(model_provider_filter) = model_provider_filter {
        specs.retain(|(provider_id, _)| provider_id == model_provider_filter);
    }

    let mut models = Vec::new();
    let provider_ids = specs
        .iter()
        .map(|(provider_id, _)| provider_id.clone())
        .chain(model_provider_filter.map(str::to_owned))
        .collect::<BTreeSet<_>>();
    for provider_id in provider_ids {
        let Some(provider) = config.model_providers.get(&provider_id) else {
            continue;
        };
        let provider_name = provider_name(&provider_id, provider.name.as_str());
        let runtime_provider = create_model_provider(provider.clone(), Some(auth_manager.clone()));
        let configured_catalog = (provider_id == config.model_provider_id)
            .then(|| config.model_catalog.clone())
            .flatten();
        let manager =
            runtime_provider.models_manager(config.codex_home.to_path_buf(), configured_catalog);
        let refresh_strategy = if model_provider_filter.is_some() {
            RefreshStrategy::OnlineIfUncached
        } else {
            RefreshStrategy::Offline
        };
        let discovered = manager.raw_model_catalog(refresh_strategy).await;
        let discovered_metadata = discovered
            .models
            .iter()
            .map(|model| (model.slug.clone(), !model.used_fallback_model_metadata))
            .collect::<std::collections::HashMap<_, _>>();
        let mut provider_models = specs
            .iter()
            .filter_map(|(configured_provider, model)| {
                (configured_provider == &provider_id).then_some(model.clone())
            })
            .collect::<BTreeSet<_>>();
        if model_provider_filter.is_some() {
            provider_models.extend(discovered.models.into_iter().map(|model| model.slug));
        }

        let mut manager_config = config.to_models_manager_config();
        manager_config.model_provider_id = Some(provider_id.clone());
        for model_name in provider_models {
            let model_info = manager.get_model_info(&model_name, &manager_config).await;
            let capabilities = effective_capabilities(
                &model_info,
                model_capability(config, &provider_id, &model_name),
                discovered_metadata
                    .get(&model_name)
                    .copied()
                    .unwrap_or(false),
                has_manual_capability(config, &provider_id, &model_name),
            );
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
                capabilities,
            ));
        }
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

fn model_capability<'a>(
    config: &'a Config,
    provider_id: &str,
    model_name: &str,
) -> Option<&'a ModelCapability> {
    let cache = config.model_capabilities.as_ref()?;
    let provider_model = format!("{provider_id}/{model_name}");
    cache
        .models
        .get(&provider_model)
        .or_else(|| cache.lookup(model_name))
}

fn has_manual_capability(config: &Config, provider_id: &str, model_name: &str) -> bool {
    let effective_config = config.config_layer_stack.effective_config();
    let Some(TomlValue::Table(model_capabilities)) = effective_config.get("model_capabilities")
    else {
        return false;
    };
    model_capabilities.keys().any(|model_key| {
        configured_model_spec_from_key(model_key, config.model_provider_id.as_str()).is_some_and(
            |(configured_provider, configured_model)| {
                configured_provider == provider_id && configured_model == model_name
            },
        )
    })
}

fn effective_capabilities(
    model: &ModelInfo,
    configured: Option<&ModelCapability>,
    has_provider_metadata: bool,
    has_manual_capability: bool,
) -> ModelCapabilities {
    let mut sources = Vec::new();
    if has_provider_metadata {
        sources.push(ModelCapabilitySource::Provider);
    }
    if configured.is_some() {
        sources.push(if has_manual_capability {
            ModelCapabilitySource::Manual
        } else {
            ModelCapabilitySource::LiteLlm
        });
    }
    if !has_provider_metadata {
        sources.push(ModelCapabilitySource::Fallback);
    }

    ModelCapabilities {
        context_window: model.resolved_context_window(),
        max_context_window: model.max_context_window,
        max_output_tokens: model.max_output_tokens,
        tool_mode: model.tool_mode,
        supports_tools: configured
            .and_then(|capability| capability.supports_tools)
            .or_else(|| model.tool_mode.map(|_| true)),
        supports_parallel_tools: configured
            .and_then(|capability| capability.supports_parallel_tools)
            .or_else(|| has_provider_metadata.then_some(model.supports_parallel_tool_calls)),
        supports_vision: configured
            .and_then(|capability| capability.supports_vision)
            .or_else(|| {
                has_provider_metadata
                    .then(|| model.input_modalities.contains(&InputModality::Image))
            }),
        supports_prompt_cache: configured.and_then(|capability| capability.supports_prompt_cache),
        supports_reasoning: configured
            .and_then(|capability| capability.supports_reasoning)
            .or_else(|| {
                has_provider_metadata.then_some(!model.supported_reasoning_levels.is_empty())
            }),
        supports_native_streaming: configured
            .and_then(|capability| capability.supports_native_streaming),
        supported_endpoints: configured
            .map(|capability| capability.supported_endpoints.clone())
            .unwrap_or_default(),
        sources,
    }
}

fn model_from_preset(
    preset: ModelPreset,
    model_provider: &str,
    model_provider_name: &str,
    capabilities: ModelCapabilities,
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
        capabilities,
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
