use crate::agent_adapters::anthropic;
use crate::agent_adapters::chat_completions;
use crate::agent_adapters::chat_completions::ChatCompletionsStreamState;
use crate::common::ModelStreamEvent;
use crate::common::ResponseStream;
use crate::error::ApiError;
use crate::rate_limits::parse_all_rate_limits;
use crate::telemetry::SseTelemetry;
use codex_agent_protocol::AgentStreamEvent;
use codex_agent_protocol::ContentBlock;
use codex_agent_protocol::ContentDelta;
use codex_agent_protocol::StopReason;
use codex_agent_protocol::TokenUsage as AgentTokenUsage;
use codex_client::ByteStream;
use codex_client::StreamResponse;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ReasoningProviderMetadata;
use codex_protocol::models::TranscriptItem;
use codex_protocol::protocol::TokenUsage;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio::time::timeout;
use tracing::debug;
use tracing::trace;

const REQUEST_ID_HEADER: &str = "x-request-id";
const ANTHROPIC_REQUEST_ID_HEADER: &str = "request-id";
const MODELS_ETAG_HEADER: &str = "x-models-etag";
const DEFAULT_RESPONSE_ID: &str = "agent-response";

#[derive(Debug, Clone, Copy)]
pub(crate) enum AgentStreamFormat {
    AnthropicMessages,
    ChatCompletions,
}

pub fn spawn_agent_stream(
    stream_response: StreamResponse,
    idle_timeout: Duration,
    telemetry: Option<Arc<dyn SseTelemetry>>,
    format: AgentStreamFormat,
    tool_name_aliases: BTreeMap<String, String>,
) -> ResponseStream {
    let rate_limit_snapshots = parse_all_rate_limits(&stream_response.headers);
    let upstream_request_id = stream_response
        .headers
        .get(REQUEST_ID_HEADER)
        .or_else(|| stream_response.headers.get(ANTHROPIC_REQUEST_ID_HEADER))
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let models_etag = stream_response
        .headers
        .get(MODELS_ETAG_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let (tx_event, rx_event) = mpsc::channel::<Result<ModelStreamEvent, ApiError>>(1600);

    tokio::spawn(async move {
        for snapshot in rate_limit_snapshots {
            let _ = tx_event
                .send(Ok(ModelStreamEvent::RateLimits(snapshot)))
                .await;
        }
        if let Some(etag) = models_etag {
            let _ = tx_event.send(Ok(ModelStreamEvent::ModelsEtag(etag))).await;
        }
        process_sse(
            stream_response.bytes,
            tx_event,
            idle_timeout,
            telemetry,
            format,
            tool_name_aliases,
        )
        .await;
    });

    ResponseStream {
        rx_event,
        upstream_request_id,
    }
}

struct AgentStreamMapper {
    response_id: Option<String>,
    pending_usage: Option<AgentTokenUsage>,
    blocks: BTreeMap<usize, BlockState>,
    block_order: Vec<usize>,
    tool_name_aliases: BTreeMap<String, String>,
}

impl AgentStreamMapper {
    fn new(tool_name_aliases: BTreeMap<String, String>) -> Self {
        Self {
            response_id: None,
            pending_usage: None,
            blocks: BTreeMap::new(),
            block_order: Vec::new(),
            tool_name_aliases,
        }
    }
}

impl Default for AgentStreamMapper {
    fn default() -> Self {
        Self::new(BTreeMap::new())
    }
}

#[derive(Debug)]
enum BlockState {
    Text {
        id: String,
        text: String,
    },
    Reasoning {
        id: String,
        text: String,
        signature: Option<String>,
    },
    ToolUse {
        id: String,
        name: String,
        initial_input: Value,
        arguments: String,
    },
}

impl AgentStreamMapper {
    fn process_event(
        &mut self,
        event: AgentStreamEvent,
    ) -> Result<Vec<ModelStreamEvent>, ApiError> {
        let mut events = Vec::new();

        match event {
            AgentStreamEvent::MessageStart { id, model, usage } => {
                self.response_id = id;
                self.pending_usage = merge_agent_usage(self.pending_usage.take(), usage);
                events.push(ModelStreamEvent::Created);
                if let Some(model) = model {
                    events.push(ModelStreamEvent::ServerModel(model));
                }
            }
            AgentStreamEvent::ContentBlockStart { index, block } => {
                self.start_block(index, block, &mut events);
            }
            AgentStreamEvent::ContentBlockDelta { index, delta } => {
                self.apply_delta(index, delta, &mut events)?;
            }
            AgentStreamEvent::ContentBlockStop { index } => {
                if let Some(event) = self.finish_block(index) {
                    events.push(event);
                }
            }
            AgentStreamEvent::MessageStop { stop_reason, usage } => {
                if let Some(StopReason::Error { message }) = stop_reason.as_ref() {
                    return Err(ApiError::Stream(message.clone()));
                }
                events.extend(self.finish_all_blocks());
                if matches!(stop_reason, Some(StopReason::MaxTokens)) {
                    events.push(ModelStreamEvent::Warning(
                        "Model output was truncated because the provider reported max_tokens."
                            .to_string(),
                    ));
                }
                let usage = merge_agent_usage(self.pending_usage.take(), usage);
                events.push(ModelStreamEvent::Completed {
                    response_id: self
                        .response_id
                        .clone()
                        .unwrap_or_else(|| DEFAULT_RESPONSE_ID.to_string()),
                    token_usage: usage.map(agent_usage_to_protocol_usage),
                    end_turn: stop_reason.as_ref().map(stop_reason_ends_turn),
                });
            }
        }

        Ok(events)
    }

    fn start_block(
        &mut self,
        index: usize,
        block: ContentBlock,
        events: &mut Vec<ModelStreamEvent>,
    ) {
        if !self.blocks.contains_key(&index) {
            self.block_order.push(index);
        }
        match block {
            ContentBlock::Text { text } => {
                let id = block_item_id("agent-message", index);
                self.blocks.insert(
                    index,
                    BlockState::Text {
                        id: id.clone(),
                        text: text.clone(),
                    },
                );
                events.push(ModelStreamEvent::OutputItemAdded(TranscriptItem::Message {
                    id: Some(id),
                    role: "assistant".to_string(),
                    content: vec![ContentItem::OutputText { text }],
                    phase: None,
                }));
            }
            ContentBlock::Reasoning { text, signature } => {
                let id = block_item_id("agent-reasoning", index);
                self.blocks.insert(
                    index,
                    BlockState::Reasoning {
                        id: id.clone(),
                        text: text.clone(),
                        signature,
                    },
                );
                events.push(ModelStreamEvent::OutputItemAdded(
                    TranscriptItem::Reasoning {
                        id,
                        summary: Vec::new(),
                        content: Some(vec![ReasoningItemContent::ReasoningText { text }]),
                        encrypted_content: None,
                        provider_metadata: None,
                    },
                ));
            }
            ContentBlock::ToolUse { id, name, input } => {
                let name = self.canonical_tool_name(&name);
                self.blocks.insert(
                    index,
                    BlockState::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        initial_input: input.clone(),
                        arguments: String::new(),
                    },
                );
                events.push(ModelStreamEvent::OutputItemAdded(
                    TranscriptItem::FunctionCall {
                        id: Some(id.clone()),
                        name,
                        namespace: None,
                        arguments: json_arguments(&input),
                        call_id: id,
                    },
                ));
            }
            ContentBlock::ToolResult { .. } | ContentBlock::Image { .. } => {}
            ContentBlock::Compaction { .. } => {}
        }
    }

    fn canonical_tool_name(&self, name: &str) -> String {
        self.tool_name_aliases
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    fn apply_delta(
        &mut self,
        index: usize,
        delta: ContentDelta,
        events: &mut Vec<ModelStreamEvent>,
    ) -> Result<(), ApiError> {
        match delta {
            ContentDelta::Text { text } => {
                self.ensure_text_block(index, events);
                if let Some(BlockState::Text {
                    id: _,
                    text: existing,
                }) = self.blocks.get_mut(&index)
                {
                    existing.push_str(&text);
                }
                events.push(ModelStreamEvent::OutputTextDelta(text));
            }
            ContentDelta::Reasoning { text } => {
                self.ensure_reasoning_block(index, events);
                if let Some(BlockState::Reasoning {
                    id: _,
                    text: existing,
                    signature: _,
                }) = self.blocks.get_mut(&index)
                {
                    existing.push_str(&text);
                }
                events.push(ModelStreamEvent::ReasoningContentDelta {
                    delta: text,
                    content_index: index_as_i64(index)?,
                });
            }
            ContentDelta::ReasoningSignature { signature } => {
                self.ensure_reasoning_block(index, events);
                if let Some(BlockState::Reasoning {
                    id: _,
                    text: _,
                    signature: existing,
                }) = self.blocks.get_mut(&index)
                {
                    *existing = Some(signature);
                }
            }
            ContentDelta::ToolInputJson { partial_json } => {
                let Some(BlockState::ToolUse { id, arguments, .. }) = self.blocks.get_mut(&index)
                else {
                    return Err(ApiError::Stream(format!(
                        "tool input delta received before tool block start at index {index}"
                    )));
                };
                arguments.push_str(&partial_json);
                events.push(ModelStreamEvent::ToolCallInputDelta {
                    item_id: id.clone(),
                    call_id: Some(id.clone()),
                    delta: partial_json,
                });
            }
        }

        Ok(())
    }

    fn ensure_text_block(&mut self, index: usize, events: &mut Vec<ModelStreamEvent>) {
        if self.blocks.contains_key(&index) {
            return;
        }
        self.start_block(
            index,
            ContentBlock::Text {
                text: String::new(),
            },
            events,
        );
    }

    fn ensure_reasoning_block(&mut self, index: usize, events: &mut Vec<ModelStreamEvent>) {
        if self.blocks.contains_key(&index) {
            return;
        }
        self.start_block(
            index,
            ContentBlock::Reasoning {
                text: String::new(),
                signature: None,
            },
            events,
        );
    }

    fn finish_block(&mut self, index: usize) -> Option<ModelStreamEvent> {
        self.block_order.retain(|block_index| *block_index != index);
        self.blocks
            .remove(&index)
            .map(block_state_to_output_item_done)
    }

    fn finish_all_blocks(&mut self) -> Vec<ModelStreamEvent> {
        let mut blocks = std::mem::take(&mut self.blocks);
        let mut events = std::mem::take(&mut self.block_order)
            .into_iter()
            .filter_map(|index| blocks.remove(&index))
            .map(block_state_to_output_item_done)
            .collect::<Vec<_>>();
        events.extend(blocks.into_values().map(block_state_to_output_item_done));
        events
    }
}

fn block_state_to_output_item_done(block: BlockState) -> ModelStreamEvent {
    match block {
        BlockState::Text { id, text } => {
            ModelStreamEvent::OutputItemDone(TranscriptItem::Message {
                id: Some(id),
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText { text }],
                phase: None,
            })
        }
        BlockState::Reasoning {
            id,
            text,
            signature,
        } => ModelStreamEvent::OutputItemDone(TranscriptItem::Reasoning {
            id,
            summary: Vec::new(),
            content: Some(vec![ReasoningItemContent::ReasoningText { text }]),
            encrypted_content: None,
            provider_metadata: signature.map(|signature| ReasoningProviderMetadata {
                anthropic_signature: Some(signature),
            }),
        }),
        BlockState::ToolUse {
            id,
            name,
            initial_input,
            arguments,
        } => {
            let arguments = if arguments.is_empty() {
                json_arguments(&initial_input)
            } else {
                arguments
            };
            ModelStreamEvent::OutputItemDone(TranscriptItem::FunctionCall {
                id: Some(id.clone()),
                name,
                namespace: None,
                arguments,
                call_id: id,
            })
        }
    }
}

fn json_arguments(input: &Value) -> String {
    serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string())
}

fn block_item_id(prefix: &str, index: usize) -> String {
    format!("{prefix}-{index}")
}

fn index_as_i64(index: usize) -> Result<i64, ApiError> {
    i64::try_from(index)
        .map_err(|_| ApiError::Stream(format!("content block index {index} exceeds i64")))
}

fn stop_reason_ends_turn(reason: &StopReason) -> bool {
    match reason {
        StopReason::ToolUse => false,
        StopReason::EndTurn
        | StopReason::MaxTokens
        | StopReason::StopSequence
        | StopReason::ContentFilter
        | StopReason::Other { .. }
        | StopReason::Error { .. } => true,
    }
}

fn agent_usage_to_protocol_usage(usage: AgentTokenUsage) -> TokenUsage {
    let uncached_input_tokens = usage.input_tokens.unwrap_or(0);
    let cache_creation_input_tokens = usage.cache_creation_input_tokens.unwrap_or(0);
    let cached_input_tokens = usage.cache_read_input_tokens.unwrap_or(0);
    let input_tokens = uncached_input_tokens
        .saturating_add(cache_creation_input_tokens)
        .saturating_add(cached_input_tokens);
    let output_tokens = usage.output_tokens.unwrap_or(0);

    TokenUsage {
        input_tokens: to_i64(input_tokens),
        cached_input_tokens: to_i64(cached_input_tokens),
        output_tokens: to_i64(output_tokens),
        reasoning_output_tokens: 0,
        total_tokens: to_i64(input_tokens.saturating_add(output_tokens)),
    }
}

fn merge_agent_usage(
    start_usage: Option<AgentTokenUsage>,
    stop_usage: Option<AgentTokenUsage>,
) -> Option<AgentTokenUsage> {
    match (start_usage, stop_usage) {
        (None, None) => None,
        (Some(usage), None) | (None, Some(usage)) => Some(usage),
        (Some(start), Some(stop)) => Some(AgentTokenUsage {
            input_tokens: stop.input_tokens.or(start.input_tokens),
            output_tokens: stop.output_tokens.or(start.output_tokens),
            cache_creation_input_tokens: stop
                .cache_creation_input_tokens
                .or(start.cache_creation_input_tokens),
            cache_read_input_tokens: stop
                .cache_read_input_tokens
                .or(start.cache_read_input_tokens),
        }),
    }
}

fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

pub async fn process_sse(
    stream: ByteStream,
    tx_event: mpsc::Sender<Result<ModelStreamEvent, ApiError>>,
    idle_timeout: Duration,
    telemetry: Option<Arc<dyn SseTelemetry>>,
    format: AgentStreamFormat,
    tool_name_aliases: BTreeMap<String, String>,
) {
    let mut stream = stream.eventsource();
    let mut mapper = AgentStreamMapper::new(tool_name_aliases);
    let mut chat_stream_state = ChatCompletionsStreamState::default();
    let mut pending_chat_stop_reason: Option<StopReason> = None;
    let mut pending_chat_usage: Option<AgentTokenUsage> = None;

    loop {
        let start = Instant::now();
        let response = timeout(idle_timeout, stream.next()).await;
        if let Some(t) = telemetry.as_ref() {
            t.on_sse_poll(&response, start.elapsed());
        }
        let sse = match response {
            Ok(Some(Ok(sse))) => sse,
            Ok(Some(Err(e))) => {
                debug!("SSE Error: {e:#}");
                let _ = tx_event.send(Err(ApiError::Stream(e.to_string()))).await;
                return;
            }
            Ok(None) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream(
                        "stream closed before response.completed".into(),
                    )))
                    .await;
                return;
            }
            Err(_) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream("idle timeout waiting for SSE".into())))
                    .await;
                return;
            }
        };

        trace!("SSE event: {}", &sse.data);
        if matches!(format, AgentStreamFormat::ChatCompletions) && sse.data.trim() == "[DONE]" {
            let stop_reason = pending_chat_stop_reason
                .take()
                .unwrap_or(StopReason::EndTurn);
            let should_continue = send_agent_stream_event(
                &mut mapper,
                AgentStreamEvent::MessageStop {
                    stop_reason: Some(stop_reason),
                    usage: pending_chat_usage.take(),
                },
                &tx_event,
            )
            .await;
            if should_continue {
                continue;
            }
            return;
        }

        let value: Value = match serde_json::from_str(&sse.data) {
            Ok(value) => value,
            Err(e) => {
                let message = format!("failed to parse agent SSE event: {e}");
                debug!("{message}, data: {}", &sse.data);
                let _ = tx_event.send(Err(ApiError::Stream(message))).await;
                return;
            }
        };

        let agent_events = match format {
            AgentStreamFormat::AnthropicMessages => match anthropic::parse_stream_event(value) {
                Ok(Some(event)) => vec![event],
                Ok(None) => Vec::new(),
                Err(error) => {
                    let _ = tx_event
                        .send(Err(ApiError::Stream(error.to_string())))
                        .await;
                    return;
                }
            },
            AgentStreamFormat::ChatCompletions => {
                match chat_completions::parse_stream_chunk_with_state(value, &mut chat_stream_state)
                {
                    Ok(events) => events,
                    Err(error) => {
                        let api_error = match error {
                            chat_completions::ChatCompletionsStreamError::ContextWindowExceeded => {
                                ApiError::ContextWindowExceeded
                            }
                            chat_completions::ChatCompletionsStreamError::QuotaExceeded => {
                                ApiError::QuotaExceeded
                            }
                            error => ApiError::Stream(error.to_string()),
                        };
                        let _ = tx_event.send(Err(api_error)).await;
                        return;
                    }
                }
            }
        };

        for event in agent_events {
            let event = if matches!(format, AgentStreamFormat::ChatCompletions) {
                match event {
                    AgentStreamEvent::MessageStop {
                        stop_reason: Some(stop_reason),
                        usage: None,
                    } => {
                        pending_chat_stop_reason = Some(stop_reason);
                        continue;
                    }
                    AgentStreamEvent::MessageStop {
                        stop_reason: Some(stop_reason),
                        usage: Some(usage),
                    } => {
                        pending_chat_stop_reason = Some(stop_reason);
                        pending_chat_usage = Some(usage);
                        let stop_reason = pending_chat_stop_reason.take();
                        let usage = pending_chat_usage.take();
                        AgentStreamEvent::MessageStop { stop_reason, usage }
                    }
                    AgentStreamEvent::MessageStop {
                        stop_reason: None,
                        usage: Some(usage),
                    } => {
                        pending_chat_usage = Some(usage);
                        if pending_chat_stop_reason.is_none() {
                            continue;
                        }
                        let stop_reason = pending_chat_stop_reason.take();
                        let usage = pending_chat_usage.take();
                        AgentStreamEvent::MessageStop { stop_reason, usage }
                    }
                    event => event,
                }
            } else {
                event
            };
            if !send_agent_stream_event(&mut mapper, event, &tx_event).await {
                return;
            }
        }
    }
}

async fn send_agent_stream_event(
    mapper: &mut AgentStreamMapper,
    event: AgentStreamEvent,
    tx_event: &mpsc::Sender<Result<ModelStreamEvent, ApiError>>,
) -> bool {
    let response_events = match mapper.process_event(event) {
        Ok(events) => events,
        Err(error) => {
            let _ = tx_event.send(Err(error)).await;
            return false;
        }
    };
    for event in response_events {
        let is_completed = matches!(event, ModelStreamEvent::Completed { .. });
        if tx_event.send(Ok(event)).await.is_err() {
            return false;
        }
        if is_completed {
            return false;
        }
    }
    true
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
