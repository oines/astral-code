#![allow(clippy::unwrap_used)]

use codex_core::LoadedAgentsMd;
use codex_core::shell::default_user_shell;
use codex_features::Feature;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::config_types::Settings;
use codex_protocol::models::PermissionProfile;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::permissions::NetworkSandboxPolicy;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::ENVIRONMENT_CONTEXT_OPEN_TAG;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use core_test_support::TempDirExt;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::request_tool_names;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::local_selections;
use core_test_support::test_codex::test_codex;
use core_test_support::test_codex::turn_permission_fields;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

fn text_user_input(text: String) -> serde_json::Value {
    text_user_input_parts(vec![text])
}

fn text_user_input_parts(texts: Vec<String>) -> serde_json::Value {
    serde_json::json!({
        "type": "message",
        "role": "user",
        "content": texts
            .into_iter()
            .map(|text| serde_json::json!({ "type": "input_text", "text": text }))
            .collect::<Vec<_>>()
    })
}

fn assert_default_env_context(text: &str, cwd: &str) {
    assert_env_context_fragment(text);
    assert!(
        text.contains(&format!("<cwd>{cwd}</cwd>")),
        "expected cwd in environment context: {text}"
    );
    assert!(
        text.contains(&format!("<shell>{}</shell>", default_user_shell().name())),
        "expected shell in environment context: {text}"
    );
}

fn assert_env_context_fragment(text: &str) {
    assert!(
        text.starts_with(ENVIRONMENT_CONTEXT_OPEN_TAG),
        "expected environment context fragment: {text}"
    );
    assert!(
        text.contains("<current_date>") && text.contains("</current_date>"),
        "expected current_date in environment context: {text}"
    );
    assert!(
        text.contains("<timezone>") && text.contains("</timezone>"),
        "expected timezone in environment context: {text}"
    );
    assert!(
        text.ends_with("</environment_context>"),
        "expected closing environment_context tag: {text}"
    );
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn request_input_texts(body: &serde_json::Value) -> Vec<String> {
    body["input"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|msg| msg["content"].as_array())
        .flatten()
        .filter_map(|item| item["text"].as_str())
        .map(str::to_string)
        .collect()
}

fn assert_request_contains_text(
    body: &serde_json::Value,
    description: &str,
    predicate: impl Fn(&str) -> bool,
) {
    let texts = request_input_texts(body);
    assert!(
        texts.iter().any(|text| predicate(text)),
        "expected {description} in request texts: {texts:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn prompt_tools_are_consistent_across_requests() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    use pretty_assertions::assert_eq;

    let server = start_mock_server().await;
    let req1 = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;
    let req2 = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-2"), ev_completed("resp-2")]),
    )
    .await;

    let TestCodex { codex, .. } = test_codex()
        .with_config(|config| {
            config.user_instructions = Some(LoadedAgentsMd::from_text_for_testing(
                "be consistent and helpful",
            ));
            config.model = Some("gpt-5.2".to_string());
            config
                .features
                .enable(Feature::CollaborationModes)
                .expect("test config should allow feature update");
        })
        .build(&server)
        .await?;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello 1".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello 2".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let request0 = req1.single_request();
    let body0 = request0.body_json();
    let tool_names0 = request_tool_names(&body0);
    assert!(
        tool_names0.contains(&"tool_search".to_string()),
        "expected tool_search in prompt tools: {tool_names0:?}"
    );

    let instructions0 = request0.instructions_text();
    assert!(
        instructions0.contains("You are Astral"),
        "expected base instructions in request: {instructions0}"
    );

    let request1 = req2.single_request();
    let body1 = request1.body_json();
    assert_eq!(request1.instructions_text(), instructions0);
    assert_eq!(
        body1["tools"], body0["tools"],
        "the same tool surface should preserve complete tool schemas and order"
    );
    assert_eq!(request_tool_names(&body1), tool_names0);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gpt_5_tools_without_apply_patch_append_apply_patch_instructions() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    use pretty_assertions::assert_eq;

    let server = start_mock_server().await;
    let req1 = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;
    let req2 = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-2"), ev_completed("resp-2")]),
    )
    .await;

    let TestCodex { codex, .. } = test_codex()
        .with_config(|config| {
            config.user_instructions = Some(LoadedAgentsMd::from_text_for_testing(
                "be consistent and helpful",
            ));
            config
                .features
                .enable(Feature::CollaborationModes)
                .expect("test config should allow feature update");
            config.model = Some("gpt-5.2".to_string());
        })
        .build(&server)
        .await?;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello 1".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;
    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello 2".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let instructions0 = req1.single_request().instructions_text();
    assert!(
        instructions0.contains("You are"),
        "expected non-empty instructions"
    );

    let instructions1 = req2.single_request().instructions_text();
    assert_eq!(
        normalize_newlines(&instructions1),
        normalize_newlines(&instructions0)
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prefixes_context_and_instructions_once_and_consistently_across_requests()
-> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    use pretty_assertions::assert_eq;

    let server = start_mock_server().await;
    let req1 = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;
    let req2 = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-2"), ev_completed("resp-2")]),
    )
    .await;

    let TestCodex { codex, config, .. } = test_codex()
        .with_config(|config| {
            config.user_instructions = Some(LoadedAgentsMd::from_text_for_testing(
                "be consistent and helpful",
            ));
            config
                .features
                .enable(Feature::CollaborationModes)
                .expect("test config should allow feature update");
        })
        .build(&server)
        .await?;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello 1".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello 2".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let body1 = req1.single_request().body_json();
    let input1 = body1["input"].as_array().expect("input array");
    assert!(
        input1.len() >= 3,
        "expected permissions, cached contextual prefix, and user msg"
    );

    let texts1 = request_input_texts(&body1);
    let ui_text = texts1
        .iter()
        .find(|text| text.contains("be consistent and helpful"))
        .expect("user instructions text");
    assert!(
        ui_text.contains("be consistent and helpful"),
        "expected user instructions in UI message: {ui_text}"
    );

    let cwd_str = config.cwd.to_string_lossy();
    let env_text = texts1
        .iter()
        .find(|text| text.starts_with(ENVIRONMENT_CONTEXT_OPEN_TAG))
        .expect("environment context text");
    assert_default_env_context(env_text, &cwd_str);
    assert_request_contains_text(&body1, "first user message", |text| text == "hello 1");

    let body2 = req2.single_request().body_json();
    let input2 = body2["input"].as_array().expect("input array");
    assert_eq!(
        &input2[..input1.len()],
        input1.as_slice(),
        "expected cached prefix to be reused"
    );
    assert_eq!(input2[input1.len()], text_user_input("hello 2".to_string()));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn overrides_turn_context_but_keeps_cached_prefix_and_key_constant() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    use pretty_assertions::assert_eq;

    let server = start_mock_server().await;
    let req1 = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;
    let req2 = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-2"), ev_completed("resp-2")]),
    )
    .await;

    let TestCodex { codex, config, .. } = test_codex()
        .with_config(|config| {
            config.user_instructions = Some(LoadedAgentsMd::from_text_for_testing(
                "be consistent and helpful",
            ));
            config
                .features
                .enable(Feature::CollaborationModes)
                .expect("test config should allow feature update");
        })
        .build(&server)
        .await?;

    // First turn
    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello 1".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let writable = TempDir::new().unwrap();
    let permission_profile = PermissionProfile::workspace_write_with(
        &[writable.abs()],
        NetworkSandboxPolicy::Enabled,
        /*exclude_tmpdir_env_var*/ true,
        /*exclude_slash_tmp*/ true,
    );
    let sandbox_policy = permission_profile
        .to_legacy_sandbox_policy(config.cwd.as_path())
        .expect("workspace profile should have legacy projection");
    core_test_support::submit_thread_settings(
        &codex,
        codex_protocol::protocol::ThreadSettingsOverrides {
            approval_policy: Some(AskForApproval::Never),
            sandbox_policy: Some(sandbox_policy),
            permission_profile: Some(permission_profile),
            effort: Some(Some(ReasoningEffort::High)),
            summary: Some(ReasoningSummary::Detailed),
            ..Default::default()
        },
    )
    .await?;

    // Second turn after overrides
    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello 2".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let request1 = req1.single_request();
    let request2 = req2.single_request();
    let body1 = request1.body_json();
    let body2 = request2.body_json();
    // prompt_cache_key should remain constant across overrides
    assert_eq!(
        body1["prompt_cache_key"], body2["prompt_cache_key"],
        "prompt_cache_key should not change across overrides"
    );

    assert_request_contains_text(&body2, "second user message", |text| text == "hello 2");
    assert_request_contains_text(&body2, "updated permissions message", |text| {
        text.starts_with("<permissions instructions>")
            && text.contains("workspace-write")
            && text.contains(&writable.abs().display().to_string())
    });
    assert_request_contains_text(&body2, "updated environment context", |text| {
        text.starts_with(ENVIRONMENT_CONTEXT_OPEN_TAG)
            && text.contains("<permission_profile type=\"managed\">")
            && text.contains("<file_system type=\"restricted\">")
            && text.contains(&format!(
                "<entry access=\"write\"><path>{}</path></entry>",
                writable.abs().display()
            ))
    });

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn override_before_first_turn_emits_environment_context() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let req = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;

    let TestCodex { codex, .. } = test_codex().build(&server).await?;

    let collaboration_mode = CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "gpt-5.4".to_string(),
            reasoning_effort: Some(ReasoningEffort::High),
            developer_instructions: None,
        },
    };

    core_test_support::submit_thread_settings(
        &codex,
        codex_protocol::protocol::ThreadSettingsOverrides {
            approval_policy: Some(AskForApproval::Never),
            model: Some("gpt-5.4".to_string()),
            effort: Some(Some(ReasoningEffort::Low)),
            collaboration_mode: Some(collaboration_mode),
            ..Default::default()
        },
    )
    .await?;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "first message".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let body = req.single_request().body_json();
    assert_eq!(body["model"].as_str(), Some("gpt-5.4"));
    let input = body["input"]
        .as_array()
        .expect("input array must be present");
    assert!(
        !input.is_empty(),
        "expected at least environment context and user message"
    );

    let env_texts: Vec<&str> = input
        .iter()
        .filter_map(|msg| {
            msg["content"].as_array().map(|content| {
                content
                    .iter()
                    .filter_map(|item| item["text"].as_str())
                    .collect::<Vec<_>>()
            })
        })
        .flatten()
        .filter(|text| text.starts_with(ENVIRONMENT_CONTEXT_OPEN_TAG))
        .collect();
    assert!(
        !env_texts.is_empty(),
        "expected environment context to be emitted: {env_texts:?}"
    );
    assert!(
        env_texts
            .iter()
            .any(|text| text.contains("<current_date>") && text.contains("</current_date>")),
        "expected current_date in environment context: {env_texts:?}"
    );
    assert!(
        env_texts
            .iter()
            .any(|text| text.contains("<timezone>") && text.contains("</timezone>")),
        "expected timezone in environment context: {env_texts:?}"
    );

    let env_count = input
        .iter()
        .filter(|msg| {
            msg["content"]
                .as_array()
                .and_then(|content| {
                    content.iter().find(|item| {
                        item["type"].as_str() == Some("input_text")
                            && item["text"]
                                .as_str()
                                .map(|text| text.starts_with(ENVIRONMENT_CONTEXT_OPEN_TAG))
                                .unwrap_or(false)
                    })
                })
                .is_some()
        })
        .count();
    assert!(
        env_count >= 1,
        "environment context should appear at least once, found {env_count}"
    );

    let permissions_texts: Vec<&str> = input
        .iter()
        .filter_map(|msg| {
            let role = msg["role"].as_str()?;
            if role != "developer" {
                return None;
            }
            msg["content"]
                .as_array()
                .and_then(|content| content.first())
                .and_then(|item| item["text"].as_str())
        })
        .collect();
    assert!(
        permissions_texts.iter().any(|text| {
            let lower = text.to_ascii_lowercase();
            (lower.contains("approval policy") || lower.contains("approval_policy"))
                && lower.contains("never")
        }),
        "permissions message should reflect overridden approval policy: {permissions_texts:?}"
    );

    let user_texts: Vec<&str> = input
        .iter()
        .filter_map(|msg| {
            msg["content"].as_array().map(|content| {
                content
                    .iter()
                    .filter_map(|item| item["text"].as_str())
                    .collect::<Vec<_>>()
            })
        })
        .flatten()
        .collect();
    assert!(
        user_texts.contains(&"first message"),
        "expected user message text, got {user_texts:?}"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn per_turn_overrides_keep_cached_prefix_and_key_constant() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    use pretty_assertions::assert_eq;

    let server = start_mock_server().await;
    let req1 = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;
    let req2 = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-2"), ev_completed("resp-2")]),
    )
    .await;

    let TestCodex { codex, .. } = test_codex()
        .with_config(|config| {
            config.user_instructions = Some(LoadedAgentsMd::from_text_for_testing(
                "be consistent and helpful",
            ));
            config
                .features
                .enable(Feature::CollaborationModes)
                .expect("test config should allow feature update");
        })
        .build(&server)
        .await?;

    // First turn
    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello 1".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    // Second turn using per-turn thread-settings overrides.
    let new_cwd = TempDir::new().unwrap();
    let writable = TempDir::new().unwrap();
    let permission_profile = PermissionProfile::workspace_write_with(
        &[writable.abs()],
        NetworkSandboxPolicy::Enabled,
        /*exclude_tmpdir_env_var*/ true,
        /*exclude_slash_tmp*/ true,
    );
    let (sandbox_policy, permission_profile) =
        turn_permission_fields(permission_profile, new_cwd.path());
    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello 2".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: codex_protocol::protocol::ThreadSettingsOverrides {
                environments: Some(local_selections(new_cwd.abs())),
                approval_policy: Some(AskForApproval::Never),
                sandbox_policy: Some(sandbox_policy),
                permission_profile,
                model: Some("o3".to_string()),
                effort: Some(Some(ReasoningEffort::High)),
                summary: Some(ReasoningSummary::Detailed),
                ..Default::default()
            },
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let request1 = req1.single_request();
    let request2 = req2.single_request();
    let body1 = request1.body_json();
    let body2 = request2.body_json();

    // prompt_cache_key should remain constant across per-turn overrides
    assert_eq!(
        body1["prompt_cache_key"], body2["prompt_cache_key"],
        "prompt_cache_key should not change across per-turn overrides"
    );

    assert!(
        request2.has_message_with_input_texts("developer", |texts| {
            texts.iter().any(|text| text.contains("<model_switch>"))
        }),
        "expected model switch section after model override"
    );
    let expected_cwd = new_cwd.path().display().to_string();
    assert_request_contains_text(&body2, "updated environment context", |text| {
        text.starts_with(ENVIRONMENT_CONTEXT_OPEN_TAG) && text.contains(&expected_cwd)
    });
    assert_request_contains_text(&body2, "second user message", |text| text == "hello 2");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn send_user_turn_with_no_changes_does_not_send_environment_context() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    use pretty_assertions::assert_eq;

    let server = start_mock_server().await;
    let req1 = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;
    let req2 = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-2"), ev_completed("resp-2")]),
    )
    .await;

    let TestCodex {
        codex,
        config,
        session_configured,
        ..
    } = test_codex()
        .with_config(|config| {
            config.user_instructions = Some(LoadedAgentsMd::from_text_for_testing(
                "be consistent and helpful",
            ));
            config
                .features
                .enable(Feature::CollaborationModes)
                .expect("test config should allow feature update");
        })
        .build(&server)
        .await?;

    let default_cwd = config.cwd.clone();
    let default_approval_policy = config.permissions.approval_policy.value();
    let default_sandbox_policy = &config.legacy_sandbox_policy();
    let default_model = session_configured.model;
    let default_effort = config.model_reasoning_effort.clone();
    let default_summary = config.model_reasoning_summary;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello 1".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: codex_protocol::protocol::ThreadSettingsOverrides {
                environments: Some(local_selections(default_cwd.clone())),
                approval_policy: Some(default_approval_policy),
                sandbox_policy: Some(default_sandbox_policy.clone()),
                summary: Some(default_summary.unwrap_or(ReasoningSummary::Auto)),
                collaboration_mode: Some(codex_protocol::config_types::CollaborationMode {
                    mode: codex_protocol::config_types::ModeKind::Default,
                    settings: codex_protocol::config_types::Settings {
                        model: default_model.clone(),
                        reasoning_effort: default_effort.clone(),
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            },
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello 2".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: codex_protocol::protocol::ThreadSettingsOverrides {
                environments: Some(local_selections(default_cwd.clone())),
                approval_policy: Some(default_approval_policy),
                sandbox_policy: Some(default_sandbox_policy.clone()),
                summary: Some(default_summary.unwrap_or(ReasoningSummary::Auto)),
                collaboration_mode: Some(codex_protocol::config_types::CollaborationMode {
                    mode: codex_protocol::config_types::ModeKind::Default,
                    settings: codex_protocol::config_types::Settings {
                        model: default_model.clone(),
                        reasoning_effort: default_effort,
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            },
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let request1 = req1.single_request();
    let request2 = req2.single_request();
    let body1 = request1.body_json();
    let body2 = request2.body_json();

    let texts1 = request_input_texts(&body1);
    let default_cwd_lossy = default_cwd.to_string_lossy();
    let env_texts1 = texts1
        .iter()
        .filter(|text| text.starts_with(ENVIRONMENT_CONTEXT_OPEN_TAG))
        .collect::<Vec<_>>();
    assert_eq!(env_texts1.len(), 1, "expected one environment context");
    let expected_env_text_1 = env_texts1[0];
    assert_default_env_context(expected_env_text_1, &default_cwd_lossy);
    assert!(
        texts1
            .iter()
            .any(|text| text.contains("be consistent and helpful")),
        "expected cached user instructions text: {texts1:?}"
    );
    assert!(
        texts1.iter().any(|text| text == "hello 1"),
        "expected first user message: {texts1:?}"
    );

    let input1 = body1["input"].as_array().expect("first input array");
    let input2 = body2["input"].as_array().expect("second input array");
    assert_eq!(
        &input2[..input1.len()],
        input1.as_slice(),
        "expected unchanged context prefix to be reused"
    );
    assert_eq!(input2[input1.len()], text_user_input("hello 2".to_string()));

    let texts2 = request_input_texts(&body2);
    let env_texts2 = texts2
        .iter()
        .filter(|text| text.starts_with(ENVIRONMENT_CONTEXT_OPEN_TAG))
        .collect::<Vec<_>>();
    assert_eq!(
        env_texts2, env_texts1,
        "unchanged turn settings should not add a fresh environment context"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn send_user_turn_with_changes_sends_environment_context() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;

    let req1 = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;
    let req2 = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-2"), ev_completed("resp-2")]),
    )
    .await;
    let TestCodex {
        codex,
        config,
        session_configured,
        ..
    } = test_codex()
        .with_config(|config| {
            config.user_instructions = Some(LoadedAgentsMd::from_text_for_testing(
                "be consistent and helpful",
            ));
            config
                .features
                .enable(Feature::CollaborationModes)
                .expect("test config should allow feature update");
        })
        .build(&server)
        .await?;

    let default_cwd = config.cwd.clone();
    let default_approval_policy = config.permissions.approval_policy.value();
    let default_sandbox_policy = &config.legacy_sandbox_policy();
    let default_model = session_configured.model;
    let default_effort = config.model_reasoning_effort.clone();
    let default_summary = config.model_reasoning_summary;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello 1".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: codex_protocol::protocol::ThreadSettingsOverrides {
                environments: Some(local_selections(default_cwd.clone())),
                approval_policy: Some(default_approval_policy),
                sandbox_policy: Some(default_sandbox_policy.clone()),
                summary: Some(default_summary.unwrap_or(ReasoningSummary::Auto)),
                collaboration_mode: Some(codex_protocol::config_types::CollaborationMode {
                    mode: codex_protocol::config_types::ModeKind::Default,
                    settings: codex_protocol::config_types::Settings {
                        model: default_model,
                        reasoning_effort: default_effort,
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            },
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let (sandbox_policy, permission_profile) =
        turn_permission_fields(PermissionProfile::Disabled, default_cwd.as_path());
    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello 2".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: codex_protocol::protocol::ThreadSettingsOverrides {
                environments: Some(local_selections(default_cwd.clone())),
                approval_policy: Some(AskForApproval::Never),
                sandbox_policy: Some(sandbox_policy),
                permission_profile,
                summary: Some(ReasoningSummary::Detailed),
                collaboration_mode: Some(codex_protocol::config_types::CollaborationMode {
                    mode: codex_protocol::config_types::ModeKind::Default,
                    settings: codex_protocol::config_types::Settings {
                        model: "o3".to_string(),
                        reasoning_effort: Some(ReasoningEffort::High),
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            },
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let request1 = req1.single_request();
    let request2 = req2.single_request();
    let body1 = request1.body_json();
    let body2 = request2.body_json();

    assert_request_contains_text(&body1, "cached user instructions", |text| {
        text.contains("be consistent and helpful")
    });
    assert_request_contains_text(&body1, "initial environment context", |text| {
        text.starts_with(ENVIRONMENT_CONTEXT_OPEN_TAG)
            && text.contains(&default_cwd.to_string_lossy().to_string())
    });
    assert_request_contains_text(&body1, "first user message", |text| text == "hello 1");

    assert!(
        request2.has_message_with_input_texts("developer", |texts| {
            texts.iter().any(|text| text.contains("<model_switch>"))
        }),
        "expected model switch section after model override"
    );
    assert_request_contains_text(&body2, "disabled permission environment context", |text| {
        text.starts_with(ENVIRONMENT_CONTEXT_OPEN_TAG)
            && text.contains(
                "<permission_profile type=\"disabled\"><file_system type=\"unrestricted\" /></permission_profile>",
            )
    });
    assert_request_contains_text(&body2, "second user message", |text| text == "hello 2");

    Ok(())
}
