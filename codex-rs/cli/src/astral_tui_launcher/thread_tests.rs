use std::collections::HashMap;

use codex_core::config::ConfigBuilder;
use codex_protocol::config_types::Personality;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::config_types::Verbosity;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::config_request_overrides;

#[tokio::test]
async fn thread_config_preserves_model_and_surface_overrides() {
    let codex_home = tempfile::tempdir().expect("create temporary CODEX_HOME");
    let mut config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .fallback_cwd(Some(codex_home.path().to_path_buf()))
        .build()
        .await
        .expect("build isolated config");
    config.model_reasoning_effort = Some(ReasoningEffort::High);
    config.model_reasoning_summary = Some(ReasoningSummary::Detailed);
    config.model_verbosity = Some(Verbosity::Low);
    config.personality = Some(Personality::Pragmatic);
    config.bypass_hook_trust = true;

    assert_eq!(
        config_request_overrides(&config),
        HashMap::from([
            ("model_reasoning_effort".to_string(), json!("high")),
            ("model_reasoning_summary".to_string(), json!("detailed")),
            ("model_verbosity".to_string(), json!("low")),
            ("personality".to_string(), json!("pragmatic")),
            ("web_search".to_string(), json!("cached")),
            ("bypass_hook_trust".to_string(), json!(true)),
        ])
    );
}
