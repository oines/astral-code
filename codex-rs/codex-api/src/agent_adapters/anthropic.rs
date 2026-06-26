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
use std::collections::BTreeMap;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnthropicMessagesOptions {
    pub max_tokens: u64,
    pub supports_image_input: bool,
}

const IMAGE_CONTENT_OMITTED_PLACEHOLDER: &str =
    "<image content omitted because you do not support image input>";

#[derive(Debug, Clone, PartialEq)]
pub struct AnthropicMessagesRequest {
    pub body: Value,
    pub tool_name_aliases: BTreeMap<String, String>,
}

pub fn to_messages_request(request: &AgentRequest, options: AnthropicMessagesOptions) -> Value {
    to_messages_request_parts(request, options).body
}

pub fn to_messages_request_parts(
    request: &AgentRequest,
    options: AnthropicMessagesOptions,
) -> AnthropicMessagesRequest {
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(request.model.clone()));
    body.insert("max_tokens".to_string(), Value::from(options.max_tokens));
    body.insert("stream".to_string(), Value::Bool(request.stream));

    let tool_name_aliases = build_tool_name_aliases(&request.tools);
    let cache_control_enabled = request.metadata.prompt_cache_key.is_some();
    if cache_control_enabled {
        body.insert("cache_control".to_string(), ephemeral_cache_control());
    }

    let system = system_blocks(
        request,
        cache_control_enabled,
        &tool_name_aliases,
        options.supports_image_input,
    );
    if !system.is_empty() {
        body.insert("system".to_string(), Value::Array(system));
    }

    let mut messages = request
        .messages
        .iter()
        .filter_map(|message| {
            message_to_anthropic(
                message,
                cache_control_enabled,
                &tool_name_aliases,
                options.supports_image_input,
            )
        })
        .collect::<Vec<_>>();
    merge_adjacent_messages(&mut messages);
    body.insert("messages".to_string(), Value::Array(messages));

    if !request.tools.is_empty() {
        let mut tools = request
            .tools
            .iter()
            .map(|tool| tool_to_anthropic(tool, &tool_name_aliases))
            .collect::<Vec<_>>();
        if cache_control_enabled {
            add_cache_control_to_last_object(&mut tools);
        }
        body.insert("tools".to_string(), Value::Array(tools));
        body.insert(
            "tool_choice".to_string(),
            tool_choice_to_anthropic(&request.tool_choice, &tool_name_aliases),
        );
    }
    apply_provider_body_overrides(&mut body, request);

    AnthropicMessagesRequest {
        body: Value::Object(body),
        tool_name_aliases,
    }
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
            delta: match content_delta_from_anthropic(required_value(&value, "delta")?)? {
                Some(delta) => delta,
                None => return Ok(None),
            },
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

fn system_blocks(
    request: &AgentRequest,
    cache_control_enabled: bool,
    tool_name_aliases: &BTreeMap<String, String>,
    supports_image_input: bool,
) -> Vec<Value> {
    let message_blocks = request
        .messages
        .iter()
        .filter(|message| is_system_role(&message.role))
        .flat_map(|message| message.content.iter());

    let mut blocks = request
        .instructions
        .iter()
        .chain(message_blocks)
        .map(|block| {
            content_block_to_anthropic(
                block,
                cache_control_enabled,
                tool_name_aliases,
                supports_image_input,
            )
        })
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

fn message_to_anthropic(
    message: &AgentMessage,
    cache_control_enabled: bool,
    tool_name_aliases: &BTreeMap<String, String>,
    supports_image_input: bool,
) -> Option<Value> {
    let role = match message.role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System | MessageRole::Developer => return None,
    };
    let content = if matches!(message.role, MessageRole::User) {
        message
            .content
            .iter()
            .filter(|block| matches!(block, ContentBlock::ToolResult { .. }))
            .chain(
                message
                    .content
                    .iter()
                    .filter(|block| !matches!(block, ContentBlock::ToolResult { .. })),
            )
            .map(|block| {
                content_block_to_anthropic(
                    block,
                    cache_control_enabled,
                    tool_name_aliases,
                    supports_image_input,
                )
            })
            .collect::<Vec<_>>()
    } else {
        message
            .content
            .iter()
            .map(|block| {
                content_block_to_anthropic(
                    block,
                    cache_control_enabled,
                    tool_name_aliases,
                    supports_image_input,
                )
            })
            .collect::<Vec<_>>()
    };

    Some(json!({
        "role": role,
        "content": content
    }))
}

fn merge_adjacent_messages(messages: &mut Vec<Value>) {
    let mut merged = Vec::with_capacity(messages.len());
    for message in std::mem::take(messages) {
        let should_merge = merged
            .last()
            .is_some_and(|previous| message_role(previous) == message_role(&message));
        if should_merge && let Some(previous) = merged.last_mut() {
            merge_message_content(previous, message);
            continue;
        }
        merged.push(message);
    }
    *messages = merged;
}

fn message_role(message: &Value) -> Option<&str> {
    message.get("role").and_then(Value::as_str)
}

fn merge_message_content(previous: &mut Value, next: Value) {
    let Some(previous_content) = previous.get_mut("content").and_then(Value::as_array_mut) else {
        return;
    };
    let Value::Object(mut next) = next else {
        return;
    };
    if let Some(Value::Array(mut next_content)) = next.remove("content") {
        previous_content.append(&mut next_content);
    }
}

fn tool_to_anthropic(tool: &AgentTool, tool_name_aliases: &BTreeMap<String, String>) -> Value {
    let mut value = Map::new();
    value.insert(
        "name".to_string(),
        Value::String(tool_name_for_wire(&tool.name, tool_name_aliases)),
    );
    value.insert(
        "description".to_string(),
        Value::String(tool.description.clone()),
    );
    value.insert("input_schema".to_string(), tool.input_schema.clone());
    if let Some(cache_control) = tool.metadata.get("cache_control") {
        value.insert("cache_control".to_string(), cache_control.clone());
    }
    if let Some(strict) = tool.metadata.get("strict") {
        value.insert("strict".to_string(), strict.clone());
    }
    if let Some(defer_loading) = tool.metadata.get("deferLoading") {
        value.insert("defer_loading".to_string(), defer_loading.clone());
    }
    Value::Object(value)
}

fn tool_choice_to_anthropic(
    tool_choice: &ToolChoice,
    tool_name_aliases: &BTreeMap<String, String>,
) -> Value {
    match tool_choice {
        ToolChoice::Auto => json!({ "type": "auto" }),
        ToolChoice::None => json!({ "type": "none" }),
        ToolChoice::Required => json!({ "type": "any" }),
        ToolChoice::Tool { name } => {
            json!({ "type": "tool", "name": tool_name_for_wire(name, tool_name_aliases) })
        }
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

fn add_cache_control_to_last_object(values: &mut [Value]) {
    if let Some(object) = values.iter_mut().rev().find_map(Value::as_object_mut) {
        object
            .entry("cache_control".to_string())
            .or_insert_with(ephemeral_cache_control);
    }
}

fn ephemeral_cache_control() -> Value {
    json!({ "type": "ephemeral" })
}

fn content_block_to_anthropic(
    block: &ContentBlock,
    cache_control_enabled: bool,
    tool_name_aliases: &BTreeMap<String, String>,
    supports_image_input: bool,
) -> Value {
    match block {
        ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
        ContentBlock::Compaction { text } => {
            let mut value = json!({ "type": "text", "text": text });
            if cache_control_enabled {
                value["cache_control"] = ephemeral_cache_control();
            }
            value
        }
        ContentBlock::Image { source, detail: _ } => {
            image_content_block(source, supports_image_input)
        }
        ContentBlock::ToolUse { id, name, input } => {
            json!({
                "type": "tool_use",
                "id": id,
                "name": tool_name_for_wire(name, tool_name_aliases),
                "input": input
            })
        }
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            let mut value = json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": content
                    .iter()
                    .map(|content| tool_result_content_to_anthropic(content, supports_image_input))
                    .collect::<Vec<_>>()
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

fn tool_result_content_to_anthropic(
    content: &ToolResultContent,
    supports_image_input: bool,
) -> Value {
    match content {
        ToolResultContent::Text { text } => json!({ "type": "text", "text": text }),
        ToolResultContent::Json { value } => {
            json!({ "type": "text", "text": serde_json::to_string(value).unwrap_or_default() })
        }
        ToolResultContent::Image { source, detail: _ } => {
            image_content_block(source, supports_image_input)
        }
    }
}

fn image_content_block(source: &ImageSource, supports_image_input: bool) -> Value {
    if supports_image_input {
        json!({ "type": "image", "source": image_source(source) })
    } else {
        json!({ "type": "text", "text": IMAGE_CONTENT_OMITTED_PLACEHOLDER })
    }
}

fn image_source(source: &ImageSource) -> Value {
    match source {
        ImageSource::Base64 { media_type, data } => {
            json!({ "type": "base64", "media_type": media_type, "data": data })
        }
        ImageSource::Url { url } => json!({ "type": "url", "url": url }),
        ImageSource::FileId { file_id } => json!({ "type": "file", "file_id": file_id }),
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

fn build_tool_name_aliases(tools: &[AgentTool]) -> BTreeMap<String, String> {
    let mut aliases = BTreeMap::new();
    let mut used_names = BTreeSet::new();

    for tool in tools {
        let wire_name = unique_anthropic_tool_name(&tool.name, &mut used_names);
        if wire_name != tool.name {
            aliases.insert(wire_name, tool.name.clone());
        }
    }

    aliases
}

fn unique_anthropic_tool_name(name: &str, used_names: &mut BTreeSet<String>) -> String {
    if is_valid_anthropic_tool_name(name) && used_names.insert(name.to_string()) {
        return name.to_string();
    }

    let hash = stable_hex_hash(name);
    let mut prefix = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    if prefix.is_empty() {
        prefix = "tool".to_string();
    }

    let suffix = format!("__{hash}");
    let max_prefix_len = ANTHROPIC_TOOL_NAME_MAX_LEN.saturating_sub(suffix.len());
    prefix.truncate(max_prefix_len);
    let mut candidate = format!("{prefix}{suffix}");
    let mut counter = 1usize;
    while !used_names.insert(candidate.clone()) {
        let counter_suffix = format!("_{counter}");
        let max_prefix_len = ANTHROPIC_TOOL_NAME_MAX_LEN
            .saturating_sub(suffix.len())
            .saturating_sub(counter_suffix.len());
        let mut prefix = prefix.clone();
        prefix.truncate(max_prefix_len);
        candidate = format!("{prefix}{suffix}{counter_suffix}");
        counter = counter.saturating_add(1);
    }
    candidate
}

const ANTHROPIC_TOOL_NAME_MAX_LEN: usize = 64;

fn is_valid_anthropic_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= ANTHROPIC_TOOL_NAME_MAX_LEN
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}

fn stable_hex_hash(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn tool_name_for_wire(name: &str, tool_name_aliases: &BTreeMap<String, String>) -> String {
    tool_name_aliases
        .iter()
        .find_map(|(wire_name, canonical_name)| (canonical_name == name).then(|| wire_name.clone()))
        .unwrap_or_else(|| name.to_string())
}

fn content_delta_from_anthropic(
    value: &Value,
) -> Result<Option<ContentDelta>, AnthropicStreamError> {
    let delta_type = required_str(value, "type")?;
    match delta_type {
        "text_delta" => Ok(Some(ContentDelta::Text {
            text: required_str(value, "text")?.to_string(),
        })),
        "input_json_delta" => Ok(Some(ContentDelta::ToolInputJson {
            partial_json: required_str(value, "partial_json")?.to_string(),
        })),
        "thinking_delta" => Ok(Some(ContentDelta::Reasoning {
            text: required_str(value, "thinking")?.to_string(),
        })),
        "signature_delta" => Ok(None),
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
