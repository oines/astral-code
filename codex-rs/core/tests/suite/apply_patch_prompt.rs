#![allow(clippy::expect_used)]

use anyhow::Result;
use codex_core::config::ToolSurface;
use codex_features::Feature;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use core_test_support::responses;
use core_test_support::responses::ResponseMock;
use core_test_support::responses::ResponsesRequest;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use serde_json::json;
use wiremock::Mock;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

#[derive(Clone, Copy, Debug)]
enum ProtocolScenario {
    ChatCompletions,
    AnthropicMessages,
}

#[derive(Clone, Copy, Debug)]
enum SurfaceScenario {
    Claude,
    Codex,
    CodeMode,
    CodeModeOnly,
}

impl SurfaceScenario {
    fn configure(self, config: &mut codex_core::config::Config) {
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

    fn expected_teaching(self) -> ApplyPatchTeaching {
        match self {
            Self::Claude => ApplyPatchTeaching::Excluded,
            Self::Codex | Self::CodeMode | Self::CodeModeOnly => ApplyPatchTeaching::Included,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum ApplyPatchTeaching {
    Excluded,
    Included,
}

const APPLY_PATCH_GRAMMAR_GUIDANCE: &str =
    "Your patch language is a stripped‑down, file‑oriented diff format";
const APPLY_PATCH_TOOL_GUIDANCE: &str = "Use the `apply_patch` tool to edit files.";
const UPSTREAM_SHELL_APPLY_PATCH_GUIDANCE: &str =
    "Use the `apply_patch` shell command to edit files.";
const APPLY_PATCH_INTERFACE_GUIDANCE: &str =
    "Use the `apply_patch` interface exposed by the current tool definitions";
const DIRECT_APPLY_PATCH_GUIDANCE: &str =
    "If `apply_patch` is available as a top-level tool, follow that tool's schema";
const CODE_MODE_APPLY_PATCH_GUIDANCE: &str =
    "If `apply_patch` is available through Code Mode, invoke it from `exec` as a nested tool";
const PROVIDER_NEUTRAL_APPLY_PATCH_DESCRIPTION: &str = "Use the `apply_patch` tool to edit files. Set the `input` string to the complete raw patch text, including the `*** Begin Patch` and `*** End Patch` envelope.";

fn provider(server: &wiremock::MockServer, protocol: ProtocolScenario) -> ModelProviderInfo {
    let wire_api = match protocol {
        ProtocolScenario::ChatCompletions => WireApi::ChatCompletions,
        ProtocolScenario::AnthropicMessages => WireApi::AnthropicMessages,
    };
    ModelProviderInfo {
        name: format!("{protocol:?} prompt test provider"),
        base_url: Some(format!("{}/v1", server.uri())),
        wire_api,
        supports_websockets: false,
        ..ModelProviderInfo::default()
    }
}

fn anthropic_text_sse(text: &str) -> String {
    [
        json!({
            "type": "message_start",
            "message": {
                "id": "msg-prompt",
                "model": "deepseek-v4-pro",
                "usage": { "input_tokens": 1 }
            }
        }),
        json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": { "type": "text", "text": "" }
        }),
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "text_delta", "text": text }
        }),
        json!({ "type": "content_block_stop", "index": 0 }),
        json!({
            "type": "message_delta",
            "delta": { "stop_reason": "end_turn" },
            "usage": { "output_tokens": 1 }
        }),
        json!({ "type": "message_stop" }),
    ]
    .into_iter()
    .map(|event| format!("data: {event}\n\n"))
    .collect()
}

fn tool_description(
    request: &ResponsesRequest,
    protocol: ProtocolScenario,
    tool_name: &str,
) -> Option<String> {
    let body = request.body_json();
    body.get("tools")
        .and_then(serde_json::Value::as_array)?
        .iter()
        .find_map(|tool| match protocol {
            ProtocolScenario::ChatCompletions => (tool
                .pointer("/function/name")
                .and_then(serde_json::Value::as_str)
                == Some(tool_name))
            .then(|| {
                tool.pointer("/function/description")
                    .and_then(serde_json::Value::as_str)
            })
            .flatten(),
            ProtocolScenario::AnthropicMessages => {
                (tool.get("name").and_then(serde_json::Value::as_str) == Some(tool_name))
                    .then(|| tool.get("description").and_then(serde_json::Value::as_str))
                    .flatten()
            }
        })
        .map(str::to_string)
}

async fn mount_anthropic_text_once(server: &wiremock::MockServer, text: &str) -> ResponseMock {
    let response_mock = ResponseMock::new();
    Mock::given(method("POST"))
        .and(path_regex(".*/messages$"))
        .and(response_mock.clone())
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(anthropic_text_sse(text), "text/event-stream"),
        )
        .expect(1)
        .mount(server)
        .await;
    response_mock
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_requests_select_apply_patch_prompt_from_effective_tool_surface() -> Result<()> {
    skip_if_no_network!(Ok(()));

    for protocol in [
        ProtocolScenario::ChatCompletions,
        ProtocolScenario::AnthropicMessages,
    ] {
        for surface in [
            SurfaceScenario::Claude,
            SurfaceScenario::Codex,
            SurfaceScenario::CodeMode,
            SurfaceScenario::CodeModeOnly,
        ] {
            let server = responses::start_mock_server().await;
            let response_mock = match protocol {
                ProtocolScenario::ChatCompletions => {
                    responses::mount_chat_completions_text_once(&server, "done").await
                }
                ProtocolScenario::AnthropicMessages => {
                    mount_anthropic_text_once(&server, "done").await
                }
            };
            let model_provider = provider(&server, protocol);
            let test = test_codex()
                .with_model("deepseek-v4-pro")
                .with_config(move |config| {
                    config.model_provider = model_provider;
                    surface.configure(config);
                })
                .build(&server)
                .await?;

            test.submit_turn("check apply_patch prompt selection")
                .await?;

            let request = response_mock.single_request();
            let instructions = match protocol {
                ProtocolScenario::ChatCompletions => request.instructions_text(),
                ProtocolScenario::AnthropicMessages => request
                    .body_json()
                    .get("system")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|block| block.get("text").and_then(serde_json::Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            };
            assert!(
                instructions.starts_with("You are Astral, an agentic coding assistant"),
                "missing Astral base prompt for {protocol:?}/{surface:?}"
            );
            assert!(
                instructions.contains("Report honestly. Say what changed, what you verified"),
                "incomplete Astral base prompt for {protocol:?}/{surface:?}"
            );
            match surface.expected_teaching() {
                ApplyPatchTeaching::Excluded => assert!(
                    !instructions.contains(APPLY_PATCH_GRAMMAR_GUIDANCE),
                    "unexpected apply_patch teaching for {protocol:?}/{surface:?}"
                ),
                ApplyPatchTeaching::Included => {
                    for guidance in [
                        APPLY_PATCH_GRAMMAR_GUIDANCE,
                        APPLY_PATCH_TOOL_GUIDANCE,
                        APPLY_PATCH_INTERFACE_GUIDANCE,
                        DIRECT_APPLY_PATCH_GUIDANCE,
                        CODE_MODE_APPLY_PATCH_GUIDANCE,
                    ] {
                        assert!(
                            instructions.contains(guidance),
                            "missing apply_patch teaching for {protocol:?}/{surface:?}: {guidance}"
                        );
                    }
                    assert!(
                        !instructions.contains(UPSTREAM_SHELL_APPLY_PATCH_GUIDANCE),
                        "shell-specific apply_patch teaching leaked into {protocol:?}/{surface:?}"
                    );
                }
            }

            let expected_direct_description = match surface {
                SurfaceScenario::Codex | SurfaceScenario::CodeMode => {
                    Some(PROVIDER_NEUTRAL_APPLY_PATCH_DESCRIPTION.to_string())
                }
                SurfaceScenario::Claude | SurfaceScenario::CodeModeOnly => None,
            };
            assert_eq!(
                tool_description(&request, protocol, "apply_patch"),
                expected_direct_description,
                "unexpected direct apply_patch tool for {protocol:?}/{surface:?}"
            );
        }
    }

    Ok(())
}
