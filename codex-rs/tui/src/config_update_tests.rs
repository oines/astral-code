use super::*;
use color_eyre::eyre::WrapErr;
use pretty_assertions::assert_eq;
use std::path::Path;

#[test]
fn app_scoped_key_path_quotes_dotted_app_ids() {
    assert_eq!(
        app_scoped_key_path("plugin.linear", "enabled"),
        "apps.\"plugin.linear\".enabled"
    );
}

#[test]
fn trusted_project_edit_targets_project_trust_level() {
    assert_eq!(
        trusted_project_edit(Path::new("/workspace/team.project")),
        ConfigEdit {
            key_path: "projects.\"/workspace/team.project\".trust_level".to_string(),
            value: serde_json::json!("trusted"),
            merge_strategy: MergeStrategy::Replace,
        }
    );
}

#[test]
fn model_selection_edits_can_persist_provider() {
    assert_eq!(
        build_model_selection_edits("deepseek-v4-pro", Some("deepseek"), Some("high")),
        vec![
            ConfigEdit {
                key_path: "model".to_string(),
                value: serde_json::json!("deepseek-v4-pro"),
                merge_strategy: MergeStrategy::Replace,
            },
            ConfigEdit {
                key_path: "model_reasoning_effort".to_string(),
                value: serde_json::json!("high"),
                merge_strategy: MergeStrategy::Replace,
            },
            ConfigEdit {
                key_path: "model_provider".to_string(),
                value: serde_json::json!("deepseek"),
                merge_strategy: MergeStrategy::Replace,
            },
        ]
    );
}

#[test]
fn format_config_error_preserves_server_validation_message() {
    let err = Err::<(), _>(color_eyre::eyre::eyre!(
        "config/batchWrite failed: Invalid configuration: features.fast_mode=true violates \
         managed requirements; allowed set [fast_mode=false]"
    ))
    .wrap_err("config/batchWrite failed in TUI")
    .unwrap_err();

    assert_eq!(
        format_config_error(&err),
        "config/batchWrite failed in TUI: config/batchWrite failed: Invalid configuration: \
         features.fast_mode=true violates managed requirements; allowed set [fast_mode=false]"
    );
}
