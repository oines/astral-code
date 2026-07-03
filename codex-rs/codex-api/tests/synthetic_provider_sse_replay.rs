use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use codex_api::AgentClient;
use codex_api::AgentOptions;
use codex_api::AuthProvider;
use codex_api::ModelStreamEvent;
use codex_api::Provider;
use codex_api::agent_adapters::anthropic::AnthropicMessagesOptions;
use codex_api::agent_adapters::chat_completions::ChatCompletionsOptions;
use codex_api::agent_protocol::AgentMessage;
use codex_api::agent_protocol::AgentRequest;
use codex_api::agent_protocol::ContentBlock;
use codex_api::agent_protocol::MessageRole;
use codex_client::HttpTransport;
use codex_client::Request;
use codex_client::Response;
use codex_client::StreamResponse;
use codex_client::TransportError;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::TranscriptItem;
use futures::StreamExt;
use http::HeaderMap;
use http::StatusCode;
use pretty_assertions::assert_eq;
use tokio::time::timeout;

const DEEPSEEK_CHAT: &str = include_str!("fixtures/synthetic_deepseek_chat_completions.sse");
const DEEPSEEK_ANTHROPIC: &str = include_str!("fixtures/synthetic_deepseek_anthropic_messages.sse");
const SILICONFLOW_CHAT: &str = include_str!("fixtures/synthetic_siliconflow_chat_completions.sse");
const GLM_CHAT: &str = include_str!("fixtures/synthetic_glm_chat_completions.sse");
const KIMI_CHAT: &str = include_str!("fixtures/synthetic_kimi_chat_completions.sse");
const MINIMAX_CHAT: &str = include_str!("fixtures/synthetic_minimax_chat_completions.sse");

#[derive(Clone, Copy)]
enum WireApi {
    AnthropicMessages,
    ChatCompletions,
}

struct SyntheticFixture {
    name: &'static str,
    wire_api: WireApi,
    body: &'static str,
    expected_path_suffix: &'static str,
    expected_events: &'static [&'static str],
}

#[derive(Clone)]
struct FixtureTransport {
    body: &'static str,
    requests: Arc<Mutex<Vec<String>>>,
}

#[derive(Clone, Default)]
struct NoAuth;

impl AuthProvider for NoAuth {
    fn add_auth_headers(&self, _headers: &mut HeaderMap) {}
}

#[async_trait]
impl HttpTransport for FixtureTransport {
    async fn execute(&self, _req: Request) -> Result<Response, TransportError> {
        Err(TransportError::Build("execute should not run".to_string()))
    }

    async fn stream(&self, req: Request) -> Result<StreamResponse, TransportError> {
        self.requests
            .lock()
            .unwrap_or_else(|err| panic!("mutex poisoned: {err}"))
            .push(req.url);
        let stream = futures::stream::iter(vec![Ok(Bytes::from_static(self.body.as_bytes()))]);
        Ok(StreamResponse {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            bytes: Box::pin(stream),
        })
    }
}

#[tokio::test]
async fn synthetic_provider_sse_fixtures_replay_to_model_stream_events() -> Result<()> {
    for fixture in synthetic_fixtures() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let transport = FixtureTransport {
            body: fixture.body,
            requests: Arc::clone(&requests),
        };
        let client = AgentClient::new(transport, provider(fixture.name), Arc::new(NoAuth));

        let stream = match fixture.wire_api {
            WireApi::AnthropicMessages => {
                client
                    .stream_anthropic_messages(
                        agent_request(fixture.name),
                        anthropic_options(),
                        AgentOptions::default(),
                    )
                    .await?
            }
            WireApi::ChatCompletions => {
                client
                    .stream_chat_completions(
                        agent_request(fixture.name),
                        ChatCompletionsOptions::default(),
                        AgentOptions::default(),
                    )
                    .await?
            }
        };

        let summaries = collect_event_summaries(stream).await?;
        let requests = requests
            .lock()
            .unwrap_or_else(|err| panic!("mutex poisoned: {err}"))
            .clone();
        assert_eq!(requests.len(), 1, "fixture {}", fixture.name);
        assert!(
            requests[0].ends_with(fixture.expected_path_suffix),
            "fixture {} sent request to {}",
            fixture.name,
            requests[0]
        );
        let expected = fixture
            .expected_events
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>();
        assert_eq!(summaries, expected, "fixture {}", fixture.name);
    }

    Ok(())
}

fn synthetic_fixtures() -> Vec<SyntheticFixture> {
    vec![
        SyntheticFixture {
            name: "deepseek-chat-synthetic",
            wire_api: WireApi::ChatCompletions,
            body: DEEPSEEK_CHAT,
            expected_path_suffix: "/chat/completions",
            expected_events: &[
                "created",
                "server_model=deepseek-chat",
                "reasoning_delta=inspect",
                "text_delta=done",
                "reasoning_done=inspect|sig=<none>",
                "message_done=done",
                "completed=deepseek-chat-synth-1|input=5 cached=0 output=3 total=8 end_turn=Some(true)",
            ],
        },
        SyntheticFixture {
            name: "deepseek-anthropic-synthetic",
            wire_api: WireApi::AnthropicMessages,
            body: DEEPSEEK_ANTHROPIC,
            expected_path_suffix: "/messages",
            expected_events: &[
                "created",
                "server_model=deepseek-reasoner",
                "reasoning_delta=think",
                "reasoning_done=think|sig=deepseek-synthetic-signature",
                "text_delta=answer",
                "message_done=answer",
                "completed=deepseek-anthropic-synth-1|input=7 cached=0 output=2 total=9 end_turn=Some(true)",
            ],
        },
        SyntheticFixture {
            name: "siliconflow-chat-synthetic",
            wire_api: WireApi::ChatCompletions,
            body: SILICONFLOW_CHAT,
            expected_path_suffix: "/chat/completions",
            expected_events: &[
                "created",
                "server_model=sf-chat",
                "text_delta=silicon",
                "text_delta= flow",
                "message_done=silicon flow",
                "completed=siliconflow-synth-1|input=9 cached=0 output=2 total=11 end_turn=Some(true)",
            ],
        },
        SyntheticFixture {
            name: "glm-chat-synthetic",
            wire_api: WireApi::ChatCompletions,
            body: GLM_CHAT,
            expected_path_suffix: "/chat/completions",
            expected_events: &[
                "created",
                "server_model=glm-4.5",
                "tool_delta=call_glm:{\"command\"",
                "tool_delta=call_glm::\"pwd\"}",
                "tool_done=Bash:{\"command\":\"pwd\"}",
                "completed=glm-synth-1|input=11 cached=0 output=4 total=15 end_turn=Some(false)",
            ],
        },
        SyntheticFixture {
            name: "kimi-chat-synthetic",
            wire_api: WireApi::ChatCompletions,
            body: KIMI_CHAT,
            expected_path_suffix: "/chat/completions",
            expected_events: &[
                "created",
                "server_model=kimi-k2",
                "reasoning_delta=inspect",
                "reasoning_delta= decide",
                "text_delta=kimi answer",
                "reasoning_done=inspect decide|sig=<none>",
                "message_done=kimi answer",
                "completed=kimi-synth-1|input=13 cached=0 output=5 total=18 end_turn=Some(true)",
            ],
        },
        SyntheticFixture {
            name: "minimax-chat-synthetic",
            wire_api: WireApi::ChatCompletions,
            body: MINIMAX_CHAT,
            expected_path_suffix: "/chat/completions",
            expected_events: &[
                "created",
                "server_model=minimax-text",
                "text_delta=mini",
                "text_delta=max",
                "message_done=minimax",
                "completed=minimax-synth-1|usage=<none> end_turn=Some(true)",
            ],
        },
    ]
}

async fn collect_event_summaries(mut stream: codex_api::ResponseStream) -> Result<Vec<String>> {
    let mut summaries = Vec::new();
    while let Some(event) = timeout(Duration::from_secs(2), stream.next()).await? {
        let event = event?;
        let is_completed = matches!(event, ModelStreamEvent::Completed { .. });
        if let Some(summary) = event_summary(&event) {
            summaries.push(summary);
        }
        if is_completed {
            break;
        }
    }
    Ok(summaries)
}

fn event_summary(event: &ModelStreamEvent) -> Option<String> {
    match event {
        ModelStreamEvent::Created => Some("created".to_string()),
        ModelStreamEvent::ServerModel(model) => Some(format!("server_model={model}")),
        ModelStreamEvent::OutputTextDelta(text) => Some(format!("text_delta={text}")),
        ModelStreamEvent::ReasoningContentDelta { delta, .. } => {
            Some(format!("reasoning_delta={delta}"))
        }
        ModelStreamEvent::ToolCallInputDelta { call_id, delta, .. } => Some(format!(
            "tool_delta={}:{}",
            call_id.as_deref().unwrap_or("<none>"),
            delta
        )),
        ModelStreamEvent::OutputItemDone(item) => output_item_summary(item),
        ModelStreamEvent::Completed {
            response_id,
            token_usage,
            end_turn,
        } => Some(completed_summary(
            response_id,
            token_usage.as_ref(),
            *end_turn,
        )),
        ModelStreamEvent::OutputItemAdded(_)
        | ModelStreamEvent::RateLimits(_)
        | ModelStreamEvent::ModelsEtag(_)
        | ModelStreamEvent::ModelVerifications(_)
        | ModelStreamEvent::TurnModerationMetadata(_)
        | ModelStreamEvent::ServerReasoningIncluded(_)
        | ModelStreamEvent::Warning(_)
        | ModelStreamEvent::ReasoningSummaryDelta { .. }
        | ModelStreamEvent::ReasoningSummaryPartAdded { .. } => None,
    }
}

fn output_item_summary(item: &TranscriptItem) -> Option<String> {
    match item {
        TranscriptItem::Message { content, .. } => {
            let text = content
                .iter()
                .filter_map(|item| match item {
                    ContentItem::OutputText { text } => Some(text.as_str()),
                    ContentItem::InputText { .. } | ContentItem::InputImage { .. } => None,
                })
                .collect::<String>();
            Some(format!("message_done={text}"))
        }
        TranscriptItem::Reasoning {
            content,
            provider_metadata,
            ..
        } => {
            let text = content
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|item| match item {
                    ReasoningItemContent::ReasoningText { text }
                    | ReasoningItemContent::Text { text } => text.as_str(),
                })
                .collect::<String>();
            let signature = provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.anthropic_signature.as_deref())
                .unwrap_or("<none>");
            Some(format!("reasoning_done={text}|sig={signature}"))
        }
        TranscriptItem::FunctionCall {
            name, arguments, ..
        } => Some(format!("tool_done={name}:{arguments}")),
        TranscriptItem::AgentMessage { .. }
        | TranscriptItem::LocalShellCall { .. }
        | TranscriptItem::FunctionCallOutput { .. }
        | TranscriptItem::ToolSearchCall { .. }
        | TranscriptItem::CustomToolCall { .. }
        | TranscriptItem::CustomToolCallOutput { .. }
        | TranscriptItem::ToolSearchOutput { .. }
        | TranscriptItem::WebSearchCall { .. }
        | TranscriptItem::ImageGenerationCall { .. }
        | TranscriptItem::Compaction { .. }
        | TranscriptItem::CompactionTrigger
        | TranscriptItem::ContextCompaction { .. }
        | TranscriptItem::Other => None,
    }
}

fn completed_summary(
    response_id: &str,
    usage: Option<&codex_protocol::protocol::TokenUsage>,
    end_turn: Option<bool>,
) -> String {
    match usage {
        Some(usage) => format!(
            "completed={response_id}|input={} cached={} output={} total={} end_turn={end_turn:?}",
            usage.input_tokens, usage.cached_input_tokens, usage.output_tokens, usage.total_tokens
        ),
        None => format!("completed={response_id}|usage=<none> end_turn={end_turn:?}"),
    }
}

fn provider(name: &str) -> Provider {
    Provider {
        name: name.to_string(),
        base_url: "https://example.com/v1".to_string(),
        query_params: None,
        headers: HeaderMap::new(),
        retry: codex_api::RetryConfig {
            max_attempts: 1,
            base_delay: Duration::from_millis(1),
            retry_429: false,
            retry_5xx: false,
            retry_transport: false,
        },
        stream_idle_timeout: Duration::from_secs(2),
    }
}

fn agent_request(model: &str) -> AgentRequest {
    AgentRequest {
        model: model.to_string(),
        messages: vec![AgentMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: "say hi".to_string(),
            }],
            id: None,
        }],
        ..Default::default()
    }
}

fn anthropic_options() -> AnthropicMessagesOptions {
    AnthropicMessagesOptions {
        max_tokens: 4096,
        supports_image_input: true,
        cache_fold: None,
        compact_input_placeholders: false,
    }
}
