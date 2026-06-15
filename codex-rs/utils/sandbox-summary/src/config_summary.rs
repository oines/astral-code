use codex_core::config::Config;

use crate::sandbox_summary::summarize_sandbox_policy;

/// Build a list of key/value pairs summarizing the effective configuration.
pub fn create_config_summary_entries(config: &Config, model: &str) -> Vec<(&'static str, String)> {
    let entries = vec![
        ("workdir", config.cwd.display().to_string()),
        ("model", model.to_string()),
        ("provider", config.model_provider_id.clone()),
        (
            "approval",
            config.permissions.approval_policy.value().to_string(),
        ),
        (
            "sandbox",
            summarize_sandbox_policy(
                &config
                    .permissions
                    .legacy_sandbox_policy(config.cwd.as_path()),
            ),
        ),
    ];

    entries
}
