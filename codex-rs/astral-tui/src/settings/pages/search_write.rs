use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::MergeStrategy;
use serde_json::Value;

use super::super::SettingsFocus;
use super::super::SettingsInput;
use super::super::SettingsStore;
use super::search::SearchField;
use super::search::SearchPageState;
use super::search::SecretDraft;

impl SearchPageState {
    pub(super) fn save(&mut self, store: &SettingsStore) -> SettingsInput {
        if !self.is_dirty() {
            return SettingsInput::Notice("No search settings changed".to_string());
        }
        let mut edits = Vec::new();
        for field in &self.changed {
            match field {
                SearchField::Provider => edits.push(edit(
                    "tools.web_search.provider",
                    optional_string(&self.provider),
                )),
                SearchField::ApiKey => match &self.secret {
                    SecretDraft::Keep => {}
                    SecretDraft::Replace(secret) => edits.push(edit(
                        "tools.web_search.api_key",
                        Value::String(secret.clone()),
                    )),
                    SecretDraft::Clear => {
                        edits.push(edit("tools.web_search.api_key", Value::Null));
                    }
                },
                SearchField::ContextSize => edits.push(edit(
                    "tools.web_search.context_size",
                    optional_string(&self.context_size),
                )),
                SearchField::AllowedDomains => edits.push(edit(
                    "tools.web_search.allowed_domains",
                    domains_value(&self.allowed_domains),
                )),
                SearchField::Country => edits.push(edit(
                    "tools.web_search.location.country",
                    text_value(&self.country),
                )),
                SearchField::Region => edits.push(edit(
                    "tools.web_search.location.region",
                    text_value(&self.region),
                )),
                SearchField::City => edits.push(edit(
                    "tools.web_search.location.city",
                    text_value(&self.city),
                )),
                SearchField::Timezone => edits.push(edit(
                    "tools.web_search.location.timezone",
                    text_value(&self.timezone),
                )),
                SearchField::Save => {}
            }
        }
        let Some(write) = store.write_edits(edits, SettingsFocus::Search) else {
            return SettingsInput::Notice("User config is not writable".to_string());
        };
        SettingsInput::Write {
            write,
            selected_theme: None,
        }
    }
}

fn edit(key_path: &str, value: Value) -> ConfigEdit {
    ConfigEdit {
        key_path: key_path.to_string(),
        value,
        merge_strategy: MergeStrategy::Replace,
    }
}

fn text_value(value: &str) -> Value {
    if value.trim().is_empty() {
        Value::Null
    } else {
        Value::String(value.trim().to_string())
    }
}

fn optional_string(value: &Option<String>) -> Value {
    value.clone().map_or(Value::Null, Value::String)
}

fn domains_value(value: &str) -> Value {
    let domains = value
        .split([',', '\n'])
        .map(str::trim)
        .filter(|domain| !domain.is_empty())
        .map(|domain| Value::String(domain.to_string()))
        .collect::<Vec<_>>();
    if domains.is_empty() {
        Value::Null
    } else {
        Value::Array(domains)
    }
}
