use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ModelsResponse;

use crate::capabilities::ModelCapabilitiesCache;
use crate::capabilities::ModelCapability;

#[derive(Debug, Clone, Default)]
pub struct ModelsManagerConfig {
    pub model_provider_id: Option<String>,
    pub model_context_window: Option<i64>,
    pub model_input_modalities: Option<Vec<InputModality>>,
    pub model_auto_compact_token_limit: Option<i64>,
    pub tool_output_token_limit: Option<usize>,
    pub base_instructions: Option<String>,
    pub personality_enabled: bool,
    pub model_supports_reasoning_summaries: Option<bool>,
    pub model_catalog: Option<ModelsResponse>,
    pub model_capabilities: Option<ModelCapabilitiesCache>,
    pub model_capability_overrides: Option<ModelCapabilitiesCache>,
}

impl ModelsManagerConfig {
    pub(crate) fn lookup_model_capability_override(&self, model: &str) -> Option<&ModelCapability> {
        let overrides = self.model_capability_overrides.as_ref()?;
        if let Some(provider_id) = self.model_provider_id.as_ref() {
            let provider_model = format!("{provider_id}/{model}");
            return overrides.models.get(&provider_model);
        }
        None
    }

    pub(crate) fn lookup_model_capability_fallback(&self, model: &str) -> Option<&ModelCapability> {
        if let Some(overrides) = self.model_capability_overrides.as_ref()
            && let Some(capability) = overrides.models.get(model)
        {
            return Some(capability);
        }
        let cache = self.model_capabilities.as_ref()?;
        if let Some(provider_id) = self.model_provider_id.as_ref() {
            let provider_model = format!("{provider_id}/{model}");
            if let Some(capability) = cache.models.get(&provider_model) {
                return Some(capability);
            }
        }
        cache.lookup(model)
    }

    pub(crate) fn has_model_capability(&self, model: &str) -> bool {
        self.lookup_model_capability_override(model).is_some()
            || self.lookup_model_capability_fallback(model).is_some()
    }
}
