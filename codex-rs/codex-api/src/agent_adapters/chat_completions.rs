use codex_agent_protocol::AgentMessage;
use codex_agent_protocol::AgentRequest;
use codex_agent_protocol::AgentStreamEvent;
use codex_agent_protocol::AgentTool;
use codex_agent_protocol::ContentBlock;
use codex_agent_protocol::ContentDelta;
use codex_agent_protocol::ImageSource;
use codex_agent_protocol::MessageRole;
use codex_agent_protocol::PROVIDER_FLAVOR_METADATA_KEY;
use codex_agent_protocol::StopReason;
use codex_agent_protocol::TokenUsage;
use codex_agent_protocol::ToolChoice;
use codex_agent_protocol::ToolResultContent;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;

const TEXT_BLOCK_INDEX: usize = 0;
const REASONING_BLOCK_INDEX: usize = 1;
const TOOL_CALL_BLOCK_INDEX_OFFSET: usize = 2;
const FLAVOR_DEEPSEEK: &str = "deepseek";
const FLAVOR_ENABLE_THINKING: &str = "enable_thinking";
const FLAVOR_GENERIC_OPENAI: &str = "generic_openai";
const FLAVOR_MINIMAX: &str = "minimax";
const FLAVOR_OPENROUTER: &str = "openrouter";
const FLAVOR_THINKING_TYPE: &str = "thinking_type";

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
    if let Some(response_format) = &request.metadata.response_format {
        body.insert("response_format".to_string(), response_format.clone());
    }

    let tool_use_names = tool_use_names_by_id(&request.messages);
    let mut messages = request
        .instructions
        .iter()
        .map(instruction_to_chat_message)
        .chain(
            request
                .messages
                .iter()
                .flat_map(|message| message_to_chat_messages(message, &tool_use_names)),
        )
        .collect::<Vec<_>>();
    normalize_system_messages(&mut messages);
    merge_adjacent_assistant_messages(&mut messages);
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
    apply_provider_flavor_defaults(&mut body, request);
    apply_provider_body_overrides(&mut body, request);
    remove_tool_control_fields_without_tools(&mut body);

    Value::Object(body)
}

pub fn parse_stream_chunk(
    value: Value,
) -> Result<Vec<AgentStreamEvent>, ChatCompletionsStreamError> {
    let mut events = Vec::new();

    if let Some(error) = value.get("error") {
        if matches!(
            error.get("code").and_then(Value::as_str),
            Some("context_length_exceeded")
        ) {
            return Err(ChatCompletionsStreamError::ContextWindowExceeded);
        }
        if matches!(
            error.get("code").and_then(Value::as_str),
            Some("insufficient_quota")
        ) {
            return Err(ChatCompletionsStreamError::QuotaExceeded);
        }
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .unwrap_or("chat completions stream error")
            .to_string();
        events.push(AgentStreamEvent::MessageStop {
            stop_reason: Some(StopReason::Error { message }),
            usage: None,
        });
        return Ok(events);
    }

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
        let finish_reason = choice.get("finish_reason").and_then(Value::as_str);
        let usage = usage_from_chat(value.get("usage"));
        let delta = match choice.get("delta") {
            Some(delta) => delta,
            None if finish_reason.is_some() || usage.is_some() => &Value::Null,
            None => return Err(ChatCompletionsStreamError::MissingField("delta")),
        };

        if delta.get("role").and_then(Value::as_str) == Some("assistant") {
            events.push(AgentStreamEvent::MessageStart {
                id: value.get("id").and_then(Value::as_str).map(str::to_string),
                model: value
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
        }

        if let Some(reasoning_content) = delta.get("reasoning_content").and_then(Value::as_str)
            && !reasoning_content.is_empty()
        {
            events.push(AgentStreamEvent::ContentBlockDelta {
                index: REASONING_BLOCK_INDEX,
                delta: ContentDelta::Reasoning {
                    text: reasoning_content.to_string(),
                },
            });
        }
        for reasoning_text in reasoning_details_texts(delta) {
            events.push(AgentStreamEvent::ContentBlockDelta {
                index: REASONING_BLOCK_INDEX,
                delta: ContentDelta::Reasoning {
                    text: reasoning_text,
                },
            });
        }

        if let Some(content) = delta.get("content").and_then(Value::as_str)
            && !content.is_empty()
        {
            events.push(AgentStreamEvent::ContentBlockDelta {
                index: TEXT_BLOCK_INDEX,
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
    ContextWindowExceeded,
    QuotaExceeded,
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
            ChatCompletionsStreamError::ContextWindowExceeded => {
                write!(f, "context window exceeded")
            }
            ChatCompletionsStreamError::QuotaExceeded => {
                write!(f, "quota exceeded")
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

fn tool_use_names_by_id(messages: &[AgentMessage]) -> HashMap<String, String> {
    let mut names = HashMap::new();
    for message in messages {
        for block in &message.content {
            if let ContentBlock::ToolUse { id, name, input: _ } = block {
                names.insert(id.clone(), name.clone());
            }
        }
    }
    names
}

fn message_to_chat_messages(
    message: &AgentMessage,
    tool_use_names: &HashMap<String, String>,
) -> Vec<Value> {
    match message.role {
        MessageRole::System | MessageRole::Developer => vec![json!({
            "role": "system",
            "content": content_blocks_text(&message.content),
        })],
        MessageRole::User => user_message_to_chat_messages(message, tool_use_names),
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

fn merge_adjacent_assistant_messages(messages: &mut Vec<Value>) {
    let mut merged = Vec::with_capacity(messages.len());
    for message in std::mem::take(messages) {
        let should_merge = merged.last().is_some_and(|previous| {
            is_assistant_message(previous) && is_assistant_message(&message)
        });
        if should_merge && let Some(previous) = merged.last_mut() {
            merge_assistant_message(previous, message);
            continue;
        }
        merged.push(message);
    }
    *messages = merged;
}

fn is_assistant_message(message: &Value) -> bool {
    message.get("role").and_then(Value::as_str) == Some("assistant")
}

fn merge_assistant_message(previous: &mut Value, next: Value) {
    let Some(previous) = previous.as_object_mut() else {
        return;
    };
    let Value::Object(mut next) = next else {
        return;
    };

    merge_optional_text_field(previous, &mut next, "content");
    merge_optional_text_field(previous, &mut next, "reasoning_content");

    if let Some(Value::Array(mut next_tool_calls)) = next.remove("tool_calls") {
        match previous.get_mut("tool_calls") {
            Some(Value::Array(previous_tool_calls)) => {
                previous_tool_calls.append(&mut next_tool_calls)
            }
            _ => {
                previous.insert("tool_calls".to_string(), Value::Array(next_tool_calls));
            }
        }
    }
}

fn merge_optional_text_field(
    previous: &mut Map<String, Value>,
    next: &mut Map<String, Value>,
    field: &str,
) {
    let Some(next_value) = next.remove(field) else {
        return;
    };
    let Some(next_text) = non_empty_string(&next_value) else {
        if !previous.contains_key(field) {
            previous.insert(field.to_string(), next_value);
        }
        return;
    };

    match previous.get_mut(field) {
        Some(Value::String(previous_text)) if !previous_text.is_empty() => {
            previous_text.push('\n');
            previous_text.push_str(&next_text);
        }
        Some(Value::String(previous_text)) => previous_text.push_str(&next_text),
        _ => {
            previous.insert(field.to_string(), Value::String(next_text));
        }
    }
}

fn non_empty_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn user_message_to_chat_messages(
    message: &AgentMessage,
    tool_use_names: &HashMap<String, String>,
) -> Vec<Value> {
    let mut messages = Vec::new();
    let user_blocks = message
        .content
        .iter()
        .filter(|block| !matches!(block, ContentBlock::ToolResult { .. }))
        .collect::<Vec<_>>();

    if !user_blocks.is_empty() {
        let user_content = if user_blocks.iter().all(|block| {
            matches!(
                block,
                ContentBlock::Text { .. } | ContentBlock::Reasoning { .. }
            )
        }) {
            let text = user_blocks
                .iter()
                .map(|block| content_block_text(block))
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(Value::String(text))
        } else {
            let parts = user_blocks
                .iter()
                .filter_map(|block| content_block_to_user_content(block))
                .collect::<Vec<_>>();
            (!parts.is_empty()).then_some(Value::Array(parts))
        };

        if let Some(user_content) = user_content {
            messages.push(json!({
                "role": "user",
                "content": user_content,
            }));
        }
    }

    messages.extend(
        message
            .content
            .iter()
            .flat_map(|block| tool_result_to_chat_messages(block, tool_use_names)),
    );
    messages
}

fn assistant_message_to_chat(message: &AgentMessage) -> Value {
    let mut value = Map::new();
    value.insert("role".to_string(), Value::String("assistant".to_string()));

    let tool_calls = message
        .content
        .iter()
        .filter_map(tool_use_to_chat_tool_call)
        .collect::<Vec<_>>();

    let text = message
        .content
        .iter()
        .filter(|block| matches!(block, ContentBlock::Text { .. }))
        .map(content_block_text)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let has_text = !text.is_empty();
    if has_text {
        value.insert("content".to_string(), Value::String(text));
    }

    let reasoning_content = message
        .content
        .iter()
        .filter(|block| matches!(block, ContentBlock::Reasoning { .. }))
        .map(content_block_text)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if !reasoning_content.is_empty() {
        if !has_text && tool_calls.is_empty() {
            value.insert("content".to_string(), Value::String(String::new()));
        }
        value.insert(
            "reasoning_content".to_string(),
            Value::String(reasoning_content),
        );
    }

    if !value.contains_key("content") {
        value.insert("content".to_string(), Value::Null);
    }
    if !tool_calls.is_empty() {
        value.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }

    Value::Object(value)
}

fn tool_result_to_chat_messages(
    block: &ContentBlock,
    tool_use_names: &HashMap<String, String>,
) -> Vec<Value> {
    let ContentBlock::ToolResult {
        tool_use_id,
        content,
        is_error: _,
    } = block
    else {
        return Vec::new();
    };

    if is_read_image_tool_result(tool_use_id, content, tool_use_names) {
        let mut messages = vec![json!({
            "role": "tool",
            "tool_call_id": tool_use_id,
            "content": read_image_tool_result_text(content),
        })];
        messages.push(json!({
            "role": "user",
            "content": read_image_tool_result_user_content(tool_use_id, content),
        }));
        return messages;
    }

    vec![json!({
        "role": "tool",
        "tool_call_id": tool_use_id,
        "content": tool_result_to_chat_content(content),
    })]
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
        if key == PROVIDER_FLAVOR_METADATA_KEY {
            continue;
        }
        if value.is_null() {
            body.remove(key);
        } else {
            body.insert(key.clone(), value.clone());
        }
    }
}

fn apply_provider_flavor_defaults(body: &mut Map<String, Value>, request: &AgentRequest) {
    let provider_flavor = provider_flavor(request);
    match provider_flavor {
        FLAVOR_DEEPSEEK => apply_deepseek_reasoning(body, request),
        FLAVOR_ENABLE_THINKING => apply_enable_thinking_reasoning(body, request),
        FLAVOR_THINKING_TYPE => apply_thinking_type_reasoning(body, request),
        FLAVOR_MINIMAX => apply_minimax_reasoning(body, request),
        FLAVOR_OPENROUTER => apply_openrouter_reasoning(body, request),
        FLAVOR_GENERIC_OPENAI => {}
        _ => {}
    }
}

fn provider_flavor(request: &AgentRequest) -> &str {
    request
        .metadata
        .provider
        .get(PROVIDER_FLAVOR_METADATA_KEY)
        .and_then(Value::as_str)
        .unwrap_or(FLAVOR_GENERIC_OPENAI)
}

fn reasoning_effort(request: &AgentRequest) -> Option<&str> {
    request
        .reasoning
        .as_ref()
        .and_then(|reasoning| reasoning.effort.as_deref())
}

fn reasoning_is_off(effort: &str) -> bool {
    matches!(effort, "none" | "off" | "disabled")
}

fn apply_deepseek_reasoning(body: &mut Map<String, Value>, request: &AgentRequest) {
    let Some(effort) = reasoning_effort(request) else {
        return;
    };
    if reasoning_is_off(effort) {
        body.insert("thinking".to_string(), json!({ "type": "disabled" }));
    } else {
        body.insert("thinking".to_string(), json!({ "type": "enabled" }));
        body.insert(
            "reasoning_effort".to_string(),
            Value::String(deepseek_reasoning_effort(effort).to_string()),
        );
    }
}

fn deepseek_reasoning_effort(effort: &str) -> &str {
    match effort {
        "xhigh" | "max" => "max",
        "minimal" | "low" | "medium" | "high" => "high",
        custom => custom,
    }
}

fn apply_enable_thinking_reasoning(body: &mut Map<String, Value>, request: &AgentRequest) {
    let Some(effort) = reasoning_effort(request) else {
        return;
    };
    body.insert(
        "enable_thinking".to_string(),
        Value::Bool(!reasoning_is_off(effort)),
    );
}

fn apply_thinking_type_reasoning(body: &mut Map<String, Value>, request: &AgentRequest) {
    let Some(effort) = reasoning_effort(request) else {
        return;
    };
    body.insert(
        "thinking".to_string(),
        json!({ "type": if reasoning_is_off(effort) { "disabled" } else { "enabled" } }),
    );
}

fn apply_minimax_reasoning(body: &mut Map<String, Value>, request: &AgentRequest) {
    let Some(effort) = reasoning_effort(request) else {
        return;
    };
    if reasoning_is_off(effort) {
        body.insert("thinking".to_string(), json!({ "type": "disabled" }));
    } else {
        body.insert("thinking".to_string(), json!({ "type": "enabled" }));
        body.insert("reasoning_split".to_string(), Value::Bool(true));
    }
}

fn apply_openrouter_reasoning(body: &mut Map<String, Value>, request: &AgentRequest) {
    let Some(effort) = reasoning_effort(request) else {
        return;
    };
    if reasoning_is_off(effort) {
        body.insert("reasoning".to_string(), json!({ "enabled": false }));
    } else {
        body.insert("reasoning".to_string(), json!({ "effort": effort }));
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

fn content_block_to_user_content(block: &ContentBlock) -> Option<Value> {
    match block {
        ContentBlock::Text { text } | ContentBlock::Reasoning { text, signature: _ } => {
            (!text.is_empty()).then(|| json!({ "type": "text", "text": text }))
        }
        ContentBlock::Image { source } => Some(json!({
            "type": "image_url",
            "image_url": { "url": image_source_url(source) }
        })),
        ContentBlock::ToolUse { .. } | ContentBlock::ToolResult { .. } => {
            let text = content_block_text(block);
            (!text.is_empty()).then(|| json!({ "type": "text", "text": text }))
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

fn is_read_image_tool_result(
    tool_use_id: &str,
    content: &[ToolResultContent],
    tool_use_names: &HashMap<String, String>,
) -> bool {
    tool_use_names
        .get(tool_use_id)
        .is_some_and(|name| name == "Read")
        && content
            .iter()
            .any(|content| matches!(content, ToolResultContent::Image { .. }))
}

fn read_image_tool_result_text(content: &[ToolResultContent]) -> String {
    let text = content
        .iter()
        .filter_map(|content| match content {
            ToolResultContent::Text { text } => Some(text.clone()),
            ToolResultContent::Json { value } => serde_json::to_string(value).ok(),
            ToolResultContent::Image { .. } => None,
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    if text.is_empty() {
        "Read returned an image. The image is attached in the following user message.".to_string()
    } else {
        format!(
            "{text}\n\nRead returned an image. The image is attached in the following user message."
        )
    }
}

fn read_image_tool_result_user_content(tool_use_id: &str, content: &[ToolResultContent]) -> Value {
    let mut parts = vec![json!({
        "type": "text",
        "text": format!("Image returned by Read tool call {tool_use_id}."),
    })];

    parts.extend(content.iter().filter_map(|content| match content {
        ToolResultContent::Image { source, detail } => {
            let mut image_url = json!({ "url": image_source_url(source) });
            if let Some(detail) = detail {
                image_url["detail"] = Value::String(detail.clone());
            }
            Some(json!({ "type": "image_url", "image_url": image_url }))
        }
        ToolResultContent::Text { .. } | ToolResultContent::Json { .. } => None,
    }));

    Value::Array(parts)
}

fn tool_result_content_text(content: &[ToolResultContent]) -> String {
    content
        .iter()
        .map(|content| match content {
            ToolResultContent::Text { text } => text.clone(),
            ToolResultContent::Json { value } => serde_json::to_string(value).unwrap_or_default(),
            ToolResultContent::Image { source, detail: _ } => image_source_url(source),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn tool_result_to_chat_content(content: &[ToolResultContent]) -> Value {
    let has_image = content
        .iter()
        .any(|content| matches!(content, ToolResultContent::Image { .. }));
    if !has_image {
        return Value::String(tool_result_content_text(content));
    }

    let parts = content
        .iter()
        .map(|content| match content {
            ToolResultContent::Text { text } => json!({ "type": "text", "text": text }),
            ToolResultContent::Json { value } => {
                json!({ "type": "text", "text": serde_json::to_string(value).unwrap_or_default() })
            }
            ToolResultContent::Image { source, detail } => {
                let mut image_url = json!({ "url": image_source_url(source) });
                if let Some(detail) = detail {
                    image_url["detail"] = Value::String(detail.clone());
                }
                json!({ "type": "image_url", "image_url": image_url })
            }
        })
        .collect::<Vec<_>>();
    Value::Array(parts)
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

fn reasoning_details_texts(delta: &Value) -> Vec<String> {
    delta
        .get("reasoning_details")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(reasoning_detail_text)
        .collect()
}

fn reasoning_detail_text(detail: &Value) -> Option<String> {
    if let Some(text) = detail.get("text").and_then(Value::as_str)
        && !text.is_empty()
    {
        return Some(text.to_string());
    }
    if let Some(text) = detail.get("content").and_then(Value::as_str)
        && !text.is_empty()
    {
        return Some(text.to_string());
    }
    detail
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_string)
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
    if value.is_null() {
        return None;
    }
    let prompt_cache_hit_tokens = value.get("prompt_cache_hit_tokens").and_then(Value::as_u64);
    let prompt_cache_miss_tokens = value
        .get("prompt_cache_miss_tokens")
        .and_then(Value::as_u64);
    Some(TokenUsage {
        input_tokens: value
            .get("prompt_tokens")
            .and_then(Value::as_u64)
            .or_else(|| {
                prompt_cache_hit_tokens
                    .zip(prompt_cache_miss_tokens)
                    .map(|(hit, miss)| hit.saturating_add(miss))
            }),
        output_tokens: value.get("completion_tokens").and_then(Value::as_u64),
        cache_creation_input_tokens: None,
        cache_read_input_tokens: value
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64)
            .or(prompt_cache_hit_tokens),
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
