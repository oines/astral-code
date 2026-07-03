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
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use tracing::warn;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnthropicMessagesOptions {
    pub max_tokens: u64,
    pub supports_image_input: bool,
    pub cache_fold: Option<AnthropicCacheFoldOptions>,
    pub compact_input_placeholders: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnthropicCacheFoldOptions {
    pub cache_reference_tool_use_ids: BTreeSet<String>,
    pub pinned_cache_edits: Vec<AnthropicPinnedCacheEdits>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnthropicPinnedCacheEdits {
    pub user_message_index: usize,
    pub cache_references: Vec<String>,
}

const IMAGE_CONTENT_OMITTED_PLACEHOLDER: &str =
    "<image content omitted because you do not support image input>";
const COMPACT_IMAGE_PLACEHOLDER: &str = "[image]";
const COMPACT_LARGE_TOOL_RESULT_PLACEHOLDER: &str = "[Old tool result content cleared]";
const COMPACT_TOOL_RESULT_TEXT_PLACEHOLDER_MIN_BYTES: usize = 4096;
const FUNCTION_RESULT_CLEARING_PROMPT: &str = "Old tool results may be automatically cleared from context to free up space. The 5 most recent eligible tool results are always kept. When a tool result contains information you may need later, write down the important details in your response.";
const MIN_THINKING_BUDGET_TOKENS: u64 = 1_024;

#[derive(Clone, Copy)]
struct ContentProjectionOptions<'a> {
    tool_name_aliases: &'a BTreeMap<String, String>,
    supports_image_input: bool,
    compact_input_placeholders: bool,
    thinking_enabled: bool,
}

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
    let cache_fold = options
        .cache_fold
        .as_ref()
        .filter(|_| cache_control_enabled);
    let thinking_enabled = apply_reasoning_config(&mut body, request, options.max_tokens);
    let projection_options = ContentProjectionOptions {
        tool_name_aliases: &tool_name_aliases,
        supports_image_input: options.supports_image_input,
        compact_input_placeholders: options.compact_input_placeholders,
        thinking_enabled,
    };

    let system = system_blocks(
        request,
        cache_control_enabled,
        cache_fold.is_some(),
        projection_options,
    );
    if !system.is_empty() {
        body.insert("system".to_string(), Value::Array(system));
    }

    let mut messages = request
        .messages
        .iter()
        .filter_map(|message| message_to_anthropic(message, projection_options))
        .collect::<Vec<_>>();
    merge_adjacent_messages(&mut messages);
    if cache_control_enabled {
        add_cache_control_to_last_message_content(&mut messages);
    }
    if let Some(cache_fold) = cache_fold {
        apply_cache_fold(&mut messages, cache_fold);
    }
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
    super::apply_provider_body_overrides(&mut body, request);

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
            usage: usage_from_anthropic(value.pointer("/message/usage")),
        })),
        "content_block_start" => {
            let index = required_usize(&value, "index")?;
            let block = match content_block_from_anthropic(required_value(&value, "content_block")?)
            {
                Ok(block) => block,
                Err(AnthropicStreamError::UnknownContentBlock(block_type)) => {
                    warn!("skipping unknown anthropic content block type: {block_type}");
                    return Ok(None);
                }
                Err(error) => return Err(error),
            };
            Ok(Some(AgentStreamEvent::ContentBlockStart { index, block }))
        }
        "content_block_delta" => {
            let index = required_usize(&value, "index")?;
            let delta = match content_delta_from_anthropic(required_value(&value, "delta")?) {
                Ok(Some(delta)) => delta,
                Ok(None) => return Ok(None),
                Err(AnthropicStreamError::UnknownContentDelta(delta_type)) => {
                    warn!("skipping unknown anthropic content delta type: {delta_type}");
                    return Ok(None);
                }
                Err(error) => return Err(error),
            };
            Ok(Some(AgentStreamEvent::ContentBlockDelta { index, delta }))
        }
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
        other => {
            warn!("skipping unknown anthropic stream event type: {other}");
            Ok(None)
        }
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
    cache_fold_enabled: bool,
    projection_options: ContentProjectionOptions<'_>,
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
        .filter_map(|block| content_block_to_anthropic(block, projection_options))
        .collect::<Vec<_>>();
    if cache_fold_enabled {
        blocks.push(json!({
            "type": "text",
            "text": FUNCTION_RESULT_CLEARING_PROMPT
        }));
    }
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
    projection_options: ContentProjectionOptions<'_>,
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
            .filter_map(|block| content_block_to_anthropic(block, projection_options))
            .collect::<Vec<_>>()
    } else {
        message
            .content
            .iter()
            .filter_map(|block| content_block_to_anthropic(block, projection_options))
            .collect::<Vec<_>>()
    };
    if content.is_empty() {
        return None;
    }

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

fn apply_reasoning_config(
    body: &mut Map<String, Value>,
    request: &AgentRequest,
    max_tokens: u64,
) -> bool {
    let Some(effort) = request
        .reasoning
        .as_ref()
        .and_then(|reasoning| reasoning.effort.as_deref())
    else {
        return false;
    };
    let Some(budget_tokens) = anthropic_thinking_budget(effort, max_tokens) else {
        return false;
    };
    body.insert(
        "thinking".to_string(),
        json!({ "type": "enabled", "budget_tokens": budget_tokens }),
    );
    true
}

fn anthropic_thinking_budget(effort: &str, max_tokens: u64) -> Option<u64> {
    if matches!(effort, "none" | "off" | "disabled") {
        return None;
    }
    let max_budget = max_tokens.saturating_sub(1);
    if max_budget < MIN_THINKING_BUDGET_TOKENS {
        return None;
    }
    let requested = match effort {
        "minimal" | "low" => MIN_THINKING_BUDGET_TOKENS,
        "medium" => 4_096,
        "high" => 8_192,
        "xhigh" | "max" => 16_384,
        custom => custom.parse::<u64>().unwrap_or(4_096),
    };
    Some(requested.max(MIN_THINKING_BUDGET_TOKENS).min(max_budget))
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

fn add_cache_control_to_last_message_content(messages: &mut [Value]) {
    if message_cache_marker_position(messages).is_some() {
        return;
    }

    for message in messages.iter_mut().rev() {
        let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        if let Some(object) = content.iter_mut().rev().find_map(Value::as_object_mut) {
            object.insert("cache_control".to_string(), ephemeral_cache_control());
            return;
        }
    }
}

fn apply_cache_fold(messages: &mut [Value], cache_fold: &AnthropicCacheFoldOptions) {
    insert_cache_edits(messages, &cache_fold.pinned_cache_edits);
    add_cache_references_before_marker_message(messages, &cache_fold.cache_reference_tool_use_ids);
}

fn add_cache_references_before_marker_message(
    messages: &mut [Value],
    tool_use_ids: &BTreeSet<String>,
) {
    let Some(marker) = message_cache_marker_position(messages) else {
        return;
    };

    for (message_index, message) in messages.iter_mut().enumerate() {
        if message_index >= marker.0 {
            continue;
        }
        let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        for block in content.iter_mut() {
            let Some(object) = block.as_object_mut() else {
                continue;
            };
            if object.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            let Some(tool_use_id) = object.get("tool_use_id").and_then(Value::as_str) else {
                continue;
            };
            if tool_use_ids.contains(tool_use_id) {
                object.insert(
                    "cache_reference".to_string(),
                    Value::String(tool_use_id.to_string()),
                );
            }
        }
    }
}

fn insert_cache_edits(messages: &mut [Value], pinned_cache_edits: &[AnthropicPinnedCacheEdits]) {
    let mut seen_refs = BTreeSet::new();
    for pinned in pinned_cache_edits {
        let cache_references = pinned
            .cache_references
            .iter()
            .filter(|cache_reference| seen_refs.insert((*cache_reference).clone()))
            .collect::<Vec<_>>();
        if cache_references.is_empty() {
            continue;
        }

        let Some(message) = messages.get_mut(pinned.user_message_index) else {
            continue;
        };
        if message_role(message) != Some("user") {
            continue;
        }
        let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };

        let insert_index = content
            .iter()
            .take_while(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
            .count();
        content.insert(
            insert_index,
            json!({
                "type": "cache_edits",
                "edits": cache_references
                    .iter()
                    .map(|cache_reference| {
                        json!({
                            "type": "delete",
                            "cache_reference": cache_reference,
                        })
                    })
                    .collect::<Vec<_>>()
            }),
        );
    }
}

fn message_cache_marker_position(messages: &[Value]) -> Option<(usize, usize)> {
    for (message_index, message) in messages.iter().enumerate().rev() {
        let Some(content) = message.get("content").and_then(Value::as_array) else {
            continue;
        };
        for (block_index, block) in content.iter().enumerate().rev() {
            if block.get("cache_control").is_some() {
                return Some((message_index, block_index));
            }
        }
    }
    None
}

fn content_block_to_anthropic(
    block: &ContentBlock,
    projection_options: ContentProjectionOptions<'_>,
) -> Option<Value> {
    match block {
        ContentBlock::Text { text } => Some(json!({ "type": "text", "text": text })),
        ContentBlock::Compaction { text } => Some(json!({ "type": "text", "text": text })),
        ContentBlock::Image { source, detail: _ } => {
            Some(image_content_block(source, projection_options))
        }
        ContentBlock::ToolUse { id, name, input } => Some(json!({
            "type": "tool_use",
            "id": id,
            "name": tool_name_for_wire(name, projection_options.tool_name_aliases),
            "input": input
        })),
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
                    .map(|content| tool_result_content_to_anthropic(content, projection_options))
                    .collect::<Vec<_>>()
            });
            if *is_error {
                value["is_error"] = Value::Bool(true);
            }
            Some(value)
        }
        ContentBlock::Reasoning { text, signature } => {
            if !projection_options.thinking_enabled {
                return None;
            }
            let signature = signature.as_ref()?;
            let mut value = json!({ "type": "thinking", "thinking": text });
            value["signature"] = Value::String(signature.clone());
            Some(value)
        }
    }
}

fn tool_result_content_to_anthropic(
    content: &ToolResultContent,
    projection_options: ContentProjectionOptions<'_>,
) -> Value {
    match content {
        ToolResultContent::Text { text } => {
            tool_result_text_content_block(text, projection_options.compact_input_placeholders)
        }
        ToolResultContent::Json { value } => {
            let text = serde_json::to_string(value).unwrap_or_default();
            tool_result_text_content_block(&text, projection_options.compact_input_placeholders)
        }
        ToolResultContent::Image { source, detail: _ } => {
            image_content_block(source, projection_options)
        }
    }
}

fn tool_result_text_content_block(text: &str, compact_input_placeholders: bool) -> Value {
    if compact_input_placeholders && text.len() > COMPACT_TOOL_RESULT_TEXT_PLACEHOLDER_MIN_BYTES {
        json!({ "type": "text", "text": COMPACT_LARGE_TOOL_RESULT_PLACEHOLDER })
    } else {
        json!({ "type": "text", "text": text })
    }
}

fn image_content_block(
    source: &ImageSource,
    projection_options: ContentProjectionOptions<'_>,
) -> Value {
    if projection_options.compact_input_placeholders {
        json!({ "type": "text", "text": COMPACT_IMAGE_PLACEHOLDER })
    } else if projection_options.supports_image_input {
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
        "signature_delta" => Ok(Some(ContentDelta::ReasoningSignature {
            signature: required_str(value, "signature")?.to_string(),
        })),
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
