use std::collections::BTreeSet;

use codex_app_server_protocol::ExperimentalFeatureStage;
use serde_json::Value;

use crate::view::AstralThemeId;

use super::Category;
use super::SettingDefinition;
use super::SettingKind;
use super::SettingOption;
use super::SettingsPage;
use super::SettingsRow;
use super::SettingsState;
use super::Subpage;
use super::categories;
use super::definitions;
use super::pages::SearchPageState;
use super::pages::SessionMemoryPageState;
use super::state::PickerOption;

impl SettingsState {
    pub(crate) fn rows(&self) -> Vec<SettingsRow> {
        let query = self.query.text().trim().to_lowercase();
        if !query.is_empty() {
            return self.search_rows(&query);
        }
        match self.page {
            SettingsPage::Root => categories()
                .iter()
                .copied()
                .map(SettingsRow::Category)
                .collect(),
            SettingsPage::Category(category) => self.category_rows(category),
            SettingsPage::Models | SettingsPage::Search | SettingsPage::SessionMemoryTemplates => {
                Vec::new()
            }
        }
    }

    pub(crate) fn set_selected(&mut self, selected: usize) {
        self.selected = selected.min(self.rows().len().saturating_sub(1));
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.rows().len().saturating_sub(1));
    }

    pub(crate) fn selected_row(&self) -> Option<SettingsRow> {
        self.rows().get(self.selected).copied()
    }

    pub(crate) fn row_key(&self, row: SettingsRow) -> String {
        match row {
            SettingsRow::Category(category) => format!("category:{category:?}"),
            SettingsRow::Definition(definition) => definition.id.to_string(),
            SettingsRow::Feature(index) => {
                format!("features.{}", self.store.data().features[index].name)
            }
        }
    }

    pub(crate) fn row_expanded(&self, row: SettingsRow) -> bool {
        self.expanded.contains(&self.row_key(row))
    }

    pub(crate) fn toggle_expanded(&mut self, row: SettingsRow) {
        let key = self.row_key(row);
        if !self.expanded.remove(&key) {
            self.expanded.insert(key);
        }
    }

    pub(crate) fn value_label(&self, definition: &SettingDefinition) -> String {
        if let SettingKind::Subpage(subpage) = definition.kind {
            return match subpage {
                super::Subpage::Search if self.search.is_dirty() => "Unsaved changes",
                super::Subpage::SessionMemoryTemplates if self.session_memory.is_dirty() => {
                    "Unsaved changes"
                }
                super::Subpage::Models
                | super::Subpage::Search
                | super::Subpage::SessionMemoryTemplates => "Open",
            }
            .to_string();
        }
        let Some(value) = self.store.effective_value(definition.key) else {
            return definition.default.to_string();
        };
        match definition.kind {
            SettingKind::Bool => value
                .as_bool()
                .map(|enabled| if enabled { "On" } else { "Off" }.to_string())
                .unwrap_or_else(|| definition.default.to_string()),
            SettingKind::Enum(options) => option_label(options, value)
                .map(str::to_string)
                .unwrap_or_else(|| display_value(value)),
            SettingKind::Theme => value
                .as_str()
                .and_then(AstralThemeId::from_name)
                .map_or_else(|| display_value(value), |theme| theme.label().to_string()),
            SettingKind::Integer
            | SettingKind::Text
            | SettingKind::DefaultProvider
            | SettingKind::DefaultModel
            | SettingKind::PermissionProfile => display_value(value),
            SettingKind::Subpage(_) => "Open".to_string(),
        }
    }

    pub(crate) fn feature_value_label(&self, index: usize) -> &'static str {
        if self.store.data().features[index].enabled {
            "On"
        } else {
            "Off"
        }
    }

    pub(crate) fn row_disabled_reason(&self, row: SettingsRow) -> Option<String> {
        match row {
            SettingsRow::Definition(definition)
                if definition.key == "default_permissions"
                    && self
                        .store
                        .data()
                        .requirements
                        .as_ref()
                        .and_then(|requirements| requirements.default_permissions.as_ref())
                        .is_some() =>
            {
                let required = self
                    .store
                    .data()
                    .requirements
                    .as_ref()
                    .and_then(|requirements| requirements.default_permissions.as_deref())
                    .unwrap_or_default();
                Some(format!("Managed policy requires {required}"))
            }
            SettingsRow::Definition(definition)
                if definition.key == "memories.compact_memory"
                    && self
                        .store
                        .data()
                        .features
                        .iter()
                        .find(|feature| feature.name == "memories")
                        .is_some_and(|feature| !feature.enabled) =>
            {
                Some("Enable the Memories feature first".to_string())
            }
            SettingsRow::Definition(definition)
                if definition.key == "memories.compact_memory"
                    && self
                        .store
                        .effective_value("memories.generate_memories")
                        .and_then(Value::as_bool)
                        == Some(false) =>
            {
                Some("Enable Generate memories first".to_string())
            }
            SettingsRow::Feature(index) => self
                .store
                .data()
                .requirements
                .as_ref()
                .and_then(|requirements| requirements.feature_requirements.as_ref())
                .and_then(|requirements| {
                    requirements
                        .get(&self.store.data().features[index].name)
                        .map(|required| {
                            format!(
                                "Managed policy requires {}",
                                if *required { "On" } else { "Off" }
                            )
                        })
                }),
            SettingsRow::Category(_) | SettingsRow::Definition(_) => None,
        }
    }

    pub(crate) fn category_value_label(&self, category: Category) -> String {
        if (category == Category::Tools && self.search.is_dirty())
            || (category == Category::Advanced && self.session_memory.is_dirty())
        {
            return "Unsaved changes".to_string();
        }
        let string_value = |key: &str, fallback: &str| {
            self.store
                .effective_value(key)
                .and_then(Value::as_str)
                .unwrap_or(fallback)
                .to_string()
        };
        match category {
            Category::Models => {
                let model = string_value("model", "Provider default");
                let effort = string_value("model_reasoning_effort", "model default");
                format!("{model} · {effort}")
            }
            Category::Tools => {
                let surface = match string_value("tools.surface", "claude").as_str() {
                    "codex" => "Codex",
                    _ => "Claude",
                };
                let search = match string_value("web_search", "disabled").as_str() {
                    "live" => "Search on",
                    "cached" => "Hosted search",
                    _ => "Search off",
                };
                format!("{surface} · {search}")
            }
            Category::Memory => {
                let session = self
                    .store
                    .effective_value("experimental_session_memory_compact")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let compact = string_value("memories.compact_memory", "off");
                format!(
                    "Session {} · compact {compact}",
                    if session { "on" } else { "off" }
                )
            }
            Category::Appearance => {
                let theme = string_value("tui.theme", "automatic");
                AstralThemeId::from_name(&theme).map_or(theme, |theme| theme.label().to_string())
            }
            Category::Permissions => string_value("default_permissions", "workspace"),
            Category::Features => {
                let visible = self
                    .store
                    .data()
                    .features
                    .iter()
                    .filter(|feature| {
                        matches!(
                            feature.stage,
                            ExperimentalFeatureStage::Stable | ExperimentalFeatureStage::Beta
                        )
                    })
                    .collect::<Vec<_>>();
                let enabled = visible.iter().filter(|feature| feature.enabled).count();
                format!("{enabled}/{} enabled", visible.len())
            }
            Category::Advanced => String::new(),
        }
    }

    fn category_rows(&self, category: Category) -> Vec<SettingsRow> {
        let mut rows = definitions()
            .iter()
            .filter(|definition| definition.category == category)
            .map(SettingsRow::Definition)
            .collect::<Vec<_>>();
        if category == Category::Features || category == Category::Advanced {
            rows.extend(
                self.store
                    .data()
                    .features
                    .iter()
                    .enumerate()
                    .filter(|(_, feature)| match (category, &feature.stage) {
                        (
                            Category::Features,
                            ExperimentalFeatureStage::Stable | ExperimentalFeatureStage::Beta,
                        )
                        | (Category::Advanced, ExperimentalFeatureStage::UnderDevelopment) => true,
                        (
                            Category::Features | Category::Advanced,
                            ExperimentalFeatureStage::Deprecated
                            | ExperimentalFeatureStage::Removed
                            | ExperimentalFeatureStage::UnderDevelopment
                            | ExperimentalFeatureStage::Stable
                            | ExperimentalFeatureStage::Beta,
                        ) => false,
                        _ => false,
                    })
                    .map(|(index, _)| SettingsRow::Feature(index)),
            );
        }
        rows
    }

    fn search_rows(&self, query: &str) -> Vec<SettingsRow> {
        let mut rows =
            definitions()
                .iter()
                .filter(|definition| {
                    [definition.label, definition.description, definition.key]
                        .iter()
                        .any(|value| value.to_lowercase().contains(query))
                        || match definition.kind {
                            SettingKind::Subpage(Subpage::Models) => {
                                "provider base url wire api environment variable model discovery capabilities litellm manual fallback"
                                    .contains(query)
                            }
                            SettingKind::Subpage(Subpage::Search) => {
                                SearchPageState::query_match(query).is_some()
                            }
                            SettingKind::Subpage(Subpage::SessionMemoryTemplates) => {
                                SessionMemoryPageState::query_match(query).is_some()
                            }
                            SettingKind::Bool
                            | SettingKind::Integer
                            | SettingKind::Text
                            | SettingKind::DefaultProvider
                            | SettingKind::DefaultModel
                            | SettingKind::Enum(_)
                            | SettingKind::Theme
                            | SettingKind::PermissionProfile => false,
                        }
                })
                .map(SettingsRow::Definition)
                .collect::<Vec<_>>();
        rows.extend(
            self.store
                .data()
                .features
                .iter()
                .enumerate()
                .filter(|(_, feature)| {
                    !matches!(
                        feature.stage,
                        ExperimentalFeatureStage::Deprecated | ExperimentalFeatureStage::Removed
                    ) && [
                        Some(feature.name.as_str()),
                        feature.display_name.as_deref(),
                        feature.description.as_deref(),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|value| value.to_lowercase().contains(query))
                })
                .map(|(index, _)| SettingsRow::Feature(index)),
        );
        rows.extend(
            categories()
                .iter()
                .copied()
                .filter(|category| {
                    [category.label(), category.description()]
                        .iter()
                        .any(|value| value.to_lowercase().contains(query))
                })
                .map(SettingsRow::Category),
        );
        rows
    }

    pub(super) fn default_model_options(&self, kind: SettingKind) -> Vec<PickerOption> {
        match kind {
            SettingKind::DefaultProvider => self.default_provider_options(),
            SettingKind::DefaultModel => self.default_model_id_options(),
            SettingKind::Bool
            | SettingKind::Integer
            | SettingKind::Text
            | SettingKind::Enum(_)
            | SettingKind::Theme
            | SettingKind::PermissionProfile
            | SettingKind::Subpage(_) => Vec::new(),
        }
    }

    fn default_provider_options(&self) -> Vec<PickerOption> {
        let mut providers = std::collections::BTreeMap::<String, String>::new();
        for model in &self.store.data().models {
            providers
                .entry(model.model_provider.clone())
                .or_insert_with(|| model.model_provider_name.clone());
        }
        if let Some(configured) = self
            .store
            .effective_value("model_providers")
            .and_then(Value::as_object)
        {
            for (id, value) in configured {
                let name = value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(id)
                    .to_string();
                providers.entry(id.clone()).or_insert(name);
            }
        }
        if let Some(current) = self
            .store
            .effective_value("model_provider")
            .and_then(Value::as_str)
        {
            providers
                .entry(current.to_string())
                .or_insert_with(|| current.to_string());
        }
        providers
            .into_iter()
            .map(|(id, name)| PickerOption {
                label: if name == id {
                    id.clone()
                } else {
                    format!("{name} [{id}]")
                },
                value: Value::String(id),
            })
            .collect()
    }

    fn default_model_id_options(&self) -> Vec<PickerOption> {
        let provider = self
            .store
            .effective_value("model_provider")
            .and_then(Value::as_str);
        let mut seen = BTreeSet::new();
        let mut options = vec![PickerOption {
            label: "Provider default".to_string(),
            value: Value::Null,
        }];
        options.extend(
            self.store
                .data()
                .models
                .iter()
                .filter(|model| provider.is_none_or(|provider| model.model_provider == provider))
                .filter(|&model| seen.insert(model.model.clone()))
                .map(|model| PickerOption {
                    label: if model.display_name == model.model {
                        model.model.clone()
                    } else {
                        format!("{} [{}]", model.display_name, model.model)
                    },
                    value: Value::String(model.model.clone()),
                })
                .collect::<Vec<_>>(),
        );
        if let Some(current) = self.store.effective_value("model").and_then(Value::as_str)
            && seen.insert(current.to_string())
        {
            options.insert(
                1,
                PickerOption {
                    label: current.to_string(),
                    value: Value::String(current.to_string()),
                },
            );
        }
        options
    }
}

fn option_label<'a>(options: &'a [SettingOption], value: &Value) -> Option<&'a str> {
    let value = value.as_str()?;
    options
        .iter()
        .find(|option| option.value == value)
        .map(|option| option.label)
}

fn display_value(value: &Value) -> String {
    match value {
        Value::Null => "Inherited".to_string(),
        Value::Bool(enabled) => {
            if *enabled {
                "On".to_string()
            } else {
                "Off".to_string()
            }
        }
        Value::Number(number) => number.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(values) => values
            .iter()
            .map(display_value)
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(_) => "Configured".to_string(),
    }
}
