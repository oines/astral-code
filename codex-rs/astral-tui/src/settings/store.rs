use codex_app_server_protocol::ConfigBatchWriteParams;
use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::ConfigLayer;
use codex_app_server_protocol::ConfigLayerMetadata;
use codex_app_server_protocol::ConfigLayerSource;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::ConfigRequirements;
use codex_app_server_protocol::ExperimentalFeature;
use codex_app_server_protocol::MergeStrategy;
use codex_app_server_protocol::Model;
use codex_app_server_protocol::PermissionProfileSummary;
use serde_json::Value;

use super::Category;

#[derive(Clone, PartialEq)]
pub(crate) struct SettingsData {
    pub(crate) config: ConfigReadResponse,
    pub(crate) models: Vec<Model>,
    pub(crate) features: Vec<ExperimentalFeature>,
    pub(crate) permission_profiles: Vec<PermissionProfileSummary>,
    pub(crate) requirements: Option<ConfigRequirements>,
}

impl std::fmt::Debug for SettingsData {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SettingsData")
            .field("config", &"<redacted>")
            .field("models", &self.models.len())
            .field("features", &self.features.len())
            .field("permission_profiles", &self.permission_profiles.len())
            .field("requirements", &self.requirements)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SettingsFocus {
    Root,
    Category(Category),
    Key(String),
    Models,
    ModelsProvider(String),
    Search,
    SessionMemoryTemplates,
}

impl SettingsFocus {
    pub(crate) fn token(&self) -> String {
        match self {
            Self::Root => "root".to_string(),
            Self::Category(category) => format!("category:{category:?}"),
            Self::Key(key) => format!("key:{key}"),
            Self::Models => "models".to_string(),
            Self::ModelsProvider(provider) => format!("models:{provider}"),
            Self::Search => "search".to_string(),
            Self::SessionMemoryTemplates => "session-memory-templates".to_string(),
        }
    }

    pub(crate) fn from_token(token: &str) -> Self {
        if let Some(key) = token.strip_prefix("key:") {
            return Self::Key(key.to_string());
        }
        if let Some(provider) = token.strip_prefix("models:") {
            return Self::ModelsProvider(provider.to_string());
        }
        if let Some(category) = token.strip_prefix("category:") {
            return Self::Category(match category {
                "Models" => Category::Models,
                "Tools" => Category::Tools,
                "Memory" => Category::Memory,
                "Appearance" => Category::Appearance,
                "Permissions" => Category::Permissions,
                "Features" => Category::Features,
                "Advanced" => Category::Advanced,
                _ => return Self::Root,
            });
        }
        match token {
            "models" => Self::Models,
            "search" => Self::Search,
            "session-memory-templates" => Self::SessionMemoryTemplates,
            _ => Self::Root,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SettingsWrite {
    pub(crate) focus: SettingsFocus,
    pub(crate) params: ConfigBatchWriteParams,
}

#[derive(Clone, PartialEq)]
pub(crate) struct SettingsStore {
    data: SettingsData,
    effective: Value,
    user: Value,
    user_file: Option<String>,
    user_version: Option<String>,
}

impl std::fmt::Debug for SettingsStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SettingsStore")
            .field("data", &self.data)
            .field("effective", &"<redacted>")
            .field("user", &"<redacted>")
            .field("user_file", &self.user_file)
            .field("user_version", &self.user_version)
            .finish()
    }
}

impl SettingsStore {
    pub(crate) fn new(data: SettingsData) -> Self {
        let effective = serde_json::to_value(&data.config.config).unwrap_or(Value::Null);
        let user_layer = data.config.layers.as_deref().and_then(base_user_layer);
        let (user, user_file, user_version) = user_layer.map_or_else(
            || (Value::Null, None, None),
            |layer| {
                let ConfigLayerSource::User { file, .. } = &layer.name else {
                    unreachable!("base_user_layer only returns user layers");
                };
                (
                    layer.config.clone(),
                    Some(file.to_string_lossy().to_string()),
                    Some(layer.version.clone()),
                )
            },
        );
        Self {
            data,
            effective,
            user,
            user_file,
            user_version,
        }
    }

    pub(crate) fn data(&self) -> &SettingsData {
        &self.data
    }

    pub(crate) fn effective_value(&self, key: &str) -> Option<&Value> {
        value_at_path(&self.effective, key)
            .filter(|value| !value.is_null())
            .or_else(|| self.layered_value(key))
    }

    pub(crate) fn user_value(&self, key: &str) -> Option<&Value> {
        value_at_path(&self.user, key)
    }

    pub(crate) fn configured_value(&self, key: &str) -> Option<&Value> {
        self.layered_value(key)
    }

    pub(crate) fn has_user_override(&self, key: &str) -> bool {
        self.user_value(key).is_some()
    }

    pub(crate) fn source(&self, key: &str) -> Option<&ConfigLayerMetadata> {
        self.data.config.origins.get(key).or_else(|| {
            let mut prefix = key;
            while let Some((parent, _)) = prefix.rsplit_once('.') {
                if let Some(origin) = self.data.config.origins.get(parent) {
                    return Some(origin);
                }
                prefix = parent;
            }
            None
        })
    }

    pub(crate) fn source_label(&self, key: &str) -> String {
        match self
            .source(key)
            .map(|origin| &origin.name)
            .or_else(|| self.source_layer(key).map(|layer| &layer.name))
        {
            Some(ConfigLayerSource::User { file, profile }) => profile.as_ref().map_or_else(
                || format!("User · {}", file.display()),
                |profile| format!("User profile {profile} · {}", file.display()),
            ),
            Some(ConfigLayerSource::Project { dot_codex_folder }) => {
                format!(
                    "Project override · {}",
                    dot_codex_folder.join("config.toml").display()
                )
            }
            Some(ConfigLayerSource::System { file }) => {
                format!("System · {}", file.display())
            }
            Some(ConfigLayerSource::Mdm { domain, key }) => {
                format!("Managed · {domain}/{key}")
            }
            Some(ConfigLayerSource::EnterpriseManaged { name, .. }) => {
                format!("Managed · {name}")
            }
            Some(ConfigLayerSource::SessionFlags) => "Session override".to_string(),
            Some(ConfigLayerSource::LegacyManagedConfigTomlFromFile { file }) => {
                format!("Managed · {}", file.display())
            }
            Some(ConfigLayerSource::LegacyManagedConfigTomlFromMdm) => {
                "Managed by device policy".to_string()
            }
            None if self.has_user_override(key) => "User".to_string(),
            None => "Built in".to_string(),
        }
    }

    pub(crate) fn is_overridden_above_user(&self, key: &str) -> bool {
        self.source(key)
            .map(|origin| &origin.name)
            .or_else(|| self.source_layer(key).map(|layer| &layer.name))
            .is_some_and(|source| source.precedence() > 20)
    }

    pub(crate) fn write_value(
        &self,
        key: impl Into<String>,
        value: Value,
        focus: SettingsFocus,
    ) -> Option<SettingsWrite> {
        self.write_edits(
            vec![ConfigEdit {
                key_path: key.into(),
                value,
                merge_strategy: MergeStrategy::Replace,
            }],
            focus,
        )
    }

    pub(crate) fn write_edits(
        &self,
        edits: Vec<ConfigEdit>,
        focus: SettingsFocus,
    ) -> Option<SettingsWrite> {
        Some(SettingsWrite {
            focus,
            params: ConfigBatchWriteParams {
                edits,
                file_path: Some(self.user_file.clone()?),
                expected_version: Some(self.user_version.clone()?),
                reload_user_config: true,
            },
        })
    }

    pub(crate) fn reset(
        &self,
        key: impl Into<String>,
        focus: SettingsFocus,
    ) -> Option<SettingsWrite> {
        self.write_value(key, Value::Null, focus)
    }

    fn layered_value(&self, key: &str) -> Option<&Value> {
        self.data
            .config
            .layers
            .as_deref()?
            .iter()
            .rev()
            .filter(|layer| layer.disabled_reason.is_none())
            .find_map(|layer| value_at_path(&layer.config, key))
    }

    fn source_layer(&self, key: &str) -> Option<&ConfigLayer> {
        self.data
            .config
            .layers
            .as_deref()?
            .iter()
            .rev()
            .filter(|layer| layer.disabled_reason.is_none())
            .find(|layer| value_at_path(&layer.config, key).is_some())
    }
}

fn base_user_layer(layers: &[ConfigLayer]) -> Option<&ConfigLayer> {
    layers
        .iter()
        .find(|layer| matches!(&layer.name, ConfigLayerSource::User { profile: None, .. }))
}

fn value_at_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return None;
    }
    path.split('.')
        .try_fold(root, |value, segment| value.get(segment))
}
