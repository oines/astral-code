use std::collections::HashMap;

use clap::Parser;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadListCwdFilter;
use codex_core::config::ConfigBuilder;
use codex_protocol::config_types::Personality;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::config_types::Verbosity;
use codex_protocol::openai_models::ReasoningEffort;
use codex_tui::Cli;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::ThreadParamsMode;
use super::config_request_overrides;
use super::resume_params;
use super::thread_list_params;

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

#[tokio::test]
async fn local_resume_preserves_recorded_thread_cwd() {
    let current = tempfile::tempdir().expect("create current cwd");
    let history = tempfile::tempdir().expect("create historical cwd");
    let config = ConfigBuilder::default()
        .codex_home(current.path().to_path_buf())
        .fallback_cwd(Some(current.path().to_path_buf()))
        .build()
        .await
        .expect("build isolated config");
    let cli = Cli::try_parse_from(["astral"]).expect("parse CLI");
    let thread = thread("thread-1", history.path());

    let params = resume_params(
        &thread,
        &cli,
        &config,
        ThreadParamsMode::Local,
        /*preserved_workspace_roots*/ None,
    );

    assert_eq!(
        params.cwd.as_deref(),
        Some(history.path().to_string_lossy().as_ref())
    );
    assert_eq!(params.runtime_workspace_roots, None);
    assert_eq!(params.model, None);
    assert_eq!(params.model_provider, None);
    assert_eq!(params.service_tier, None);
    assert_eq!(params.approval_policy, None);
    assert_eq!(params.approvals_reviewer, None);
    assert_eq!(params.sandbox, None);
    assert_eq!(params.permissions, None);
    assert_eq!(params.config, None);
    assert_eq!(params.base_instructions, None);
    assert_eq!(params.developer_instructions, None);
}

#[tokio::test]
async fn explicit_local_cwd_overrides_recorded_thread_cwd() {
    let current = tempfile::tempdir().expect("create current cwd");
    let history = tempfile::tempdir().expect("create historical cwd");
    let cli = Cli::try_parse_from(["astral", "-C", current.path().to_str().expect("utf-8 cwd")])
        .expect("parse CLI");
    let config = ConfigBuilder::default()
        .codex_home(current.path().to_path_buf())
        .harness_overrides(codex_core::config::ConfigOverrides {
            cwd: Some(current.path().to_path_buf()),
            ..Default::default()
        })
        .build()
        .await
        .expect("build isolated config");
    let thread = thread("thread-1", history.path());

    let params = resume_params(
        &thread,
        &cli,
        &config,
        ThreadParamsMode::Local,
        /*preserved_workspace_roots*/ None,
    );

    assert_eq!(
        params.cwd.as_deref(),
        Some(current.path().to_string_lossy().as_ref())
    );
    assert!(params.runtime_workspace_roots.is_some());
    assert!(params.config.is_some());
}

#[tokio::test]
async fn local_resume_preserves_explicit_add_dir_and_hook_trust_bypass() {
    let current = tempfile::tempdir().expect("create current cwd");
    let history = tempfile::tempdir().expect("create historical cwd");
    let extra = tempfile::tempdir().expect("create additional cwd");
    let cli = Cli::try_parse_from([
        "astral",
        "--add-dir",
        extra.path().to_str().expect("utf-8 additional cwd"),
        "--dangerously-bypass-hook-trust",
    ])
    .expect("parse CLI");
    let config = ConfigBuilder::default()
        .codex_home(current.path().to_path_buf())
        .fallback_cwd(Some(current.path().to_path_buf()))
        .build()
        .await
        .expect("build isolated config");
    let thread = thread("thread-1", history.path());

    let preserved_workspace_roots = vec![
        AbsolutePathBuf::try_from(history.path().to_path_buf()).expect("absolute history cwd"),
        AbsolutePathBuf::try_from(extra.path().to_path_buf()).expect("absolute additional cwd"),
    ];
    let params = resume_params(
        &thread,
        &cli,
        &config,
        ThreadParamsMode::Local,
        Some(preserved_workspace_roots.clone()),
    );

    assert_eq!(
        params.runtime_workspace_roots,
        Some(preserved_workspace_roots)
    );
    assert_eq!(
        params.config,
        Some(HashMap::from([(
            "bypass_hook_trust".to_string(),
            json!(true),
        )]))
    );
    assert_eq!(params.permissions, None);
}

#[tokio::test]
async fn named_thread_lookup_is_not_filtered_by_current_provider() {
    let cwd = tempfile::tempdir().expect("create cwd");
    let config = ConfigBuilder::default()
        .codex_home(cwd.path().to_path_buf())
        .fallback_cwd(Some(cwd.path().to_path_buf()))
        .build()
        .await
        .expect("build isolated config");

    let named = thread_list_params(
        Some("older session"),
        /*show_all*/ true,
        /*include_non_interactive*/ false,
        &config,
        ThreadParamsMode::Local,
        /*remote_cwd*/ None,
        /*cursor*/ None,
    );
    let current_provider = thread_list_params(
        /*id_or_name*/ None,
        /*show_all*/ false,
        /*include_non_interactive*/ false,
        &config,
        ThreadParamsMode::Local,
        /*remote_cwd*/ None,
        /*cursor*/ None,
    );

    assert_eq!(named.model_providers, None);
    assert_eq!(
        current_provider.model_providers,
        Some(vec![config.model_provider_id.clone()])
    );
    assert_eq!(
        current_provider.cwd,
        Some(ThreadListCwdFilter::One(
            config.cwd.to_string_lossy().to_string()
        ))
    );
}

fn thread(id: &str, cwd: &std::path::Path) -> Thread {
    serde_json::from_value(json!({
        "id": id,
        "sessionId": id,
        "forkedFromId": null,
        "parentThreadId": null,
        "preview": "historical session",
        "ephemeral": false,
        "modelProvider": "old-provider",
        "createdAt": 1,
        "updatedAt": 2,
        "status": {"type": "idle"},
        "path": null,
        "cwd": cwd,
        "cliVersion": "0.0.0",
        "source": "cli",
        "threadSource": "user",
        "agentNickname": null,
        "agentRole": null,
        "gitInfo": null,
        "name": "older session",
        "turns": []
    }))
    .expect("valid thread")
}
