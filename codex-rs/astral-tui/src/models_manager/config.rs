use codex_app_server_protocol::ConfigBatchWriteParams;
use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::ConfigLayerSource;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::MergeStrategy;
use serde_json::Map;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ConfigWriteTarget {
    file_path: String,
    expected_version: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModelsConfigWrite {
    focus_provider: String,
    edits: Vec<ConfigEdit>,
    target: ConfigWriteTarget,
}

impl ModelsConfigWrite {
    pub(crate) fn into_parts(self) -> (String, ConfigBatchWriteParams) {
        (
            self.focus_provider,
            ConfigBatchWriteParams {
                edits: self.edits,
                file_path: Some(self.target.file_path),
                expected_version: Some(self.target.expected_version),
                reload_user_config: true,
            },
        )
    }
}

pub(super) fn write_target(response: &ConfigReadResponse) -> Option<ConfigWriteTarget> {
    response.layers.as_deref()?.iter().find_map(|layer| {
        let ConfigLayerSource::User {
            file,
            profile: None,
        } = &layer.name
        else {
            return None;
        };
        Some(ConfigWriteTarget {
            file_path: file.to_string_lossy().to_string(),
            expected_version: layer.version.clone(),
        })
    })
}

pub(super) fn configured_providers(
    response: &ConfigReadResponse,
) -> std::collections::BTreeMap<String, Map<String, Value>> {
    response
        .config
        .additional
        .get("model_providers")
        .and_then(Value::as_object)
        .map(|providers| {
            providers
                .iter()
                .filter_map(|(id, value)| {
                    value.as_object().cloned().map(|value| (id.clone(), value))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn configured_capabilities(
    response: &ConfigReadResponse,
) -> std::collections::BTreeMap<String, Map<String, Value>> {
    response
        .config
        .additional
        .get("model_capabilities")
        .and_then(Value::as_object)
        .map(|models| {
            models
                .iter()
                .filter_map(|(id, value)| {
                    value.as_object().cloned().map(|value| (id.clone(), value))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn provider_write(
    target: ConfigWriteTarget,
    provider_id: String,
    value: Map<String, Value>,
) -> ModelsConfigWrite {
    ModelsConfigWrite {
        focus_provider: provider_id.clone(),
        edits: vec![ConfigEdit {
            key_path: quoted_key("model_providers", &provider_id),
            value: Value::Object(value),
            merge_strategy: MergeStrategy::Replace,
        }],
        target,
    }
}

pub(super) fn capability_write(
    target: ConfigWriteTarget,
    provider_id: String,
    model_id: String,
    value: Map<String, Value>,
) -> ModelsConfigWrite {
    let model_key = format!("{provider_id}/{model_id}");
    ModelsConfigWrite {
        focus_provider: provider_id,
        edits: vec![ConfigEdit {
            key_path: quoted_key("model_capabilities", &model_key),
            value: Value::Object(value),
            merge_strategy: MergeStrategy::Replace,
        }],
        target,
    }
}

fn quoted_key(root: &str, key: &str) -> String {
    let escaped = key.replace('\\', "\\\\").replace('"', "\\\"");
    format!("{root}.\"{escaped}\"")
}
