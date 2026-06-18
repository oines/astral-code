#![allow(clippy::expect_used)]

use anyhow::Result;
use codex_features::Feature;
use codex_login::CodexAuth;
use codex_model_provider_info::WireApi;
use codex_model_provider_info::create_oss_provider_with_base_url;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::models::PermissionProfile;
use codex_protocol::openai_models::ApplyPatchToolType;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelVisibility;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::openai_models::TruncationPolicyConfig;
use codex_protocol::openai_models::default_input_modalities;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use core_test_support::TempDirExt;
use core_test_support::skip_if_no_network;
use core_test_support::skip_if_sandbox;
use core_test_support::test_codex::local_selections;
use core_test_support::test_codex::test_codex;
use core_test_support::test_codex::turn_permission_fields;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::Respond;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_review_uses_current_chat_completions_model_for_bash_approval() -> Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_sandbox!(Ok(()));

    let server = MockServer::start().await;
    let model = "astral-current-reviewer";
    mount_chat_completions_sse_sequence(
        &server,
        vec![
            chat_completions_tool_call_sse(
                "chatcmpl-parent-tool",
                "bash-approval-call",
                "Bash",
                json!({
                    "command": "printf chat-approved > guardian-chat-completions.txt",
                    "sandbox_permissions": "require_escalated",
                    "justification": "Exercise Astral auto-review through chat completions.",
                    "yield_time_ms": 1_000_u64,
                }),
            ),
            chat_completions_text_sse(
                "chatcmpl-guardian",
                json!({
                    "risk_level": "low",
                    "user_authorization": "high",
                    "outcome": "allow",
                    "rationale": "The command writes a marker file in the workspace.",
                })
                .to_string(),
            ),
            chat_completions_text_sse("chatcmpl-parent-done", "done".to_string()),
        ],
    )
    .await;

    let mut builder = test_codex()
        .with_auth(CodexAuth::from_api_key("dummy"))
        .with_config({
            let base_url = format!("{}/v1", server.uri());
            move |config| {
                config.model = Some(model.to_string());
                config.model_provider =
                    create_oss_provider_with_base_url(&base_url, WireApi::ChatCompletions);
                config.model_catalog = Some(ModelsResponse {
                    models: vec![remote_model(
                        model, /*auto_review_model_override*/ None,
                    )],
                });
                config.approvals_reviewer = ApprovalsReviewer::AutoReview;
                config
                    .features
                    .enable(Feature::UnifiedExec)
                    .expect("test config should allow feature update");
                config
                    .features
                    .enable(Feature::ExecPermissionApprovals)
                    .expect("test config should allow feature update");
                config
                    .features
                    .enable(Feature::GuardianApproval)
                    .expect("test config should allow feature update");
            }
        });
    let test = builder.build(&server).await?;

    let cwd_path = test.cwd.abs();
    let (sandbox_policy, permission_profile) =
        turn_permission_fields(PermissionProfile::read_only(), cwd_path.as_path());
    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "run a Bash command that requires automatic approval review".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: codex_protocol::protocol::ThreadSettingsOverrides {
                environments: Some(local_selections(cwd_path)),
                approval_policy: Some(AskForApproval::OnRequest),
                approvals_reviewer: Some(ApprovalsReviewer::AutoReview),
                sandbox_policy: Some(sandbox_policy),
                permission_profile,
                ..Default::default()
            },
        })
        .await?;

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut observed_events = Vec::new();
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or_default();
        if remaining.is_zero() {
            let request_count = server.received_requests().await.unwrap_or_default().len();
            panic!(
                "timed out waiting for auto-review chat completions turn to complete; observed {observed_events:?}; request_count={request_count}"
            );
        }

        let event = match tokio::time::timeout(remaining, test.codex.next_event()).await {
            Ok(Ok(event)) => event.msg,
            Ok(Err(err)) => panic!("event stream ended unexpectedly: {err}"),
            Err(_) => {
                let request_count = server.received_requests().await.unwrap_or_default().len();
                panic!(
                    "timed out waiting for auto-review chat completions turn to complete; observed {observed_events:?}; request_count={request_count}"
                );
            }
        };
        observed_events.push(format!("{event:?}"));
        if matches!(event, EventMsg::TurnComplete(_)) {
            break;
        }
    }

    let marker_path = test.cwd.path().join("guardian-chat-completions.txt");
    assert_eq!(
        tokio::fs::read_to_string(marker_path).await?,
        "chat-approved"
    );

    let requests = server.received_requests().await.unwrap_or_default();
    let chat_bodies = requests
        .iter()
        .filter(|request| request.url.path().ends_with("/chat/completions"))
        .map(|request| {
            serde_json::from_slice::<serde_json::Value>(&request.body)
                .expect("chat completions request body should be json")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        chat_bodies.len(),
        3,
        "expected parent tool call, Guardian review, and parent follow-up"
    );
    assert_eq!(chat_bodies[0]["model"].as_str(), Some(model));
    assert_eq!(chat_bodies[1]["model"].as_str(), Some(model));
    assert_eq!(chat_bodies[2]["model"].as_str(), Some(model));
    let guardian_body = chat_bodies[1].to_string();
    assert!(
        guardian_body.contains("You are judging one planned coding-agent action."),
        "expected Guardian prompt in reviewer request: {guardian_body}"
    );
    assert!(
        guardian_body.contains("Exercise Astral auto-review through chat completions."),
        "expected reviewed Bash approval reason in Guardian request: {guardian_body}"
    );

    Ok(())
}

fn remote_model(slug: &str, auto_review_model_override: Option<&str>) -> ModelInfo {
    ModelInfo {
        slug: slug.to_string(),
        display_name: format!("{slug} display"),
        description: Some(format!("{slug} description")),
        default_reasoning_level: Some(ReasoningEffort::Medium),
        supported_reasoning_levels: vec![ReasoningEffortPreset {
            effort: ReasoningEffort::Medium,
            description: ReasoningEffort::Medium.to_string(),
        }],
        shell_type: ConfigShellToolType::ShellCommand,
        visibility: ModelVisibility::List,
        supported_in_api: true,
        input_modalities: default_input_modalities(),
        used_fallback_model_metadata: false,
        supports_search_tool: false,
        use_responses_lite: false,
        auto_review_model_override: auto_review_model_override.map(str::to_string),
        tool_mode: None,
        multi_agent_version: None,
        priority: 1,
        additional_speed_tiers: Vec::new(),
        service_tiers: Vec::new(),
        default_service_tier: None,
        upgrade: None,
        base_instructions: "base instructions".to_string(),
        model_messages: None,
        supports_reasoning_summaries: false,
        default_reasoning_summary: ReasoningSummary::Auto,
        support_verbosity: false,
        default_verbosity: None,
        availability_nux: None,
        apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
        web_search_tool_type: Default::default(),
        truncation_policy: TruncationPolicyConfig::bytes(/*limit*/ 10_000),
        supports_parallel_tool_calls: false,
        supports_image_detail_original: false,
        context_window: Some(272_000),
        max_context_window: None,
        auto_compact_token_limit: None,
        effective_context_window_percent: 95,
        experimental_supported_tools: Vec::new(),
    }
}

fn chat_completions_tool_call_sse(
    id: &str,
    call_id: &str,
    name: &str,
    arguments: serde_json::Value,
) -> String {
    chat_completions_sse(json!({
        "id": id,
        "model": "astral-test-model",
        "choices": [{
            "delta": {
                "role": "assistant",
                "tool_calls": [{
                    "index": 0,
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments.to_string(),
                    },
                }],
            },
            "finish_reason": "tool_calls",
        }],
        "usage": {
            "prompt_tokens": 13,
            "completion_tokens": 5,
            "prompt_tokens_details": { "cached_tokens": 0 },
        },
    }))
}

fn chat_completions_text_sse(id: &str, text: String) -> String {
    chat_completions_sse(json!({
        "id": id,
        "model": "astral-test-model",
        "choices": [{
            "delta": {
                "role": "assistant",
                "content": text,
            },
            "finish_reason": "stop",
        }],
        "usage": {
            "prompt_tokens": 13,
            "completion_tokens": 5,
            "prompt_tokens_details": { "cached_tokens": 0 },
        },
    }))
}

fn chat_completions_sse(chunk: serde_json::Value) -> String {
    format!("data: {chunk}\n\n")
}

async fn mount_chat_completions_sse_sequence(server: &MockServer, bodies: Vec<String>) {
    struct SeqResponder {
        num_calls: AtomicUsize,
        responses: Vec<String>,
    }

    impl Respond for SeqResponder {
        fn respond(&self, _: &wiremock::Request) -> ResponseTemplate {
            let call_num = self.num_calls.fetch_add(1, Ordering::SeqCst);
            match self.responses.get(call_num) {
                Some(body) => ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(body.clone()),
                None => panic!("no chat completions response for {call_num}"),
            }
        }
    }

    let num_calls = bodies.len();
    Mock::given(method("POST"))
        .and(path_regex(".*/chat/completions$"))
        .respond_with(SeqResponder {
            num_calls: AtomicUsize::new(0),
            responses: bodies,
        })
        .up_to_n_times(num_calls as u64)
        .expect(num_calls as u64)
        .mount(server)
        .await;
}
