use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::MergeStrategy;
use serde_json::Value;

use super::super::SettingsStore;
use super::session_memory::TemplateSource;

pub(super) fn template_value(
    store: &SettingsStore,
    inline_key: &str,
    file_key: &str,
) -> (TemplateSource, String) {
    let configured = |key| {
        store
            .configured_value(key)
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    if let Some(value) = configured(inline_key) {
        (TemplateSource::Inline, value)
    } else if let Some(value) = configured(file_key) {
        (TemplateSource::File, value)
    } else {
        (TemplateSource::BuiltIn, String::new())
    }
}

pub(super) fn push_template_edits(
    edits: &mut Vec<ConfigEdit>,
    source: TemplateSource,
    value: &str,
    inline_key: &str,
    file_key: &str,
) {
    let (inline, file) = match source {
        TemplateSource::BuiltIn => (Value::Null, Value::Null),
        TemplateSource::Inline => (Value::String(value.to_string()), Value::Null),
        TemplateSource::File => (Value::Null, Value::String(value.to_string())),
    };
    edits.extend([
        ConfigEdit {
            key_path: inline_key.to_string(),
            value: inline,
            merge_strategy: MergeStrategy::Replace,
        },
        ConfigEdit {
            key_path: file_key.to_string(),
            value: file,
            merge_strategy: MergeStrategy::Replace,
        },
    ]);
}

pub(super) fn template_label(source: TemplateSource, value: &str) -> String {
    match source {
        TemplateSource::BuiltIn => "Built-in".to_string(),
        TemplateSource::Inline => {
            let line = value.lines().next().unwrap_or_default();
            if line.chars().count() > 36 {
                format!("{}…", line.chars().take(36).collect::<String>())
            } else {
                line.to_string()
            }
        }
        TemplateSource::File => value.to_string(),
    }
}
