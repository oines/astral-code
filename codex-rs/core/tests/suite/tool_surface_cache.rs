#![allow(clippy::expect_used)]

use anyhow::Result;
use codex_core::LoadedAgentsMd;
use codex_core::config::ToolSurface;
use codex_features::Feature;
use core_test_support::responses::ResponsesRequest;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::request_tool_names;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use sha1::Digest;
use sha1::Sha1;

#[derive(Clone, Copy, Debug)]
enum SurfaceScenario {
    Claude,
    Codex,
    CodeMode,
    CodeModeOnly,
}

impl SurfaceScenario {
    fn configure(self, config: &mut codex_core::config::Config) {
        config.user_instructions = Some(LoadedAgentsMd::from_text_for_testing(
            "SURFACE_CACHE_AGENTS",
        ));
        match self {
            Self::Claude => config.tool_surface = ToolSurface::Claude,
            Self::Codex => config.tool_surface = ToolSurface::Codex,
            Self::CodeMode => {
                config.tool_surface = ToolSurface::Claude;
                config
                    .features
                    .enable(Feature::CodeMode)
                    .expect("enable code mode");
            }
            Self::CodeModeOnly => {
                config.tool_surface = ToolSurface::Claude;
                config
                    .features
                    .enable(Feature::CodeMode)
                    .expect("enable code mode");
                config
                    .features
                    .enable(Feature::CodeModeOnly)
                    .expect("enable code mode only");
            }
        }
    }

    fn expected_tool_names(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &[
                "Bash",
                "ReadTaskOutput",
                "SendTaskInput",
                "ListBackgroundTasks",
                "StopBackgroundTask",
                "Read",
                "Write",
                "Edit",
                "Glob",
                "Grep",
                "TodoWrite",
                "Skill",
                "AskUserQuestion",
                "RequestPermissions",
                "tool_search",
            ],
            Self::Codex => &[
                "exec_command",
                "write_stdin",
                "update_plan",
                "request_user_input",
                "request_permissions",
                "apply_patch",
                "view_image",
                "tool_search",
            ],
            Self::CodeMode => &[
                "exec",
                "wait",
                "exec_command",
                "write_stdin",
                "update_plan",
                "request_user_input",
                "request_permissions",
                "apply_patch",
                "view_image",
                "tool_search",
            ],
            Self::CodeModeOnly => &["exec", "wait", "request_user_input"],
        }
    }

    fn expected_tools_hash(self) -> &'static str {
        match self {
            Self::Claude => "561a0dd2631d6cd07405b570b5a2337caaa06140",
            Self::Codex => "38925bfc2843385d7e9e03708d686a1879d9524c",
            Self::CodeMode => "041890ec3737fac1dfc582cd7131c792f09d7624",
            Self::CodeModeOnly => "9c31f41947a17118422d1c611df14bdbe21f5b46",
        }
    }
}

fn tools_hash(body: &Value) -> String {
    let tools = serde_json::to_vec(body.get("tools").unwrap_or(&Value::Null))
        .expect("serialize request tools");
    format!("{:x}", Sha1::digest(tools))
}

fn stable_context_prefix(body: &Value, first_prompt: &str) -> Vec<Value> {
    let input = body
        .get("input")
        .and_then(Value::as_array)
        .expect("request input array");
    let prompt_index = input
        .iter()
        .position(|item| item.to_string().contains(first_prompt))
        .expect("first prompt in request input");
    input[..prompt_index].to_vec()
}

fn assert_world_state_order(request: &ResponsesRequest) {
    let texts = request.message_input_texts("user");
    let agents_index = texts
        .iter()
        .position(|text| text.contains("SURFACE_CACHE_AGENTS"))
        .expect("AGENTS.md world-state fragment");
    let environment_index = texts
        .iter()
        .position(|text| text.starts_with("<environment_context>"))
        .expect("environment world-state fragment");
    assert!(agents_index < environment_index);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_requests_pin_tool_surface_order_schema_and_stable_prefix() -> Result<()> {
    skip_if_no_network!(Ok(()));

    for scenario in [
        SurfaceScenario::Claude,
        SurfaceScenario::Codex,
        SurfaceScenario::CodeMode,
        SurfaceScenario::CodeModeOnly,
    ] {
        let server = start_mock_server().await;
        let response_mock = mount_sse_sequence(
            &server,
            vec![
                sse(vec![
                    ev_response_created("resp-1"),
                    ev_assistant_message("msg-1", "done one"),
                    ev_completed("resp-1"),
                ]),
                sse(vec![
                    ev_response_created("resp-2"),
                    ev_assistant_message("msg-2", "done two"),
                    ev_completed("resp-2"),
                ]),
            ],
        )
        .await;
        let mut builder = test_codex().with_config(move |config| scenario.configure(config));
        let test = builder.build(&server).await?;
        test.submit_turn("surface cache turn one").await?;
        test.submit_turn("surface cache turn two").await?;

        let requests = response_mock.requests();
        assert_eq!(requests.len(), 2);
        let first = requests[0].body_json();
        let second = requests[1].body_json();
        assert_eq!(first.get("tools"), second.get("tools"));
        assert_eq!(tools_hash(&first), tools_hash(&second));
        assert_eq!(
            stable_context_prefix(&first, "surface cache turn one"),
            stable_context_prefix(&second, "surface cache turn one")
        );
        assert_world_state_order(&requests[0]);
        assert_world_state_order(&requests[1]);
        assert_eq!(
            request_tool_names(&first),
            scenario.expected_tool_names(),
            "unexpected tool order for {scenario:?}"
        );
        assert_eq!(
            tools_hash(&first),
            scenario.expected_tools_hash(),
            "unexpected tool schema hash for {scenario:?}"
        );
    }

    Ok(())
}
