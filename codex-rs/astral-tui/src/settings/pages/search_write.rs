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

fn optional_string(value: &Option<String>) -> Value {
    value.clone().map_or(Value::Null, Value::String)
}
