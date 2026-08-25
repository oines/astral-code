use std::collections::BTreeMap;

use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::openai_models::ToolMode;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde_json::Value;

pub const MODEL_CAPABILITIES_FILE_NAME: &str = "model-capabilities.toml";

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct ModelCapabilitiesCache {
    pub version: u32,
    pub source: String,
    pub generated_at_unix_seconds: u64,
    #[serde(default)]
    pub models: BTreeMap<String, ModelCapability>,
}

impl ModelCapabilitiesCache {
    pub fn from_litellm_registry(
        source: String,
        generated_at_unix_seconds: u64,
        registry: BTreeMap<String, LiteLlmModelHint>,
    ) -> Self {
        let models = registry
            .into_iter()
            .filter_map(|(model, hint)| {
                if !hint.is_language_model_hint() {
                    return None;
                }
                let capability = ModelCapability::from_litellm_hint(hint);
                capability.has_signal().then_some((model, capability))
            })
            .collect();
        Self {
            version: 1,
            source,
            generated_at_unix_seconds,
            models,
        }
    }

    pub fn lookup(&self, model: &str) -> Option<&ModelCapability> {
        self.models
            .get(model)
            .or_else(|| self.lookup_by_namespaced_suffix(model))
            .or_else(|| self.lookup_by_litellm_suffix(model))
    }

    fn lookup_by_namespaced_suffix(&self, model: &str) -> Option<&ModelCapability> {
        let (_, suffix) = model.split_once('/')?;
        self.models.get(suffix)
    }

    fn lookup_by_litellm_suffix(&self, model: &str) -> Option<&ModelCapability> {
        let suffix = format!("/{model}");
        let mut matches = self
            .models
            .iter()
            .filter_map(|(key, capability)| key.ends_with(&suffix).then_some(capability));
        let capability = matches.next()?;
        matches.next().is_none().then_some(capability)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct ModelCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub litellm_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_mode: Option<ToolMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_tools: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_parallel_tools: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_vision: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_web_search: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_image_generation: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_prompt_cache: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_reasoning: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_native_streaming: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_endpoints: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pricing: Option<ModelPricing>,
}

impl ModelCapability {
    fn from_litellm_hint(hint: LiteLlmModelHint) -> Self {
        let pricing = ModelPricing::from_litellm_hint(&hint);
        let supports_vision = hint.supports_vision.or_else(|| {
            hint.input_modalities.as_ref().map(|modalities| {
                modalities
                    .iter()
                    .any(|modality| modality.eq_ignore_ascii_case("image"))
            })
        });
        Self {
            litellm_provider: hint.litellm_provider,
            mode: hint.mode,
            tool_mode: None,
            context_window: hint.max_input_tokens,
            max_context_window: hint.max_input_tokens,
            max_output_tokens: hint.max_output_tokens.or(hint.max_tokens),
            supports_tools: hint.supports_function_calling,
            supports_parallel_tools: hint.supports_parallel_function_calling,
            supports_vision,
            supports_web_search: None,
            supports_image_generation: None,
            supports_prompt_cache: hint.supports_prompt_caching,
            supports_reasoning: hint.supports_reasoning,
            supports_native_streaming: hint.supports_native_streaming,
            supported_endpoints: hint.supported_endpoints.unwrap_or_default(),
            pricing,
        }
    }

    pub fn apply_fallback_to_model_info(&self, model: &mut ModelInfo) {
        if model.tool_mode.is_none()
            && let Some(tool_mode) = self.tool_mode
        {
            model.tool_mode = Some(tool_mode);
        }

        if model.context_window.is_none() {
            model.context_window = self
                .context_window
                .or(self.max_context_window)
                .and_then(u64_to_i64);
        }
        if model.max_context_window.is_none() {
            model.max_context_window = self.max_context_window.and_then(u64_to_i64);
        }
        if let Some(max_output_tokens) = self.max_output_tokens.and_then(u64_to_i64)
            && model.max_output_tokens.is_none()
        {
            model.max_output_tokens = Some(max_output_tokens);
        }

        if model.used_fallback_model_metadata {
            if let Some(supports_parallel_tools) = self.supports_parallel_tools {
                model.supports_parallel_tool_calls = supports_parallel_tools;
            }
            if let Some(supports_vision) = self.supports_vision {
                set_vision_support(model, supports_vision);
            }
            if let Some(supports_web_search) = self.supports_web_search {
                model.supports_web_search = supports_web_search;
            }
            if let Some(supports_image_generation) = self.supports_image_generation {
                model.supports_image_generation = supports_image_generation;
            }
        }

        if self.supports_reasoning == Some(true) && model.supported_reasoning_levels.is_empty() {
            set_default_reasoning_support(model);
        }
    }

    pub fn apply_override_to_model_info(&self, model: &mut ModelInfo) {
        if let Some(tool_mode) = self.tool_mode {
            model.tool_mode = Some(tool_mode);
        }
        if let Some(context_window) = self.context_window.and_then(u64_to_i64) {
            model.context_window = Some(context_window);
        }
        if let Some(max_context_window) = self.max_context_window.and_then(u64_to_i64) {
            model.max_context_window = Some(max_context_window);
        }
        if let Some(max_output_tokens) = self.max_output_tokens.and_then(u64_to_i64) {
            model.max_output_tokens = Some(max_output_tokens);
        }
        if let Some(supports_parallel_tools) = self.supports_parallel_tools {
            model.supports_parallel_tool_calls = supports_parallel_tools;
        }
        if let Some(supports_vision) = self.supports_vision {
            set_vision_support(model, supports_vision);
        }
        if let Some(supports_web_search) = self.supports_web_search {
            model.supports_web_search = supports_web_search;
        }
        if let Some(supports_image_generation) = self.supports_image_generation {
            model.supports_image_generation = supports_image_generation;
        }
        match self.supports_reasoning {
            Some(true) if model.supported_reasoning_levels.is_empty() => {
                set_default_reasoning_support(model);
            }
            Some(false) => {
                model.default_reasoning_level = None;
                model.supported_reasoning_levels.clear();
            }
            Some(true) | None => {}
        }
    }

    fn has_signal(&self) -> bool {
        self.litellm_provider.is_some()
            || self.mode.is_some()
            || self.tool_mode.is_some()
            || self.context_window.is_some()
            || self.max_context_window.is_some()
            || self.max_output_tokens.is_some()
            || self.supports_tools.is_some()
            || self.supports_parallel_tools.is_some()
            || self.supports_vision.is_some()
            || self.supports_web_search.is_some()
            || self.supports_image_generation.is_some()
            || self.supports_prompt_cache.is_some()
            || self.supports_reasoning.is_some()
            || self.supports_native_streaming.is_some()
            || !self.supported_endpoints.is_empty()
            || self.pricing.is_some()
    }
}

fn set_vision_support(model: &mut ModelInfo, supports_vision: bool) {
    model.input_modalities = if supports_vision {
        vec![InputModality::Text, InputModality::Image]
    } else {
        vec![InputModality::Text]
    };
}

fn set_default_reasoning_support(model: &mut ModelInfo) {
    model.supported_reasoning_levels = vec![
        ReasoningEffortPreset {
            effort: ReasoningEffort::High,
            description: "High".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::Custom("max".to_string()),
            description: "Max".to_string(),
        },
    ];
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct ModelPricing {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_cost_per_token: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_cost_per_token: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_token_cost: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_token_cost: Option<f64>,
}

impl ModelPricing {
    fn from_litellm_hint(hint: &LiteLlmModelHint) -> Option<Self> {
        let pricing = Self {
            input_cost_per_token: hint.input_cost_per_token,
            output_cost_per_token: hint.output_cost_per_token,
            cache_creation_input_token_cost: hint.cache_creation_input_token_cost,
            cache_read_input_token_cost: hint.cache_read_input_token_cost,
        };
        pricing.has_signal().then_some(pricing)
    }

    fn has_signal(&self) -> bool {
        self.input_cost_per_token.is_some()
            || self.output_cost_per_token.is_some()
            || self.cache_creation_input_token_cost.is_some()
            || self.cache_read_input_token_cost.is_some()
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct LiteLlmModelHint {
    #[serde(default)]
    pub litellm_provider: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_u64_lossy")]
    pub max_input_tokens: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_u64_lossy")]
    pub max_output_tokens: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_u64_lossy")]
    pub max_tokens: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_bool_lossy")]
    pub supports_function_calling: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_bool_lossy")]
    pub supports_parallel_function_calling: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_bool_lossy")]
    pub supports_vision: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_string_vec_lossy")]
    pub input_modalities: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_bool_lossy")]
    pub supports_prompt_caching: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_bool_lossy")]
    pub supports_reasoning: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_bool_lossy")]
    pub supports_native_streaming: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_string_vec_lossy")]
    pub supported_endpoints: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_f64_lossy")]
    pub input_cost_per_token: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_optional_f64_lossy")]
    pub output_cost_per_token: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_optional_f64_lossy")]
    pub cache_creation_input_token_cost: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_optional_f64_lossy")]
    pub cache_read_input_token_cost: Option<f64>,
}

impl LiteLlmModelHint {
    fn is_language_model_hint(&self) -> bool {
        let language_mode = self
            .mode
            .as_deref()
            .is_some_and(|mode| matches!(mode, "chat" | "completion" | "text_completion"));
        let language_endpoint = self.supported_endpoints.as_ref().is_some_and(|endpoints| {
            endpoints
                .iter()
                .any(|endpoint| matches!(endpoint.as_str(), "/v1/chat/completions"))
        });
        language_mode
            || language_endpoint
            || self.supports_function_calling == Some(true)
            || self.supports_reasoning == Some(true)
    }
}

fn u64_to_i64(value: u64) -> Option<i64> {
    i64::try_from(value).ok()
}

fn deserialize_optional_u64_lossy<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse::<u64>().ok(),
        Value::Bool(_) | Value::Array(_) | Value::Object(_) | Value::Null => None,
    }))
}

fn deserialize_optional_f64_lossy<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.parse::<f64>().ok(),
        Value::Bool(_) | Value::Array(_) | Value::Object(_) | Value::Null => None,
    }))
}

fn deserialize_optional_bool_lossy<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| match value {
        Value::Bool(value) => Some(value),
        Value::String(text) if text.eq_ignore_ascii_case("true") => Some(true),
        Value::String(text) if text.eq_ignore_ascii_case("false") => Some(false),
        Value::Number(_) | Value::String(_) | Value::Array(_) | Value::Object(_) | Value::Null => {
            None
        }
    }))
}

fn deserialize_optional_string_vec_lossy<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| match value {
        Value::Array(items) => {
            let strings = items
                .into_iter()
                .filter_map(|item| match item {
                    Value::String(text) if !text.is_empty() => Some(text),
                    Value::Number(number) => Some(number.to_string()),
                    Value::Bool(value) => Some(value.to_string()),
                    Value::String(_) | Value::Array(_) | Value::Object(_) | Value::Null => None,
                })
                .collect::<Vec<_>>();
            (!strings.is_empty()).then_some(strings)
        }
        Value::String(text) if !text.is_empty() => Some(vec![text]),
        Value::Number(number) => Some(vec![number.to_string()]),
        Value::Bool(value) => Some(vec![value.to_string()]),
        Value::String(_) | Value::Object(_) | Value::Null => None,
    }))
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod tests;
