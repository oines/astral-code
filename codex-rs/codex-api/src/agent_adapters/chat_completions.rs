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

const TOOL_CALL_BLOCK_INDEX_OFFSET: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatCompletionsOptions {
    pub max_tokens: Option<u64>,
}

pub fn to_chat_completions_request(
    request: &AgentRequest,
    options: ChatCompletionsOptions,
) -> Value {
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(request.model.clone()));
    body.insert("stream".to_string(), Value::Bool(request.stream));
    if let Some(max_tokens) = options.max_tokens {
        body.insert("max_tokens".to_string(), Value::from(max_tokens));
    }
    if let Some(service_tier) = &request.metadata.service_tier {
        body.insert(
            "service_tier".to_string(),
            Value::String(service_tier.clone()),
        );
    }
    if request.stream {
        body.insert(
            "stream_options".to_string(),
            json!({ "include_usage": true }),
        );
    }

    let mut messages = request
        .instructions
        .iter()
        .map(instruction_to_chat_message)
        .chain(request.messages.iter().flat_map(message_to_chat_messages))
        .collect::<Vec<_>>();
    normalize_system_messages(&mut messages);
    body.insert("messages".to_string(), Value::Array(messages));

    if !request.tools.is_empty() {
        body.insert(
            "tools".to_string(),
            Value::Array(request.tools.iter().map(tool_to_chat).collect()),
        );
        body.insert(
            "tool_choice".to_string(),
            tool_choice_to_chat(&request.tool_choice),
        );
        body.insert(
            "parallel_tool_calls".to_string(),
            Value::Bool(request.parallel_tool_calls),
        );
    }
    apply_provider_body_overrides(&mut body, request);
    remove_tool_control_fields_without_tools(&mut body);

    Value::Object(body)
}

pub fn parse_stream_chunk(
    value: Value,
) -> Result<Vec<AgentStreamEvent>, ChatCompletionsStreamError> {
    let mut events = Vec::new();

    let choices = required_array(&value, "choices")?;
    if choices.is_empty() {
        if let Some(usage) = usage_from_chat(value.get("usage")) {
            events.push(AgentStreamEvent::MessageStop {
                stop_reason: None,
                usage: Some(usage),
            });
        }
        return Ok(events);
    }

    for choice in choices {
        let delta = choice
            .get("delta")
            .ok_or(ChatCompletionsStreamError::MissingField("delta"))?;

        if delta.get("role").and_then(Value::as_str) == Some("assistant") {
            events.push(AgentStreamEvent::MessageStart {
                id: value.get("id").and_then(Value::as_str).map(str::to_string),
                model: value
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
        }

        if let Some(content) = delta.get("content").and_then(Value::as_str)
            && !content.is_empty()
        {
            events.push(AgentStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text {
                    text: content.to_string(),
                },
            });
        }

        if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for tool_call in tool_calls {
                append_tool_call_delta(&mut events, tool_call)?;
            }
        }

        let finish_reason = choice.get("finish_reason").and_then(Value::as_str);
        let usage = usage_from_chat(value.get("usage"));
        if finish_reason.is_some() || usage.is_some() {
            events.push(AgentStreamEvent::MessageStop {
                stop_reason: finish_reason.map(stop_reason_from_chat),
                usage,
            });
        }
    }

    Ok(events)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatCompletionsStreamError {
    MissingField(&'static str),
    InvalidField(&'static str),
}

impl std::fmt::Display for ChatCompletionsStreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChatCompletionsStreamError::MissingField(field) => {
                write!(f, "chat completions stream chunk missing field {field}")
            }
            ChatCompletionsStreamError::InvalidField(field) => {
                write!(f, "chat completions stream chunk has invalid field {field}")
            }
        }
    }
}

impl std::error::Error for ChatCompletionsStreamError {}

fn instruction_to_chat_message(block: &ContentBlock) -> Value {
    json!({
        "role": "system",
        "content": content_block_text(block),
    })
}

fn message_to_chat_messages(message: &AgentMessage) -> Vec<Value> {
    match message.role {
        MessageRole::System | MessageRole::Developer => vec![json!({
            "role": "system",
            "content": content_blocks_text(&message.content),
        })],
        MessageRole::User => user_message_to_chat_messages(message),
        MessageRole::Assistant => vec![assistant_message_to_chat(message)],
    }
}

fn normalize_system_messages(messages: &mut Vec<Value>) {
    let system_count = messages
        .iter()
        .filter(|message| is_system_message(message))
        .count();
    if system_count == 0 {
        return;
    }

    if system_count == 1 {
        if let Some(index) = messages.iter().position(is_system_message)
            && index > 0
        {
            let message = messages.remove(index);
            messages.insert(0, message);
        }
        return;
    }

    let mut system_chunks = Vec::new();
    let mut rest = Vec::with_capacity(messages.len());
    for message in std::mem::take(messages) {
        if is_system_message(&message) {
            if let Some(text) = chat_message_text(&message) {
                system_chunks.push(text);
            }
        } else {
            rest.push(message);
        }
    }

    if !system_chunks.is_empty() {
        messages.push(json!({
            "role": "system",
            "content": system_chunks.join("\n\n"),
        }));
    }
    messages.extend(rest);
}

fn is_system_message(message: &Value) -> bool {
    message.get("role").and_then(Value::as_str) == Some("system")
}

fn chat_message_text(message: &Value) -> Option<String> {
    match message.get("content") {
        Some(Value::String(text)) if !text.trim().is_empty() => Some(text.clone()),
        Some(Value::Array(parts)) => {
            let text = parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .filter(|text| !text.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    }
}

fn user_message_to_chat_messages(message: &AgentMessage) -> Vec<Value> {
    let mut messages = Vec::new();
    let user_content = message
        .content
        .iter()
        .filter(|block| !matches!(block, ContentBlock::ToolResult { .. }))
        .map(content_block_to_user_content)
        .collect::<Vec<_>>();
    if !user_content.is_empty() {
        messages.push(json!({
            "role": "user",
            "content": user_content,
        }));
    }

    messages.extend(
        message
            .content
            .iter()
            .filter_map(tool_result_to_chat_message),
    );
    messages
}

fn assistant_message_to_chat(message: &AgentMessage) -> Value {
    let mut value = Map::new();
    value.insert("role".to_string(), Value::String("assistant".to_string()));

    let text = message
        .content
        .iter()
        .filter(|block| {
            matches!(
                block,
                ContentBlock::Text { .. } | ContentBlock::Reasoning { .. }
            )
        })
        .map(content_block_text)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if !text.is_empty() {
        value.insert("content".to_string(), Value::String(text));
    } else {
        value.insert("content".to_string(), Value::Null);
    }

    let tool_calls = message
        .content
        .iter()
        .filter_map(tool_use_to_chat_tool_call)
        .collect::<Vec<_>>();
    if !tool_calls.is_empty() {
        value.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }

    Value::Object(value)
}

fn tool_result_to_chat_message(block: &ContentBlock) -> Option<Value> {
    let ContentBlock::ToolResult {
        tool_use_id,
        content,
        is_error: _,
    } = block
    else {
        return None;
    };

    Some(json!({
        "role": "tool",
        "tool_call_id": tool_use_id,
        "content": tool_result_content_text(content),
    }))
}

fn tool_use_to_chat_tool_call(block: &ContentBlock) -> Option<Value> {
    let ContentBlock::ToolUse { id, name, input } = block else {
        return None;
    };

    Some(json!({
        "id": id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": serde_json::to_string(input).unwrap_or_default(),
        }
    }))
}

fn tool_to_chat(tool: &AgentTool) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema,
        }
    })
}

fn tool_choice_to_chat(tool_choice: &ToolChoice) -> Value {
    match tool_choice {
        ToolChoice::Auto => Value::String("auto".to_string()),
        ToolChoice::None => Value::String("none".to_string()),
        ToolChoice::Required => Value::String("required".to_string()),
        ToolChoice::Tool { name } => json!({
            "type": "function",
            "function": { "name": name },
        }),
    }
}

fn apply_provider_body_overrides(body: &mut Map<String, Value>, request: &AgentRequest) {
    for (key, value) in &request.metadata.provider {
        body.insert(key.clone(), value.clone());
    }
}

fn remove_tool_control_fields_without_tools(body: &mut Map<String, Value>) {
    let has_tools = body
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty());
    if has_tools {
        return;
    }

    body.remove("tool_choice");
    body.remove("parallel_tool_calls");
}

fn content_block_to_user_content(block: &ContentBlock) -> Value {
    match block {
        ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
        ContentBlock::Image { source } => json!({
            "type": "image_url",
            "image_url": { "url": image_source_url(source) }
        }),
        ContentBlock::Reasoning { text, signature: _ } => json!({ "type": "text", "text": text }),
        ContentBlock::ToolUse { .. } | ContentBlock::ToolResult { .. } => {
            json!({ "type": "text", "text": content_block_text(block) })
        }
    }
}

fn content_blocks_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .map(content_block_text)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn content_block_text(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Text { text } | ContentBlock::Reasoning { text, signature: _ } => {
            text.clone()
        }
        ContentBlock::Image { source } => image_source_url(source),
        ContentBlock::ToolUse { id, name, input } => {
            format!(
                "tool_use {id} {name} {}",
                serde_json::to_string(input).unwrap_or_default()
            )
        }
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error: _,
        } => format!(
            "tool_result {tool_use_id} {}",
            tool_result_content_text(content)
        ),
    }
}

fn tool_result_content_text(content: &[ToolResultContent]) -> String {
    content
        .iter()
        .map(|content| match content {
            ToolResultContent::Text { text } => text.clone(),
            ToolResultContent::Json { value } => serde_json::to_string(value).unwrap_or_default(),
            ToolResultContent::Image { source } => image_source_url(source),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn image_source_url(source: &ImageSource) -> String {
    match source {
        ImageSource::Base64 { media_type, data } => format!("data:{media_type};base64,{data}"),
        ImageSource::Url { url } => url.clone(),
    }
}

fn append_tool_call_delta(
    events: &mut Vec<AgentStreamEvent>,
    tool_call: &Value,
) -> Result<(), ChatCompletionsStreamError> {
    let index = tool_call
        .get("index")
        .and_then(Value::as_u64)
        .map(usize::try_from)
        .transpose()
        .map_err(|_| ChatCompletionsStreamError::InvalidField("index"))?
        .unwrap_or(0)
        .checked_add(TOOL_CALL_BLOCK_INDEX_OFFSET)
        .ok_or(ChatCompletionsStreamError::InvalidField("index"))?;

    let function = tool_call.get("function");
    let id = tool_call.get("id").and_then(Value::as_str);
    let name = function
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str);
    if let (Some(id), Some(name)) = (id, name) {
        events.push(AgentStreamEvent::ContentBlockStart {
            index,
            block: ContentBlock::ToolUse {
                id: id.to_string(),
                name: name.to_string(),
                input: json!({}),
            },
        });
    }

    if let Some(arguments) = function
        .and_then(|function| function.get("arguments"))
        .and_then(Value::as_str)
        && !arguments.is_empty()
    {
        events.push(AgentStreamEvent::ContentBlockDelta {
            index,
            delta: ContentDelta::ToolInputJson {
                partial_json: arguments.to_string(),
            },
        });
    }

    Ok(())
}

fn stop_reason_from_chat(reason: &str) -> StopReason {
    match reason {
        "stop" => StopReason::EndTurn,
        "tool_calls" | "function_call" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        "content_filter" => StopReason::ContentFilter,
        other => StopReason::Other {
            reason: other.to_string(),
        },
    }
}

fn usage_from_chat(value: Option<&Value>) -> Option<TokenUsage> {
    let value = value?;
    Some(TokenUsage {
        input_tokens: value.get("prompt_tokens").and_then(Value::as_u64),
        output_tokens: value.get("completion_tokens").and_then(Value::as_u64),
        cache_creation_input_tokens: None,
        cache_read_input_tokens: value
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64),
    })
}

fn required_array<'a>(
    value: &'a Value,
    field: &'static str,
) -> Result<&'a Vec<Value>, ChatCompletionsStreamError> {
    value
        .get(field)
        .ok_or(ChatCompletionsStreamError::MissingField(field))?
        .as_array()
        .ok_or(ChatCompletionsStreamError::InvalidField(field))
}

#[cfg(test)]
#[path = "chat_completions_tests.rs"]
mod tests;
