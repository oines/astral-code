use codex_app_server_protocol::ConfigBatchWriteParams;
use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::ConfigLayerSource;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::MergeStrategy;
use serde_json::Map;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProviderConfigSource {
    pub(super) label: String,
    pub(super) user_writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ConfigWriteTarget {
    file_path: String,
    expected_version: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModelsConfigWrite {
    focus_provider: Option<String>,
    edits: Vec<ConfigEdit>,
    target: ConfigWriteTarget,
}

impl ModelsConfigWrite {
    pub(crate) fn into_parts(self) -> (Option<String>, ConfigBatchWriteParams) {
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

pub(super) fn configured_provider_sources(
    response: &ConfigReadResponse,
) -> std::collections::BTreeMap<String, ProviderConfigSource> {
    let mut sources = std::collections::BTreeMap::new();
    let Some(layers) = response.layers.as_deref() else {
        return sources;
    };
    for layer in layers
        .iter()
        .rev()
        .filter(|layer| layer.disabled_reason.is_none())
    {
        let Some(providers) = layer
            .config
            .get("model_providers")
            .and_then(Value::as_object)
        else {
            continue;
        };
        let source = provider_source(&layer.name);
        for id in providers.keys() {
            sources.entry(id.clone()).or_insert_with(|| source.clone());
        }
    }
    sources
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
        focus_provider: Some(provider_id.clone()),
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
        focus_provider: Some(provider_id),
        edits: vec![ConfigEdit {
            key_path: quoted_key("model_capabilities", &model_key),
            value: Value::Object(value),
            merge_strategy: MergeStrategy::Replace,
        }],
        target,
    }
}

pub(super) fn provider_delete(
    target: ConfigWriteTarget,
    provider_id: String,
    capability_keys: impl IntoIterator<Item = String>,
    clear_default: bool,
) -> ModelsConfigWrite {
    let mut edits = vec![ConfigEdit {
        key_path: quoted_key("model_providers", &provider_id),
        value: Value::Null,
        merge_strategy: MergeStrategy::Replace,
    }];
    edits.extend(capability_keys.into_iter().map(|model_key| ConfigEdit {
        key_path: quoted_key("model_capabilities", &model_key),
        value: Value::Null,
        merge_strategy: MergeStrategy::Replace,
    }));
    if clear_default {
        edits.extend(
            ["model_provider", "model"]
                .into_iter()
                .map(|key_path| ConfigEdit {
                    key_path: key_path.to_string(),
                    value: Value::Null,
                    merge_strategy: MergeStrategy::Replace,
                }),
        );
    }
    ModelsConfigWrite {
        focus_provider: None,
        edits,
        target,
    }
}

fn quoted_key(root: &str, key: &str) -> String {
    let escaped = key.replace('\\', "\\\\").replace('"', "\\\"");
    format!("{root}.\"{escaped}\"")
}

fn provider_source(source: &ConfigLayerSource) -> ProviderConfigSource {
    match source {
        ConfigLayerSource::User {
            file,
            profile: None,
        } => ProviderConfigSource {
            label: format!("User · {}", file.display()),
            user_writable: true,
        },
        ConfigLayerSource::User {
            file,
            profile: Some(profile),
        } => ProviderConfigSource {
            label: format!("User profile {profile} · {}", file.display()),
            user_writable: false,
        },
        ConfigLayerSource::Project { dot_codex_folder } => ProviderConfigSource {
            label: format!(
                "Project override · {}",
                dot_codex_folder.join("config.toml").display()
            ),
            user_writable: false,
        },
        ConfigLayerSource::System { file } => ProviderConfigSource {
            label: format!("System · {}", file.display()),
            user_writable: false,
        },
        ConfigLayerSource::Mdm { domain, key } => ProviderConfigSource {
            label: format!("Managed · {domain}/{key}"),
            user_writable: false,
        },
        ConfigLayerSource::EnterpriseManaged { name, .. } => ProviderConfigSource {
            label: format!("Managed · {name}"),
            user_writable: false,
        },
        ConfigLayerSource::SessionFlags => ProviderConfigSource {
            label: "Session override".to_string(),
            user_writable: false,
        },
        ConfigLayerSource::LegacyManagedConfigTomlFromFile { file } => ProviderConfigSource {
            label: format!("Managed · {}", file.display()),
            user_writable: false,
        },
        ConfigLayerSource::LegacyManagedConfigTomlFromMdm => ProviderConfigSource {
            label: "Managed by device policy".to_string(),
            user_writable: false,
        },
    }
}
