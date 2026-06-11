use codex_agent_protocol::AgentMessage;
use codex_agent_protocol::AgentRequest;
use codex_agent_protocol::AgentStreamEvent;
use codex_agent_protocol::AgentTool;
use codex_agent_protocol::ContentBlock;
use codex_agent_protocol::ContentDelta;
use codex_agent_protocol::ImageSource;
use codex_agent_protocol::MessageRole;
use codex_agent_protocol::StopReason;
use codex_agent_protocol::TokenUsage;
use codex_agent_protocol::ToolChoice;
use codex_agent_protocol::ToolResultContent;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnthropicMessagesOptions {
    pub max_tokens: u64,
}

pub fn to_messages_request(request: &AgentRequest, options: AnthropicMessagesOptions) -> Value {
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(request.model.clone()));
    body.insert("max_tokens".to_string(), Value::from(options.max_tokens));
    body.insert("stream".to_string(), Value::Bool(request.stream));

    let cache_control_enabled = request.metadata.prompt_cache_key.is_some();

    let system = system_blocks(request, cache_control_enabled);
    if !system.is_empty() {
        body.insert("system".to_string(), Value::Array(system));
    }

    let mut messages = request
        .messages
        .iter()
        .filter_map(message_to_anthropic)
        .collect::<Vec<_>>();
    if cache_control_enabled {
        add_cache_control_to_last_message_block(&mut messages);
    }
    body.insert("messages".to_string(), Value::Array(messages));

    if !request.tools.is_empty() {
        let mut tools = request
            .tools
            .iter()
            .map(tool_to_anthropic)
            .collect::<Vec<_>>();
        if cache_control_enabled {
            add_cache_control_to_last_object(&mut tools);
        }
        body.insert("tools".to_string(), Value::Array(tools));
    }

    body.insert(
        "tool_choice".to_string(),
        tool_choice_to_anthropic(&request.tool_choice),
    );
    apply_provider_body_overrides(&mut body, request);

    Value::Object(body)
}

pub fn parse_stream_event(value: Value) -> Result<Option<AgentStreamEvent>, AnthropicStreamError> {
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or(AnthropicStreamError::MissingType)?;

    match event_type {
        "message_start" => Ok(Some(AgentStreamEvent::MessageStart {
            id: value
                .pointer("/message/id")
                .and_then(Value::as_str)
                .map(str::to_string),
            model: value
                .pointer("/message/model")
                .and_then(Value::as_str)
                .map(str::to_string),
        })),
        "content_block_start" => Ok(Some(AgentStreamEvent::ContentBlockStart {
            index: required_usize(&value, "index")?,
            block: content_block_from_anthropic(required_value(&value, "content_block")?)?,
        })),
        "content_block_delta" => Ok(Some(AgentStreamEvent::ContentBlockDelta {
            index: required_usize(&value, "index")?,
            delta: content_delta_from_anthropic(required_value(&value, "delta")?)?,
        })),
        "content_block_stop" => Ok(Some(AgentStreamEvent::ContentBlockStop {
            index: required_usize(&value, "index")?,
        })),
        "message_delta" => Ok(Some(AgentStreamEvent::MessageStop {
            stop_reason: value
                .pointer("/delta/stop_reason")
                .and_then(Value::as_str)
                .map(stop_reason_from_anthropic),
            usage: usage_from_anthropic(value.get("usage")),
        })),
        "message_stop" | "ping" => Ok(None),
        "error" => Ok(Some(AgentStreamEvent::MessageStop {
            stop_reason: Some(StopReason::Error {
                message: value
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("anthropic stream error")
                    .to_string(),
            }),
            usage: None,
        })),
        other => Err(AnthropicStreamError::UnknownEvent(other.to_string())),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnthropicStreamError {
    MissingType,
    MissingField(&'static str),
    InvalidField(&'static str),
    UnknownEvent(String),
    UnknownContentBlock(String),
    UnknownContentDelta(String),
}

impl std::fmt::Display for AnthropicStreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnthropicStreamError::MissingType => write!(f, "anthropic stream event missing type"),
            AnthropicStreamError::MissingField(field) => {
                write!(f, "anthropic stream event missing field {field}")
            }
            AnthropicStreamError::InvalidField(field) => {
                write!(f, "anthropic stream event has invalid field {field}")
            }
            AnthropicStreamError::UnknownEvent(event) => {
                write!(f, "unknown anthropic stream event {event}")
            }
            AnthropicStreamError::UnknownContentBlock(block) => {
                write!(f, "unknown anthropic content block {block}")
            }
            AnthropicStreamError::UnknownContentDelta(delta) => {
                write!(f, "unknown anthropic content delta {delta}")
            }
        }
    }
}

impl std::error::Error for AnthropicStreamError {}

fn system_blocks(request: &AgentRequest, cache_control_enabled: bool) -> Vec<Value> {
    let message_blocks = request
        .messages
        .iter()
        .filter(|message| is_system_role(&message.role))
        .flat_map(|message| message.content.iter());

    let mut blocks = request
        .instructions
        .iter()
        .chain(message_blocks)
        .map(content_block_to_anthropic)
        .collect::<Vec<_>>();
    if cache_control_enabled {
        add_cache_control_to_last_object(&mut blocks);
    }
    blocks
}

fn is_system_role(role: &MessageRole) -> bool {
    match role {
        MessageRole::System | MessageRole::Developer => true,
        MessageRole::User | MessageRole::Assistant => false,
    }
}

fn message_to_anthropic(message: &AgentMessage) -> Option<Value> {
    let role = match message.role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System | MessageRole::Developer => return None,
    };

    Some(json!({
        "role": role,
        "content": message
            .content
            .iter()
            .map(content_block_to_anthropic)
            .collect::<Vec<_>>()
    }))
}

fn tool_to_anthropic(tool: &AgentTool) -> Value {
    let mut value = Map::new();
    value.insert("name".to_string(), Value::String(tool.name.clone()));
    value.insert(
        "description".to_string(),
        Value::String(tool.description.clone()),
    );
    value.insert("input_schema".to_string(), tool.input_schema.clone());
    if let Some(cache_control) = tool.metadata.get("cache_control") {
        value.insert("cache_control".to_string(), cache_control.clone());
    }
    Value::Object(value)
}

fn tool_choice_to_anthropic(tool_choice: &ToolChoice) -> Value {
    match tool_choice {
        ToolChoice::Auto => json!({ "type": "auto" }),
        ToolChoice::None => json!({ "type": "none" }),
        ToolChoice::Required => json!({ "type": "any" }),
        ToolChoice::Tool { name } => json!({ "type": "tool", "name": name }),
    }
}

fn apply_provider_body_overrides(body: &mut Map<String, Value>, request: &AgentRequest) {
    for (key, value) in &request.metadata.provider {
        body.insert(key.clone(), value.clone());
    }
}

fn add_cache_control_to_last_object(values: &mut [Value]) {
    if let Some(object) = values.iter_mut().rev().find_map(Value::as_object_mut) {
        object
            .entry("cache_control".to_string())
            .or_insert_with(ephemeral_cache_control);
    }
}

fn add_cache_control_to_last_message_block(messages: &mut [Value]) {
    for message in messages.iter_mut().rev() {
        let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        for block in content.iter_mut().rev() {
            let Some(object) = block.as_object_mut() else {
                continue;
            };
            if object.get("type").and_then(Value::as_str) == Some("thinking") {
                continue;
            }
            object
                .entry("cache_control".to_string())
                .or_insert_with(ephemeral_cache_control);
            return;
        }
    }
}

fn ephemeral_cache_control() -> Value {
    json!({ "type": "ephemeral" })
}

fn content_block_to_anthropic(block: &ContentBlock) -> Value {
    match block {
        ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
        ContentBlock::Image { source } => {
            json!({ "type": "image", "source": image_source(source) })
        }
        ContentBlock::ToolUse { id, name, input } => {
            json!({ "type": "tool_use", "id": id, "name": name, "input": input })
        }
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            let mut value = json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": content.iter().map(tool_result_content_to_anthropic).collect::<Vec<_>>()
            });
            if *is_error {
                value["is_error"] = Value::Bool(true);
            }
            value
        }
        ContentBlock::Reasoning { text, signature } => {
            let mut value = json!({ "type": "thinking", "thinking": text });
            if let Some(signature) = signature {
                value["signature"] = Value::String(signature.clone());
            }
            value
        }
    }
}

fn tool_result_content_to_anthropic(content: &ToolResultContent) -> Value {
    match content {
        ToolResultContent::Text { text } => json!({ "type": "text", "text": text }),
        ToolResultContent::Json { value } => {
            json!({ "type": "text", "text": serde_json::to_string(value).unwrap_or_default() })
        }
        ToolResultContent::Image { source } => {
            json!({ "type": "image", "source": image_source(source) })
        }
    }
}

fn image_source(source: &ImageSource) -> Value {
    match source {
        ImageSource::Base64 { media_type, data } => {
            json!({ "type": "base64", "media_type": media_type, "data": data })
        }
        ImageSource::Url { url } => json!({ "type": "url", "url": url }),
    }
}

fn content_block_from_anthropic(value: &Value) -> Result<ContentBlock, AnthropicStreamError> {
    let block_type = required_str(value, "type")?;
    match block_type {
        "text" => Ok(ContentBlock::Text {
            text: required_str(value, "text")?.to_string(),
        }),
        "tool_use" => Ok(ContentBlock::ToolUse {
            id: required_str(value, "id")?.to_string(),
            name: required_str(value, "name")?.to_string(),
            input: value.get("input").cloned().unwrap_or_else(|| json!({})),
        }),
        "thinking" => Ok(ContentBlock::Reasoning {
            text: required_str(value, "thinking")?.to_string(),
            signature: value
                .get("signature")
                .and_then(Value::as_str)
                .map(str::to_string),
        }),
        other => Err(AnthropicStreamError::UnknownContentBlock(other.to_string())),
    }
}

fn content_delta_from_anthropic(value: &Value) -> Result<ContentDelta, AnthropicStreamError> {
    let delta_type = required_str(value, "type")?;
    match delta_type {
        "text_delta" => Ok(ContentDelta::Text {
            text: required_str(value, "text")?.to_string(),
        }),
        "input_json_delta" => Ok(ContentDelta::ToolInputJson {
            partial_json: required_str(value, "partial_json")?.to_string(),
        }),
        "thinking_delta" => Ok(ContentDelta::Reasoning {
            text: required_str(value, "thinking")?.to_string(),
        }),
        other => Err(AnthropicStreamError::UnknownContentDelta(other.to_string())),
    }
}

fn stop_reason_from_anthropic(reason: &str) -> StopReason {
    match reason {
        "end_turn" => StopReason::EndTurn,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        "stop_sequence" => StopReason::StopSequence,
        "refusal" => StopReason::ContentFilter,
        other => StopReason::Other {
            reason: other.to_string(),
        },
    }
}

fn usage_from_anthropic(value: Option<&Value>) -> Option<TokenUsage> {
    let value = value?;
    Some(TokenUsage {
        input_tokens: value.get("input_tokens").and_then(Value::as_u64),
        output_tokens: value.get("output_tokens").and_then(Value::as_u64),
        cache_creation_input_tokens: value
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64),
        cache_read_input_tokens: value.get("cache_read_input_tokens").and_then(Value::as_u64),
    })
}

fn required_value<'a>(
    value: &'a Value,
    field: &'static str,
) -> Result<&'a Value, AnthropicStreamError> {
    value
        .get(field)
        .ok_or(AnthropicStreamError::MissingField(field))
}

fn required_str<'a>(
    value: &'a Value,
    field: &'static str,
) -> Result<&'a str, AnthropicStreamError> {
    required_value(value, field)?
        .as_str()
        .ok_or(AnthropicStreamError::InvalidField(field))
}

fn required_usize(value: &Value, field: &'static str) -> Result<usize, AnthropicStreamError> {
    let raw = required_value(value, field)?
        .as_u64()
        .ok_or(AnthropicStreamError::InvalidField(field))?;
    usize::try_from(raw).map_err(|_| AnthropicStreamError::InvalidField(field))
}

#[cfg(test)]
#[path = "anthropic_tests.rs"]
mod tests;
