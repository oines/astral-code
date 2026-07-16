#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Result;
use base64::Engine;
use codex_protocol::models::ContentItem;
use codex_protocol::models::TranscriptItem;
use codex_protocol::openai_models::ModelsResponse;
use futures::SinkExt;
use futures::StreamExt;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::sync::oneshot;
use tokio_tungstenite::accept_hdr_async_with_config;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::extensions::ExtensionsConfig;
use tokio_tungstenite::tungstenite::extensions::compression::deflate::DeflateConfig;
use tokio_tungstenite::tungstenite::handshake::server::Request;
use tokio_tungstenite::tungstenite::handshake::server::Response;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use wiremock::BodyPrintLimit;
use wiremock::Match;
use wiremock::Mock;
use wiremock::MockBuilder;
use wiremock::MockServer;
use wiremock::Respond;
use wiremock::ResponseTemplate;
use wiremock::http::HeaderName;
use wiremock::http::HeaderValue;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

#[derive(Debug, Clone)]
pub struct ResponseMock {
    requests: Arc<Mutex<Vec<ResponsesRequest>>>,
}

impl ResponseMock {
    pub fn new() -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn single_request(&self) -> ResponsesRequest {
        let requests = self.requests.lock().unwrap();
        if requests.len() != 1 {
            panic!("expected 1 request, got {}", requests.len());
        }
        requests.first().unwrap().clone()
    }

    pub fn requests(&self) -> Vec<ResponsesRequest> {
        self.requests.lock().unwrap().clone()
    }

    pub fn last_request(&self) -> Option<ResponsesRequest> {
        self.requests.lock().unwrap().last().cloned()
    }

    /// Returns true if any captured request contains a `function_call` with the
    /// provided `call_id`.
    pub fn saw_function_call(&self, call_id: &str) -> bool {
        self.requests()
            .iter()
            .any(|req| req.has_function_call(call_id))
    }

    /// Returns the `output` string for a matching `function_call_output` with
    /// the provided `call_id`, searching across all captured requests.
    pub fn function_call_output_text(&self, call_id: &str) -> Option<String> {
        self.requests()
            .iter()
            .find_map(|req| req.function_call_output_text(call_id))
    }
}

#[derive(Debug, Clone)]
pub struct ResponsesRequest(wiremock::Request);

fn is_zstd_encoding(value: &str) -> bool {
    value
        .split(',')
        .any(|entry| entry.trim().eq_ignore_ascii_case("zstd"))
}

fn decode_body_bytes(body: &[u8], content_encoding: Option<&str>) -> Vec<u8> {
    if content_encoding.is_some_and(is_zstd_encoding) {
        zstd::stream::decode_all(std::io::Cursor::new(body)).unwrap_or_else(|err| {
            panic!("failed to decode zstd request body: {err}");
        })
    } else {
        body.to_vec()
    }
}

impl ResponsesRequest {
    pub fn body_json(&self) -> Value {
        let body = decode_body_bytes(
            &self.0.body,
            self.0
                .headers
                .get("content-encoding")
                .and_then(|value| value.to_str().ok()),
        );
        let mut body: Value = serde_json::from_slice(&body).unwrap();
        add_responses_compat_fields(&mut body);
        body
    }

    pub fn body_bytes(&self) -> Vec<u8> {
        self.0.body.clone()
    }

    pub fn body_text(&self) -> String {
        let body = decode_body_bytes(
            &self.0.body,
            self.0
                .headers
                .get("content-encoding")
                .and_then(|value| value.to_str().ok()),
        );
        String::from_utf8(body).unwrap_or_else(|err| {
            panic!("request body should be UTF-8 JSON: {err}");
        })
    }

    pub fn body_contains_text(&self, text: &str) -> bool {
        let json_fragment = serde_json::to_string(text)
            .expect("serialize text to JSON")
            .trim_matches('"')
            .to_string();
        self.body_json().to_string().contains(&json_fragment)
    }

    pub fn tool_by_name(&self, namespace: &str, tool_name: &str) -> Option<Value> {
        namespace_child_tool(&self.body_json(), namespace, tool_name).cloned()
    }

    pub fn instructions_text(&self) -> String {
        let body = self.body_json();
        if let Some(instructions) = body.get("instructions").and_then(Value::as_str) {
            return instructions.to_string();
        }
        chat_message_text_groups(&body, "system")
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Returns all `input_text` spans from `message` inputs for the provided role.
    pub fn message_input_texts(&self, role: &str) -> Vec<String> {
        let body = self.body_json();
        if body.get("messages").is_some() {
            return message_input_text_groups_from_items(&request_input_items(&body), role)
                .into_iter()
                .flatten()
                .collect();
        } else {
            message_input_text_groups_from_items(&self.input(), role)
                .into_iter()
                .flatten()
                .collect()
        }
    }

    /// Returns `input_text` spans grouped by `message` input for the provided role.
    pub fn message_input_text_groups(&self, role: &str) -> Vec<Vec<String>> {
        let body = self.body_json();
        if body.get("messages").is_some() {
            return message_input_text_groups_from_items(&request_input_items(&body), role);
        }

        message_input_text_groups_from_items(&self.input(), role)
    }

    pub fn has_message_with_input_texts(
        &self,
        role: &str,
        predicate: impl Fn(&[String]) -> bool,
    ) -> bool {
        self.message_input_text_groups(role)
            .iter()
            .any(|texts| predicate(texts))
    }

    /// Returns all `input_image` `image_url` spans from `message` inputs for the provided role.
    pub fn message_input_image_urls(&self, role: &str) -> Vec<String> {
        let body = self.body_json();
        if body.get("messages").is_some() {
            return chat_message_image_urls(&body, role);
        }

        self.inputs_of_type("message")
            .into_iter()
            .filter(|item| item.get("role").and_then(Value::as_str) == Some(role))
            .filter_map(|item| item.get("content").and_then(Value::as_array).cloned())
            .flatten()
            .filter(|span| span.get("type").and_then(Value::as_str) == Some("input_image"))
            .filter_map(|span| {
                span.get("image_url")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .collect()
    }

    pub fn input(&self) -> Vec<Value> {
        request_input_items(&self.body_json())
    }

    pub fn inputs_of_type(&self, ty: &str) -> Vec<Value> {
        self.input()
            .iter()
            .filter(|item| item.get("type").and_then(Value::as_str) == Some(ty))
            .cloned()
            .collect()
    }

    pub fn function_call_output(&self, call_id: &str) -> Value {
        self.call_output(call_id, "function_call_output")
    }

    pub fn custom_tool_call_output(&self, call_id: &str) -> Value {
        let input = self.input();
        input
            .iter()
            .find(|item| {
                item.get("type").and_then(Value::as_str) == Some("custom_tool_call_output")
                    && item.get("call_id").and_then(Value::as_str) == Some(call_id)
            })
            .or_else(|| {
                input.iter().find(|item| {
                    item.get("type").and_then(Value::as_str) == Some("function_call_output")
                        && item.get("call_id").and_then(Value::as_str) == Some(call_id)
                })
            })
            .cloned()
            .unwrap_or_else(|| {
                panic!("custom tool call output {call_id} item not found in request")
            })
    }

    pub fn tool_search_output(&self, call_id: &str) -> Value {
        let output_item = self.call_output(call_id, "tool_search_output");
        let parsed = match output_item.get("output") {
            Some(Value::String(output_text)) => match serde_json::from_str::<Value>(output_text) {
                Ok(Value::Object(parsed)) => parsed,
                Ok(_) | Err(_) => return output_item,
            },
            Some(Value::Object(parsed)) => parsed.clone(),
            Some(Value::Array(_))
            | Some(Value::Number(_))
            | Some(Value::Bool(_))
            | Some(Value::Null)
            | None => return output_item,
        };
        let mut parsed = parsed;
        parsed
            .entry("type".to_string())
            .or_insert_with(|| Value::String("tool_search_output".to_string()));
        parsed
            .entry("call_id".to_string())
            .or_insert_with(|| Value::String(call_id.to_string()));
        parsed
            .entry("status".to_string())
            .or_insert_with(|| Value::String("completed".to_string()));
        parsed
            .entry("execution".to_string())
            .or_insert_with(|| Value::String("client".to_string()));
        Value::Object(parsed)
    }

    pub fn call_output(&self, call_id: &str, call_type: &str) -> Value {
        self.input()
            .iter()
            .find(|item| {
                item.get("type").unwrap() == call_type && item.get("call_id").unwrap() == call_id
            })
            .cloned()
            .unwrap_or_else(|| panic!("function call output {call_id} item not found in request"))
    }

    /// Returns true if this request's `input` contains a `function_call` with
    /// the specified `call_id`.
    pub fn has_function_call(&self, call_id: &str) -> bool {
        self.input().iter().any(|item| {
            item.get("type").and_then(Value::as_str) == Some("function_call")
                && item.get("call_id").and_then(Value::as_str) == Some(call_id)
        })
    }

    /// If present, returns the `output` string of the `function_call_output`
    /// entry matching `call_id` in this request's `input`.
    pub fn function_call_output_text(&self, call_id: &str) -> Option<String> {
        let binding = self.input();
        let item = binding.iter().find(|item| {
            item.get("type").and_then(Value::as_str) == Some("function_call_output")
                && item.get("call_id").and_then(Value::as_str) == Some(call_id)
        })?;
        item.get("output")
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    pub fn function_call_output_content_and_success(
        &self,
        call_id: &str,
    ) -> Option<(Option<String>, Option<bool>)> {
        self.call_output_content_and_success(call_id, "function_call_output")
    }

    pub fn custom_tool_call_output_content_and_success(
        &self,
        call_id: &str,
    ) -> Option<(Option<String>, Option<bool>)> {
        self.call_output_content_and_success(call_id, "custom_tool_call_output")
            .or_else(|| self.call_output_content_and_success(call_id, "function_call_output"))
    }

    fn call_output_content_and_success(
        &self,
        call_id: &str,
        call_type: &str,
    ) -> Option<(Option<String>, Option<bool>)> {
        let output = self
            .call_output(call_id, call_type)
            .get("output")
            .cloned()
            .unwrap_or(Value::Null);
        match output {
            Value::String(_) | Value::Array(_) => Some((output_value_to_text(&output), None)),
            Value::Object(obj) => Some((
                obj.get("content").and_then(|content| match content {
                    Value::String(text) => Some(text.clone()),
                    Value::Array(_) => output_value_to_text(content),
                    Value::Object(_) | Value::Number(_) | Value::Bool(_) | Value::Null => None,
                }),
                obj.get("success").and_then(Value::as_bool),
            )),
            _ => Some((None, None)),
        }
    }

    pub fn header(&self, name: &str) -> Option<String> {
        self.0
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    }

    pub fn path(&self) -> String {
        self.0.url.path().to_string()
    }

    pub fn query_param(&self, name: &str) -> Option<String> {
        self.0
            .url
            .query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.to_string())
    }
}

fn add_responses_compat_fields(body: &mut Value) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    let Some(messages) = object.get("messages").and_then(Value::as_array).cloned() else {
        return;
    };

    if !object.contains_key("input") {
        object.insert(
            "input".to_string(),
            Value::Array(chat_messages_to_response_input(&messages)),
        );
    }

    if !object.contains_key("instructions") {
        let instructions = chat_message_text_groups_from_messages(&messages, "system")
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n\n");
        if !instructions.is_empty() {
            object.insert("instructions".to_string(), Value::String(instructions));
        }
    }
}

pub fn request_input_items(body: &Value) -> Vec<Value> {
    if let Some(input) = body.get("input").and_then(Value::as_array) {
        return input.clone();
    }
    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        return chat_messages_to_response_input(messages);
    }
    panic!("input or messages array not found in request")
}

pub fn request_tool_name(tool: &Value) -> Option<&str> {
    tool.pointer("/function/name")
        .or_else(|| tool.get("name"))
        .or_else(|| tool.get("type"))
        .and_then(Value::as_str)
}

pub fn request_tool_description(tool: &Value) -> Option<&str> {
    tool.pointer("/function/description")
        .or_else(|| tool.get("description"))
        .and_then(Value::as_str)
}

pub fn request_tool_names(body: &Value) -> Vec<String> {
    body.get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(request_tool_name)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn chat_message_text_groups(body: &Value, role: &str) -> Vec<Vec<String>> {
    body.get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some(role))
        .map(chat_message_texts)
        .collect()
}

fn chat_message_text_groups_from_messages(messages: &[Value], role: &str) -> Vec<Vec<String>> {
    messages
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some(role))
        .map(chat_message_texts)
        .collect()
}

fn message_input_text_groups_from_items(items: &[Value], role: &str) -> Vec<Vec<String>> {
    items
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
        .filter(|item| item.get("role").and_then(Value::as_str) == Some(role))
        .filter_map(|item| item.get("content").and_then(Value::as_array).cloned())
        .map(|content| {
            content
                .into_iter()
                .filter(|span| span.get("type").and_then(Value::as_str) == Some("input_text"))
                .filter_map(|span| span.get("text").and_then(Value::as_str).map(str::to_owned))
                .collect()
        })
        .collect()
}

fn chat_message_texts(message: &Value) -> Vec<String> {
    match message.get("content") {
        Some(Value::String(text)) => vec![text.clone()],
        Some(Value::Array(parts)) => parts
            .iter()
            .filter(|part| {
                matches!(
                    part.get("type").and_then(Value::as_str),
                    Some("text" | "input_text")
                )
            })
            .filter_map(|part| part.get("text").and_then(Value::as_str).map(str::to_string))
            .collect(),
        Some(Value::Object(_))
        | Some(Value::Number(_))
        | Some(Value::Bool(_))
        | Some(Value::Null)
        | None => Vec::new(),
    }
}

fn chat_message_image_urls(body: &Value, role: &str) -> Vec<String> {
    let role = if role == "developer" { "system" } else { role };
    body.get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some(role))
        .flat_map(chat_message_content_parts)
        .filter_map(chat_content_part_image_url)
        .collect()
}

fn chat_messages_to_response_input(messages: &[Value]) -> Vec<Value> {
    let mut items = Vec::new();
    let mut call_names = HashMap::new();
    let mut index = 0;

    while index < messages.len() {
        let message = &messages[index];
        if message.get("type").and_then(Value::as_str) == Some("agent_message") {
            items.push(message.clone());
            index += 1;
            continue;
        }

        match message.get("role").and_then(Value::as_str) {
            Some("system") | Some("developer") | Some("user") => {
                let source_role = message
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("user");
                let role = if source_role == "system" {
                    "developer"
                } else {
                    source_role
                };
                let mut content = chat_message_content_parts(message)
                    .into_iter()
                    .filter_map(chat_content_part_to_input_content)
                    .collect::<Vec<Value>>();
                if source_role == "system" {
                    content = extract_contextual_system_content(content);
                }
                if !content.is_empty() {
                    if source_role == "system" {
                        items.extend(content.into_iter().map(|part| {
                            serde_json::json!({
                                "type": "message",
                                "role": role,
                                "content": [part],
                            })
                        }));
                    } else {
                        items.push(serde_json::json!({
                            "type": "message",
                            "role": role,
                            "content": content,
                        }));
                    }
                }
            }
            Some("assistant") => {
                let content = chat_message_content_parts(message)
                    .into_iter()
                    .filter_map(chat_content_part_to_output_content)
                    .collect::<Vec<Value>>();
                if !content.is_empty() {
                    items.push(serde_json::json!({
                        "type": "message",
                        "role": "assistant",
                        "content": content,
                    }));
                }

                for tool_call in message
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let Some(call_id) = tool_call.get("id").and_then(Value::as_str) else {
                        continue;
                    };
                    let name = tool_call
                        .pointer("/function/name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let arguments = tool_call
                        .pointer("/function/arguments")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    call_names.insert(call_id.to_string(), name.to_string());
                    items.push(serde_json::json!({
                        "type": chat_tool_call_item_type(name),
                        "call_id": call_id,
                        "name": name,
                        "arguments": arguments,
                    }));
                }
            }
            Some("tool") => {
                let Some(call_id) = message.get("tool_call_id").and_then(Value::as_str) else {
                    index += 1;
                    continue;
                };
                let call_type = call_names
                    .get(call_id)
                    .map(|name| chat_tool_output_item_type(name))
                    .unwrap_or("function_call_output");
                let mut output = chat_tool_message_output(message);
                if let Some(image_items) =
                    following_chat_image_tool_result_items(messages.get(index + 1), call_id)
                {
                    output = chat_tool_message_output_with_images(message, image_items);
                    index += 1;
                }
                items.push(serde_json::json!({
                    "type": call_type,
                    "call_id": call_id,
                    "output": output,
                }));
            }
            Some(_) | None => {}
        }

        index += 1;
    }

    items
}

fn extract_contextual_system_content(content: Vec<Value>) -> Vec<Value> {
    content
        .into_iter()
        .flat_map(|part| {
            let Some(text) = part.get("text").and_then(Value::as_str) else {
                return Vec::new();
            };
            split_chat_text_content(text)
                .into_iter()
                .filter(|part| {
                    is_contextual_system_paragraph(part) || !is_base_system_instruction_text(part)
                })
                .map(|part| serde_json::json!({ "type": "input_text", "text": part }))
                .collect()
        })
        .collect()
}

fn is_contextual_system_paragraph(paragraph: &str) -> bool {
    paragraph.starts_with("<permissions instructions>")
        || (paragraph.starts_with('<') && paragraph.contains("</"))
}

fn is_base_system_instruction_text(paragraph: &str) -> bool {
    paragraph.starts_with("You are ")
        || paragraph.starts_with("Knowledge cutoff:")
        || paragraph.starts_with("Current date:")
}

fn chat_tool_call_item_type(name: &str) -> &'static str {
    if is_custom_tool_name(name) {
        "custom_tool_call"
    } else if name == "tool_search" {
        "tool_search_call"
    } else {
        "function_call"
    }
}

fn chat_tool_output_item_type(name: &str) -> &'static str {
    if is_custom_tool_name(name) {
        "custom_tool_call_output"
    } else if name == "tool_search" {
        "tool_search_output"
    } else {
        "function_call_output"
    }
}

fn is_custom_tool_name(name: &str) -> bool {
    name == "apply_patch"
}

fn chat_message_content_parts(message: &Value) -> Vec<Value> {
    match message.get("content") {
        Some(Value::String(text)) if !text.is_empty() => split_chat_text_content(text)
            .into_iter()
            .map(|text| serde_json::json!({ "type": "text", "text": text }))
            .collect(),
        Some(Value::Array(parts)) => parts.clone(),
        Some(Value::Object(_))
        | Some(Value::Number(_))
        | Some(Value::Bool(_))
        | Some(Value::Null)
        | None
        | Some(Value::String(_)) => Vec::new(),
    }
}

fn split_chat_text_content(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cursor = 0;

    while let Some((relative_start, open_tag, close_tag)) =
        find_next_contextual_text_block(&text[cursor..])
    {
        let start = cursor + relative_start;
        push_trimmed_text_part(&mut parts, &text[cursor..start]);

        let content_start = start + open_tag.len();
        let end = text[content_start..]
            .find(close_tag)
            .map(|relative_end| content_start + relative_end + close_tag.len())
            .unwrap_or(text.len());
        push_trimmed_text_part(&mut parts, &text[start..end]);
        cursor = end;
    }

    push_trimmed_text_part(&mut parts, &text[cursor..]);
    parts
}

fn find_next_contextual_text_block(text: &str) -> Option<(usize, &'static str, &'static str)> {
    contextual_text_blocks()
        .iter()
        .filter_map(|(open_tag, close_tag)| {
            text.find(open_tag)
                .map(|start| (start, *open_tag, *close_tag))
        })
        .min_by_key(|(start, _, _)| *start)
}

fn contextual_text_blocks() -> &'static [(&'static str, &'static str)] {
    &[
        ("<permissions instructions>", "</permissions instructions>"),
        ("<user_instructions>", "</user_instructions>"),
        ("<environment_context>", "</environment_context>"),
        ("<apps_instructions>", "</apps_instructions>"),
        ("<skills_instructions>", "</skills_instructions>"),
        ("<plugins_instructions>", "</plugins_instructions>"),
        ("<collaboration_mode>", "</collaboration_mode>"),
        ("<realtime_conversation>", "</realtime_conversation>"),
        ("<model_switch>", "</model_switch>"),
        ("<turn_aborted>", "</turn_aborted>"),
    ]
}

fn push_trimmed_text_part(parts: &mut Vec<String>, text: &str) {
    let text = text.trim();
    if !text.is_empty() {
        parts.push(text.to_string());
    }
}

fn chat_content_part_to_input_content(part: Value) -> Option<Value> {
    match part.get("type").and_then(Value::as_str) {
        Some("text" | "input_text") => {
            let text = part.get("text").and_then(Value::as_str)?;
            Some(serde_json::json!({ "type": "input_text", "text": text }))
        }
        Some("image_url" | "input_image") => {
            let image_url = chat_content_part_image_url(part.clone())?;
            let mut content = serde_json::json!({
                "type": "input_image",
                "image_url": image_url,
            });
            if let Some(detail) = chat_content_part_image_detail(&part) {
                content["detail"] = detail;
            }
            Some(content)
        }
        Some(_) | None => None,
    }
}

fn chat_content_part_to_output_content(part: Value) -> Option<Value> {
    match part.get("type").and_then(Value::as_str) {
        Some("text" | "output_text" | "input_text") => {
            let text = part.get("text").and_then(Value::as_str)?;
            Some(serde_json::json!({ "type": "output_text", "text": text }))
        }
        Some(_) | None => None,
    }
}

fn chat_content_part_image_url(part: Value) -> Option<String> {
    part.get("image_url")
        .and_then(|image_url| {
            image_url.as_str().map(str::to_string).or_else(|| {
                image_url
                    .get("url")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
        })
        .or_else(|| part.get("data").and_then(Value::as_str).map(str::to_string))
}

fn chat_content_part_image_detail(part: &Value) -> Option<Value> {
    part.get("detail")
        .cloned()
        .or_else(|| part.pointer("/image_url/detail").cloned())
}

fn chat_tool_message_output(message: &Value) -> Value {
    match message.get("content") {
        Some(Value::String(text)) => serde_json::from_str::<Value>(text)
            .unwrap_or_else(|_| chat_tool_text_output(text).unwrap_or(Value::String(text.clone()))),
        Some(Value::Array(parts)) => Value::Array(
            parts
                .iter()
                .filter_map(|part| match part.get("type").and_then(Value::as_str) {
                    Some("text" | "input_text") => part
                        .get("text")
                        .and_then(Value::as_str)
                        .map(|text| serde_json::json!({ "type": "input_text", "text": text })),
                    Some("image_url" | "input_image") => chat_content_part_image_url(part.clone())
                        .map(|image_url| {
                            let mut content = serde_json::json!({
                                "type": "input_image",
                                "image_url": image_url,
                            });
                            if let Some(detail) = chat_content_part_image_detail(part) {
                                content["detail"] = detail;
                            }
                            content
                        }),
                    Some(_) | None => None,
                })
                .collect(),
        ),
        Some(Value::Object(_))
        | Some(Value::Number(_))
        | Some(Value::Bool(_))
        | Some(Value::Null)
        | None => Value::String(String::new()),
    }
}

fn following_chat_image_tool_result_items(
    message: Option<&Value>,
    call_id: &str,
) -> Option<Vec<Value>> {
    let message = message?;
    if message.get("role").and_then(Value::as_str) != Some("user") {
        return None;
    }

    let parts = chat_message_content_parts(message);
    let first_text = parts
        .first()
        .and_then(|part| part.get("text"))
        .and_then(Value::as_str)?;
    if !is_chat_image_tool_result_label(first_text, call_id) {
        return None;
    }

    let image_items = parts
        .into_iter()
        .skip(1)
        .filter_map(|part| match part.get("type").and_then(Value::as_str) {
            Some("image_url" | "input_image") => chat_content_part_to_input_content(part),
            Some(_) | None => None,
        })
        .collect::<Vec<_>>();

    (!image_items.is_empty()).then_some(image_items)
}

fn is_chat_image_tool_result_label(text: &str, call_id: &str) -> bool {
    text == format!("Image returned by tool call {call_id}.")
        || (text.starts_with("Image returned by ")
            && text.ends_with(&format!(" tool call {call_id}.")))
}

fn chat_tool_message_output_with_images(message: &Value, image_items: Vec<Value>) -> Value {
    let output = chat_tool_message_output(message);
    let mut items = match output {
        Value::Array(items) => items,
        Value::String(text) => {
            let text = strip_chat_image_tool_result_notice(&text);
            if text.is_empty() {
                Vec::new()
            } else {
                vec![serde_json::json!({ "type": "input_text", "text": text })]
            }
        }
        Value::Object(_) | Value::Number(_) | Value::Bool(_) | Value::Null => Vec::new(),
    };
    items.extend(image_items);
    Value::Array(items)
}

fn strip_chat_image_tool_result_notice(text: &str) -> String {
    const NOTICE: &str =
        "Tool returned image content. The image is attached in the following user message.";

    if let Some(prefix) = text.strip_suffix(&format!("\n\n{NOTICE}")) {
        return prefix.to_string();
    }

    text.strip_suffix(NOTICE)
        .map_or_else(|| text.to_string(), |prefix| prefix.trim_end().to_string())
}

fn chat_tool_text_output(text: &str) -> Option<Value> {
    if text.starts_with("data:image/") {
        return Some(Value::Array(vec![serde_json::json!({
            "type": "input_image",
            "image_url": text,
        })]));
    }

    let (header, image_url) = text.split_once("\nOutput:\n")?;
    if !header.starts_with("Wall time: ") || !image_url.starts_with("data:image/") {
        return None;
    }

    Some(Value::Array(vec![
        serde_json::json!({
            "type": "input_text",
            "text": format!("{header}\nOutput:"),
        }),
        serde_json::json!({
            "type": "input_image",
            "image_url": image_url,
            "detail": "high",
        }),
    ]))
}

pub fn output_value_to_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => match items.as_slice() {
            [item] if item.get("type").and_then(Value::as_str) == Some("input_text") => {
                item.get("text").and_then(Value::as_str).map(str::to_string)
            }
            [_] | [] | [_, _, ..] => None,
        },
        Value::Object(_) | Value::Number(_) | Value::Bool(_) | Value::Null => None,
    }
}

pub fn namespace_child_tool<'a>(
    body: &'a Value,
    namespace: &str,
    tool_name: &str,
) -> Option<&'a Value> {
    let tools = body.get("tools")?.as_array()?;
    let provider_neutral_name = format!("{namespace}__{tool_name}");
    let legacy_provider_neutral_name = format!("{namespace}___{tool_name}");
    for tool in tools {
        if request_tool_name(tool) == Some(provider_neutral_name.as_str())
            || request_tool_name(tool) == Some(legacy_provider_neutral_name.as_str())
        {
            return Some(tool);
        }

        if tool.get("name").and_then(Value::as_str) == Some(namespace)
            && tool.get("type").and_then(Value::as_str) == Some("namespace")
        {
            let child_tools = tool.get("tools")?.as_array()?;
            if let Some(child_tool) = child_tools
                .iter()
                .find(|tool| tool.get("name").and_then(Value::as_str) == Some(tool_name))
            {
                return Some(child_tool);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::http::HeaderMap;
    use wiremock::http::Method;

    fn request_with_input(input: Value) -> ResponsesRequest {
        ResponsesRequest(wiremock::Request {
            url: "http://localhost/v1/chat/completions"
                .parse()
                .expect("valid request url"),
            method: Method::POST,
            headers: HeaderMap::new(),
            body: serde_json::to_vec(&serde_json::json!({ "input": input }))
                .expect("serialize request body"),
        })
    }

    #[test]
    fn call_output_content_and_success_returns_only_single_text_content_item() {
        let single_text = request_with_input(serde_json::json!([
            {
                "type": "function_call_output",
                "call_id": "call-1",
                "output": [{ "type": "input_text", "text": "hello" }]
            },
            {
                "type": "custom_tool_call_output",
                "call_id": "call-2",
                "output": [{ "type": "input_text", "text": "world" }]
            }
        ]));
        assert_eq!(
            single_text.function_call_output_content_and_success("call-1"),
            Some((Some("hello".to_string()), None))
        );
        assert_eq!(
            single_text.custom_tool_call_output_content_and_success("call-2"),
            Some((Some("world".to_string()), None))
        );

        let mixed_content = request_with_input(serde_json::json!([
            {
                "type": "function_call_output",
                "call_id": "call-3",
                "output": [
                    { "type": "input_text", "text": "hello" },
                    { "type": "input_image", "image_url": "data:image/png;base64,abc" }
                ]
            },
            {
                "type": "custom_tool_call_output",
                "call_id": "call-4",
                "output": [{ "type": "input_image", "image_url": "data:image/png;base64,abc" }]
            }
        ]));
        assert_eq!(
            mixed_content.function_call_output_content_and_success("call-3"),
            Some((None, None))
        );
        assert_eq!(
            mixed_content.custom_tool_call_output_content_and_success("call-4"),
            Some((None, None))
        );
    }

    #[test]
    fn custom_tool_call_response_sse_converts_to_chat_tool_call() {
        let converted = responses_sse_to_chat_completions_sse(&sse(vec![
            ev_response_created("resp-1"),
            ev_custom_tool_call("call-1", "exec", "text(\"hi\")"),
            ev_completed("resp-1"),
        ]))
        .expect("responses SSE should convert");

        assert!(
            converted.contains(r#""tool_calls""#),
            "converted SSE should include chat tool calls: {converted}"
        );
        assert!(
            converted.contains(r#""finish_reason":"tool_calls""#),
            "converted SSE should preserve tool-use stop reason: {converted}"
        );
        assert!(
            converted.contains(r#""arguments":"{\"input\":\"text(\\\"hi\\\")\"}""#),
            "converted SSE should carry custom input as function arguments: {converted}"
        );
    }
}

#[derive(Debug, Clone)]
pub struct WebSocketRequest {
    body: Value,
}

impl WebSocketRequest {
    pub fn body_json(&self) -> Value {
        self.body.clone()
    }
}

#[derive(Debug, Clone)]
pub struct WebSocketHandshake {
    uri: String,
    headers: Vec<(String, String)>,
}

impl WebSocketHandshake {
    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn header(&self, name: &str) -> Option<String> {
        self.headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone())
    }
}

#[derive(Debug, Clone)]
pub struct WebSocketConnectionConfig {
    pub requests: Vec<Vec<Value>>,
    pub response_headers: Vec<(String, String)>,
    /// Optional delay inserted before accepting the websocket handshake.
    ///
    /// Tests use this to force websocket setup into an in-flight state so first-turn warmup paths
    /// can be exercised deterministically.
    pub accept_delay: Option<Duration>,
    /// Whether the server should send a websocket close frame after all scripted responses.
    ///
    /// Tests can disable this to simulate a peer that surfaces a terminal event but never
    /// completes the close handshake.
    pub close_after_requests: bool,
}

pub struct WebSocketTestServer {
    uri: String,
    connections: Arc<Mutex<Vec<Vec<WebSocketRequest>>>>,
    handshakes: Arc<Mutex<Vec<WebSocketHandshake>>>,
    request_log_updated: Arc<Notify>,
    preflight_connection_available: bool,
    shutdown: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl WebSocketTestServer {
    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn connections(&self) -> Vec<Vec<WebSocketRequest>> {
        self.connections.lock().unwrap().clone()
    }

    pub fn single_connection(&self) -> Vec<WebSocketRequest> {
        let connections = self.connections.lock().unwrap();
        if connections.len() != 1 {
            panic!("expected 1 connection, got {}", connections.len());
        }
        connections.first().cloned().unwrap_or_default()
    }

    pub async fn wait_for_request(
        &self,
        connection_index: usize,
        request_index: usize,
    ) -> WebSocketRequest {
        loop {
            if let Some(request) = self
                .connections
                .lock()
                .unwrap()
                .get(connection_index)
                .and_then(|connection| connection.get(request_index))
                .cloned()
            {
                return request;
            }
            self.request_log_updated.notified().await;
        }
    }

    pub fn handshakes(&self) -> Vec<WebSocketHandshake> {
        self.handshakes.lock().unwrap().clone()
    }

    pub async fn connect_preflight_if_first_connection_empty(&self) {
        if !self.preflight_connection_available {
            return;
        }
        if let Ok((mut stream, _)) = tokio_tungstenite::connect_async(self.uri.as_str()).await {
            let _ = stream.close(None).await;
        }
    }

    /// Waits until at least `expected` websocket handshakes have been observed or timeout elapses.
    ///
    /// Uses a short bounded polling interval so tests can deterministically wait for background
    /// websocket activity without busy-spinning.
    pub async fn wait_for_handshakes(&self, expected: usize, timeout: Duration) -> bool {
        if self.handshakes.lock().unwrap().len() >= expected {
            return true;
        }

        let deadline = tokio::time::Instant::now() + timeout;
        let poll_interval = Duration::from_millis(10);
        loop {
            if self.handshakes.lock().unwrap().len() >= expected {
                return true;
            }
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return false;
            }
            let sleep_for = std::cmp::min(poll_interval, deadline.saturating_duration_since(now));
            tokio::time::sleep(sleep_for).await;
        }
    }
    pub fn single_handshake(&self) -> WebSocketHandshake {
        let handshakes = self.handshakes.lock().unwrap();
        if handshakes.len() != 1 {
            panic!("expected 1 handshake, got {}", handshakes.len());
        }
        handshakes.first().cloned().unwrap()
    }

    pub async fn shutdown(self) {
        let _ = self.shutdown.send(());
        let mut task = self.task;
        if tokio::time::timeout(Duration::from_secs(10), &mut task)
            .await
            .is_err()
        {
            task.abort();
            let _ = task.await;
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelsMock {
    requests: Arc<Mutex<Vec<wiremock::Request>>>,
}

impl ModelsMock {
    fn new() -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn requests(&self) -> Vec<wiremock::Request> {
        self.requests.lock().unwrap().clone()
    }

    pub fn single_request_path(&self) -> String {
        let requests = self.requests.lock().unwrap();
        if requests.len() != 1 {
            panic!("expected 1 request, got {}", requests.len());
        }
        requests.first().unwrap().url.path().to_string()
    }
}

impl Match for ModelsMock {
    fn matches(&self, request: &wiremock::Request) -> bool {
        self.requests.lock().unwrap().push(request.clone());
        true
    }
}

impl Match for ResponseMock {
    fn matches(&self, request: &wiremock::Request) -> bool {
        self.requests
            .lock()
            .unwrap()
            .push(ResponsesRequest(request.clone()));

        // Enforce invariant checks on every request body captured by the mock.
        // Panic on orphan tool outputs or calls to catch regressions early.
        validate_request_body_invariants(request);
        true
    }
}

/// Build an SSE stream body from a list of JSON events.
pub fn sse(events: Vec<Value>) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for ev in events {
        let kind = ev.get("type").and_then(|v| v.as_str()).unwrap();
        writeln!(&mut out, "event: {kind}").unwrap();
        if !ev.as_object().map(|o| o.len() == 1).unwrap_or(false) {
            write!(&mut out, "data: {ev}\n\n").unwrap();
        } else {
            out.push('\n');
        }
    }
    out
}

pub fn sse_completed(id: &str) -> String {
    sse(vec![ev_response_created(id), ev_completed(id)])
}

/// Convenience: SSE event for a completed response with a specific id.
pub fn ev_completed(id: &str) -> Value {
    serde_json::json!({
        "type": "response.completed",
        "response": {
            "id": id,
            "usage": {"input_tokens":0,"input_tokens_details":null,"output_tokens":0,"output_tokens_details":null,"total_tokens":0}
        }
    })
}

/// Convenience: SSE event for a created response with a specific id.
pub fn ev_response_created(id: &str) -> Value {
    serde_json::json!({
        "type": "response.created",
        "response": {
            "id": id,
        }
    })
}

pub fn ev_model_verification_metadata(id: &str, verifications: Vec<&str>) -> Value {
    serde_json::json!({
        "type": "response.metadata",
        "sequence_number": 1,
        "response_id": id,
        "metadata": {
            "openai_verification_recommendation": verifications,
        }
    })
}

pub fn ev_completed_with_tokens(id: &str, total_tokens: i64) -> Value {
    serde_json::json!({
        "type": "response.completed",
        "response": {
            "id": id,
            "usage": {
                "input_tokens": total_tokens,
                "input_tokens_details": null,
                "output_tokens": 0,
                "output_tokens_details": null,
                "total_tokens": total_tokens
            }
        }
    })
}

/// Convenience: SSE event for a single assistant message output item.
pub fn ev_assistant_message(id: &str, text: &str) -> Value {
    serde_json::json!({
        "type": "response.output_item.done",
        "item": {
            "type": "message",
            "role": "assistant",
            "id": id,
            "content": [{"type": "output_text", "text": text}]
        }
    })
}

pub fn user_message_item(text: &str) -> TranscriptItem {
    TranscriptItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        phase: None,
    }
}

pub fn ev_message_item_added(id: &str, text: &str) -> Value {
    serde_json::json!({
        "type": "response.output_item.added",
        "item": {
            "type": "message",
            "role": "assistant",
            "id": id,
            "content": [{"type": "output_text", "text": text}]
        }
    })
}

pub fn ev_output_text_delta(delta: &str) -> Value {
    serde_json::json!({
        "type": "response.output_text.delta",
        "delta": delta,
    })
}

pub fn ev_reasoning_item(id: &str, summary: &[&str], raw_content: &[&str]) -> Value {
    let summary_entries: Vec<Value> = summary
        .iter()
        .map(|text| serde_json::json!({"type": "summary_text", "text": text}))
        .collect();

    let overhead = "b".repeat(550);
    let raw_content_joined = raw_content.join("");
    let encrypted_content =
        base64::engine::general_purpose::STANDARD.encode(overhead + raw_content_joined.as_str());

    let mut event = serde_json::json!({
        "type": "response.output_item.done",
        "item": {
            "type": "reasoning",
            "id": id,
            "summary": summary_entries,
            "encrypted_content": encrypted_content,
        }
    });

    if !raw_content.is_empty() {
        let content_entries: Vec<Value> = raw_content
            .iter()
            .map(|text| serde_json::json!({"type": "reasoning_text", "text": text}))
            .collect();
        event["item"]["content"] = Value::Array(content_entries);
    }

    event
}

pub fn ev_reasoning_item_added(id: &str, summary: &[&str]) -> Value {
    let summary_entries: Vec<Value> = summary
        .iter()
        .map(|text| serde_json::json!({"type": "summary_text", "text": text}))
        .collect();

    serde_json::json!({
        "type": "response.output_item.added",
        "item": {
            "type": "reasoning",
            "id": id,
            "summary": summary_entries,
        }
    })
}

pub fn ev_reasoning_summary_text_delta(delta: &str) -> Value {
    serde_json::json!({
        "type": "response.reasoning_summary_text.delta",
        "delta": delta,
        "summary_index": 0,
    })
}

pub fn ev_reasoning_text_delta(delta: &str) -> Value {
    serde_json::json!({
        "type": "response.reasoning_text.delta",
        "delta": delta,
        "content_index": 0,
    })
}

pub fn ev_web_search_call_added_partial(id: &str, status: &str) -> Value {
    serde_json::json!({
        "type": "response.output_item.added",
        "item": {
            "type": "web_search_call",
            "id": id,
            "status": status
        }
    })
}

pub fn ev_web_search_call_done(id: &str, status: &str, query: &str) -> Value {
    serde_json::json!({
        "type": "response.output_item.done",
        "item": {
            "type": "web_search_call",
            "id": id,
            "status": status,
            "action": {"type": "search", "query": query}
        }
    })
}

pub fn ev_image_generation_call(
    id: &str,
    status: &str,
    revised_prompt: &str,
    result: &str,
) -> Value {
    serde_json::json!({
        "type": "response.output_item.done",
        "item": {
            "type": "image_generation_call",
            "id": id,
            "status": status,
            "revised_prompt": revised_prompt,
            "result": result,
        }
    })
}

pub fn ev_function_call(call_id: &str, name: &str, arguments: &str) -> Value {
    serde_json::json!({
        "type": "response.output_item.done",
        "item": {
            "type": "function_call",
            "call_id": call_id,
            "name": name,
            "arguments": arguments
        }
    })
}

pub fn ev_function_call_with_namespace(
    call_id: &str,
    namespace: &str,
    name: &str,
    arguments: &str,
) -> Value {
    serde_json::json!({
        "type": "response.output_item.done",
        "item": {
            "type": "function_call",
            "call_id": call_id,
            "namespace": namespace,
            "name": name,
            "arguments": arguments
        }
    })
}

pub fn ev_tool_search_call(call_id: &str, arguments: &serde_json::Value) -> Value {
    serde_json::json!({
        "type": "response.output_item.done",
        "item": {
            "type": "tool_search_call",
            "call_id": call_id,
            "execution": "client",
            "arguments": arguments,
        }
    })
}

pub fn ev_custom_tool_call(call_id: &str, name: &str, input: &str) -> Value {
    serde_json::json!({
        "type": "response.output_item.done",
        "item": {
            "type": "custom_tool_call",
            "call_id": call_id,
            "name": name,
            "input": input
        }
    })
}

pub fn ev_local_shell_call(call_id: &str, status: &str, command: Vec<&str>) -> Value {
    serde_json::json!({
        "type": "response.output_item.done",
        "item": {
            "type": "local_shell_call",
            "call_id": call_id,
            "status": status,
            "action": {
                "type": "exec",
                "command": command,
            }
        }
    })
}

/// Convenience: SSE event for an `apply_patch` custom tool call with raw patch
/// text. This mirrors the legacy event payload used by tests when the model
/// invokes `apply_patch` directly.
pub fn ev_apply_patch_custom_tool_call(call_id: &str, patch: &str) -> Value {
    serde_json::json!({
        "type": "response.output_item.done",
        "item": {
            "type": "custom_tool_call",
            "name": "apply_patch",
            "input": patch,
            "call_id": call_id
        }
    })
}

pub fn ev_shell_command_call(call_id: &str, command: &str) -> Value {
    let args = serde_json::json!({ "command": command });
    ev_shell_command_call_with_args(call_id, &args)
}

pub fn ev_shell_command_call_with_args(call_id: &str, args: &serde_json::Value) -> Value {
    let arguments = serde_json::to_string(args).expect("serialize shell command arguments");
    ev_function_call(call_id, "shell_command", &arguments)
}

pub fn ev_apply_patch_shell_command_call_via_heredoc(call_id: &str, patch: &str) -> Value {
    let args = serde_json::json!({ "command": format!("apply_patch <<'EOF'\n{patch}\nEOF\n") });
    let arguments = serde_json::to_string(&args).expect("serialize apply_patch arguments");

    ev_function_call(call_id, "shell_command", &arguments)
}

pub fn sse_failed(id: &str, code: &str, message: &str) -> String {
    sse(vec![serde_json::json!({
        "type": "response.failed",
        "response": {
            "id": id,
            "error": {"code": code, "message": message}
        }
    })])
}

pub fn sse_response(body: String) -> ResponseTemplate {
    let body = responses_sse_to_chat_completions_sse(&body).unwrap_or(body);
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_raw(body, "text/event-stream")
}

pub async fn mount_response_once(server: &MockServer, response: ResponseTemplate) -> ResponseMock {
    let (mock, response_mock) = base_mock();
    mock.respond_with(response)
        .up_to_n_times(1)
        .mount(server)
        .await;
    response_mock
}

pub async fn mount_response_once_match<M>(
    server: &MockServer,
    matcher: M,
    response: ResponseTemplate,
) -> ResponseMock
where
    M: wiremock::Match + Send + Sync + 'static,
{
    let (mock, response_mock) = base_mock();
    mock.and(matcher)
        .respond_with(response)
        .up_to_n_times(1)
        .mount(server)
        .await;
    response_mock
}

fn base_mock() -> (MockBuilder, ResponseMock) {
    let response_mock = ResponseMock::new();
    let mock = Mock::given(method("POST"))
        .and(path_regex(".*/chat/completions$"))
        .and(response_mock.clone());
    (mock, response_mock)
}

fn chat_completions_mock() -> (MockBuilder, ResponseMock) {
    let response_mock = ResponseMock::new();
    let mock = Mock::given(method("POST"))
        .and(path_regex(".*/chat/completions$"))
        .and(response_mock.clone());
    (mock, response_mock)
}

fn models_mock() -> (MockBuilder, ModelsMock) {
    let models_mock = ModelsMock::new();
    let mock = Mock::given(method("GET"))
        .and(path_regex(".*/models$"))
        .and(models_mock.clone());
    (mock, models_mock)
}

pub async fn mount_sse_once_match<M>(server: &MockServer, matcher: M, body: String) -> ResponseMock
where
    M: wiremock::Match + Send + Sync + 'static,
{
    let (mock, response_mock) = base_mock();
    mock.and(matcher)
        .respond_with(sse_response(body))
        .up_to_n_times(1)
        .mount(server)
        .await;
    response_mock
}

pub fn chat_completions_sse(chunks: Vec<Value>) -> String {
    let mut body = String::new();
    for chunk in chunks {
        body.push_str("data: ");
        body.push_str(&chunk.to_string());
        body.push_str("\n\n");
    }
    body.push_str("data: [DONE]\n\n");
    body
}

pub fn chat_completions_text_sse(text: &str) -> String {
    chat_completions_sse(vec![serde_json::json!({
        "id": "chatcmpl-test",
        "model": "astral-test-model",
        "choices": [{
            "delta": {
                "role": "assistant",
                "content": text,
            },
            "finish_reason": "stop",
        }],
        "usage": {
            "prompt_tokens": 1,
            "completion_tokens": 1,
        },
    })])
}

pub fn chat_completions_completed_sse() -> String {
    chat_completions_sse(vec![serde_json::json!({
        "id": "chatcmpl-test",
        "model": "astral-test-model",
        "choices": [{
            "delta": {},
            "finish_reason": "stop",
        }],
    })])
}

pub fn chat_completions_error_sse(message: &str) -> String {
    chat_completions_sse(vec![serde_json::json!({
        "error": {
            "message": message,
        },
    })])
}

pub fn chat_completions_apply_patch_tool_call_sse(call_id: &str, patch: &str) -> String {
    let arguments =
        serde_json::to_string(&serde_json::json!({ "input": patch })).expect("serialize patch");
    chat_completions_sse(vec![serde_json::json!({
        "id": "chatcmpl-test",
        "model": "astral-test-model",
        "choices": [{
            "delta": {
                "role": "assistant",
                "tool_calls": [{
                    "index": 0,
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": "apply_patch",
                        "arguments": arguments,
                    },
                }],
            },
            "finish_reason": "tool_calls",
        }],
    })])
}

pub fn responses_sse_to_chat_completions_sse(body: &str) -> Option<String> {
    ResponsesSseToChatCompletionsConverter::new().convert_body(body)
}

pub fn responses_sse_to_chat_completions_sse_chunk(body: &str) -> Option<String> {
    ResponsesSseToChatCompletionsConverter::new().convert_chunk(body)
}

#[derive(Debug)]
pub struct ResponsesSseToChatCompletionsConverter {
    response_id: String,
    model: Option<String>,
    message_started: bool,
    text_delta_seen: bool,
    pending_tool_call: bool,
    next_tool_index: u64,
    tool_states: HashMap<String, ToolCallState>,
    item_id_to_call_id: HashMap<String, String>,
    reasoning_texts_by_item: HashMap<String, HashSet<String>>,
    converted_responses_event: bool,
}

impl ResponsesSseToChatCompletionsConverter {
    pub fn new() -> Self {
        Self {
            response_id: "chatcmpl-test".to_string(),
            model: None,
            message_started: false,
            text_delta_seen: false,
            pending_tool_call: false,
            next_tool_index: 0,
            tool_states: HashMap::new(),
            item_id_to_call_id: HashMap::new(),
            reasoning_texts_by_item: HashMap::new(),
            converted_responses_event: false,
        }
    }

    pub fn converted_responses_event(&self) -> bool {
        self.converted_responses_event
    }

    pub fn convert_chunk(&mut self, body: &str) -> Option<String> {
        let converted = self.convert_body(body)?;
        if converted == body {
            return Some(converted);
        }
        Some(strip_chat_done(converted))
    }

    fn convert_body(&mut self, body: &str) -> Option<String> {
        let events = parse_sse_data_values(body);
        if events.is_empty() {
            return None;
        }
        if events
            .iter()
            .any(|event| event.get("choices").is_some() || event.get("error").is_some())
        {
            return Some(body.to_string());
        }

        let mut chunks = Vec::new();
        for event in events {
            let Some(event_type) = event.get("type").and_then(Value::as_str) else {
                continue;
            };
            if event_type.starts_with("response.") {
                self.converted_responses_event = true;
            }
            match event_type {
                "response.created" => {
                    self.response_id = event
                        .pointer("/response/id")
                        .and_then(Value::as_str)
                        .unwrap_or(self.response_id.as_str())
                        .to_string();
                    if let Some(model) = response_model_from_event(&event) {
                        self.model = Some(model);
                    }
                    self.push_message_start(&mut chunks);
                }
                "response.output_text.delta" => {
                    self.push_message_start(&mut chunks);
                    if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                        push_text_chunk(
                            &mut chunks,
                            &self.response_id,
                            self.model.as_deref(),
                            delta,
                        );
                        self.text_delta_seen = true;
                    }
                }
                "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                    self.push_message_start(&mut chunks);
                    if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                        push_reasoning_chunk(
                            &mut chunks,
                            &self.response_id,
                            self.model.as_deref(),
                            delta,
                        );
                    }
                }
                "response.output_item.added" => {
                    let Some(item) = event.get("item") else {
                        continue;
                    };
                    match item.get("type").and_then(Value::as_str) {
                        Some("message") => {
                            self.push_message_start(&mut chunks);
                            if let Some(text) = message_item_text(item) {
                                push_text_chunk(
                                    &mut chunks,
                                    &self.response_id,
                                    self.model.as_deref(),
                                    &text,
                                );
                                self.text_delta_seen = true;
                            }
                        }
                        Some("reasoning") => {
                            self.push_message_start(&mut chunks);
                            self.push_reasoning_item_texts(&mut chunks, item);
                        }
                        Some("function_call")
                        | Some("custom_tool_call")
                        | Some("tool_search_call") => self.push_tool_call_item(&mut chunks, item),
                        _ => {
                            if tool_call_from_item(item).is_some() {
                                self.push_tool_call_item(&mut chunks, item);
                            }
                        }
                    }
                }
                "response.function_call_arguments.delta"
                | "response.custom_tool_call_input.delta" => {
                    self.push_tool_call_arguments_delta(&mut chunks, event_type, &event);
                }
                "response.output_item.done" => {
                    let Some(item) = event.get("item") else {
                        continue;
                    };
                    match item.get("type").and_then(Value::as_str) {
                        Some("message") => {
                            self.push_message_start(&mut chunks);
                            if !self.text_delta_seen
                                && let Some(text) = message_item_text(item)
                            {
                                push_text_chunk(
                                    &mut chunks,
                                    &self.response_id,
                                    self.model.as_deref(),
                                    &text,
                                );
                            }
                        }
                        Some("reasoning") => {
                            self.push_message_start(&mut chunks);
                            self.push_reasoning_item_texts(&mut chunks, item);
                        }
                        Some("function_call")
                        | Some("custom_tool_call")
                        | Some("tool_search_call") => {
                            self.push_tool_call_done_item(&mut chunks, item)
                        }
                        _ => {}
                    }
                }
                "response.failed" => {
                    let message = event
                        .pointer("/response/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("response failed");
                    let code = event
                        .pointer("/response/error/code")
                        .and_then(Value::as_str)
                        .unwrap_or("response_failed");
                    chunks.push(serde_json::json!({
                        "error": { "code": code, "message": message },
                    }));
                }
                "response.incomplete" => {
                    let reason = event
                        .pointer("/response/incomplete_details/reason")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    chunks.push(serde_json::json!({
                        "error": {
                            "message": format!("Incomplete response returned, reason: {reason}"),
                        },
                    }));
                }
                "response.completed" => {
                    if let Some(model) = response_model_from_event(&event) {
                        self.model = Some(model);
                    }
                    self.push_message_start(&mut chunks);
                    let finish_reason = if self.pending_tool_call {
                        "tool_calls"
                    } else {
                        "stop"
                    };
                    chunks.push(chat_chunk(
                        &self.response_id,
                        self.model.as_deref(),
                        serde_json::json!({}),
                        Some(finish_reason),
                        chat_usage_from_response_completed(&event),
                    ));
                    self.pending_tool_call = false;
                    self.tool_states.clear();
                    self.item_id_to_call_id.clear();
                }
                _ => {}
            }
        }

        Some(chat_completions_sse(chunks))
    }

    fn push_message_start(&mut self, chunks: &mut Vec<Value>) {
        push_message_start(
            chunks,
            &self.response_id,
            self.model.as_deref(),
            &mut self.message_started,
        );
    }

    fn push_reasoning_item_texts(&mut self, chunks: &mut Vec<Value>, item: &Value) {
        let item_id = item.get("id").and_then(Value::as_str);
        for text in reasoning_item_texts(item) {
            if let Some(item_id) = item_id {
                let seen = self
                    .reasoning_texts_by_item
                    .entry(item_id.to_string())
                    .or_default();
                if !seen.insert(text.clone()) {
                    continue;
                }
            }
            push_reasoning_chunk(chunks, &self.response_id, self.model.as_deref(), &text);
        }
    }

    fn push_tool_call_item(&mut self, chunks: &mut Vec<Value>, item: &Value) {
        let Some((call_id, name, arguments, item_id)) = tool_call_from_item(item) else {
            return;
        };
        self.push_message_start(chunks);
        let state = ensure_tool_state(
            chunks,
            &self.response_id,
            self.model.as_deref(),
            &mut self.tool_states,
            &mut self.next_tool_index,
            &call_id,
            &name,
        );
        if let Some(item_id) = item_id {
            self.item_id_to_call_id.insert(item_id, call_id.clone());
        }
        if !arguments.is_empty() {
            push_tool_arguments_chunk(
                chunks,
                &self.response_id,
                self.model.as_deref(),
                state.index,
                &arguments,
            );
            state.has_argument_delta = true;
        }
        self.pending_tool_call = true;
    }

    fn push_tool_call_arguments_delta(
        &mut self,
        chunks: &mut Vec<Value>,
        event_type: &str,
        event: &Value,
    ) {
        let Some(delta) = event.get("delta").and_then(Value::as_str) else {
            return;
        };
        let call_id = event
            .get("call_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                event
                    .get("item_id")
                    .and_then(Value::as_str)
                    .and_then(|item_id| self.item_id_to_call_id.get(item_id).cloned())
            });
        let Some(call_id) = call_id else {
            return;
        };
        let Some(state) = self.tool_states.get_mut(&call_id) else {
            return;
        };
        let arguments = if event_type == "response.custom_tool_call_input.delta" {
            if !state.custom_input_open {
                push_tool_arguments_chunk(
                    chunks,
                    &self.response_id,
                    self.model.as_deref(),
                    state.index,
                    "\"",
                );
                state.custom_input_open = true;
            }
            state.custom_input_saw_delta = true;
            json_string_fragment(delta)
        } else {
            delta.to_string()
        };
        push_tool_arguments_chunk(
            chunks,
            &self.response_id,
            self.model.as_deref(),
            state.index,
            &arguments,
        );
        state.has_argument_delta = true;
        self.pending_tool_call = true;
    }

    fn push_tool_call_done_item(&mut self, chunks: &mut Vec<Value>, item: &Value) {
        let Some((call_id, name, arguments, item_id)) = tool_call_from_item(item) else {
            return;
        };
        self.push_message_start(chunks);
        let state = ensure_tool_state(
            chunks,
            &self.response_id,
            self.model.as_deref(),
            &mut self.tool_states,
            &mut self.next_tool_index,
            &call_id,
            &name,
        );
        if let Some(item_id) = item_id {
            self.item_id_to_call_id.insert(item_id, call_id.clone());
        }
        if state.custom_input_open {
            if !state.custom_input_saw_delta
                && let Some(input) = item.get("input").and_then(Value::as_str)
                && !input.is_empty()
            {
                let input = json_string_fragment(input);
                push_tool_arguments_chunk(
                    chunks,
                    &self.response_id,
                    self.model.as_deref(),
                    state.index,
                    &input,
                );
            }
            push_tool_arguments_chunk(
                chunks,
                &self.response_id,
                self.model.as_deref(),
                state.index,
                "\"",
            );
            state.custom_input_open = false;
            state.has_argument_delta = true;
        } else if !state.has_argument_delta && !arguments.is_empty() {
            push_tool_arguments_chunk(
                chunks,
                &self.response_id,
                self.model.as_deref(),
                state.index,
                &arguments,
            );
            state.has_argument_delta = true;
        }
        self.pending_tool_call = true;
    }
}

impl Default for ResponsesSseToChatCompletionsConverter {
    fn default() -> Self {
        Self::new()
    }
}

fn strip_chat_done(mut body: String) -> String {
    const DONE: &str = "data: [DONE]\n\n";
    if body.ends_with(DONE) {
        body.truncate(body.len() - DONE.len());
    }
    body
}

fn parse_sse_data_values(body: &str) -> Vec<Value> {
    let mut values = Vec::new();
    let mut data_lines = Vec::new();

    for line in body.lines() {
        if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.trim_start().to_string());
        } else if line.is_empty() {
            flush_sse_data(&mut values, &mut data_lines);
        }
    }
    flush_sse_data(&mut values, &mut data_lines);

    values
}

fn flush_sse_data(values: &mut Vec<Value>, data_lines: &mut Vec<String>) {
    if data_lines.is_empty() {
        return;
    }
    let data = data_lines.join("\n");
    data_lines.clear();
    if data.trim() == "[DONE]" {
        return;
    }
    if let Ok(value) = serde_json::from_str::<Value>(&data) {
        values.push(value);
    }
}

fn response_model_from_event(event: &Value) -> Option<String> {
    event
        .pointer("/response/model")
        .or_else(|| event.pointer("/response/headers/OpenAI-Model"))
        .or_else(|| event.pointer("/response/headers/openai-model"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn push_message_start(
    chunks: &mut Vec<Value>,
    response_id: &str,
    model: Option<&str>,
    message_started: &mut bool,
) {
    if *message_started {
        return;
    }
    chunks.push(chat_chunk(
        response_id,
        model,
        serde_json::json!({ "role": "assistant" }),
        None,
        None,
    ));
    *message_started = true;
}

fn push_text_chunk(chunks: &mut Vec<Value>, response_id: &str, model: Option<&str>, text: &str) {
    if text.is_empty() {
        return;
    }
    chunks.push(chat_chunk(
        response_id,
        model,
        serde_json::json!({ "content": text }),
        None,
        None,
    ));
}

fn push_reasoning_chunk(
    chunks: &mut Vec<Value>,
    response_id: &str,
    model: Option<&str>,
    text: &str,
) {
    if text.is_empty() {
        return;
    }
    chunks.push(chat_chunk(
        response_id,
        model,
        serde_json::json!({ "reasoning_content": text }),
        None,
        None,
    ));
}

fn push_tool_start_chunk(
    chunks: &mut Vec<Value>,
    response_id: &str,
    model: Option<&str>,
    index: u64,
    call_id: &str,
    name: &str,
) {
    chunks.push(chat_chunk(
        response_id,
        model,
        serde_json::json!({
            "tool_calls": [{
                "index": index,
                "id": call_id,
                "type": "function",
                "function": { "name": name },
            }],
        }),
        None,
        None,
    ));
}

fn push_tool_arguments_chunk(
    chunks: &mut Vec<Value>,
    response_id: &str,
    model: Option<&str>,
    index: u64,
    arguments: &str,
) {
    if arguments.is_empty() {
        return;
    }
    chunks.push(chat_chunk(
        response_id,
        model,
        serde_json::json!({
            "tool_calls": [{
                "index": index,
                "function": { "arguments": arguments },
            }],
        }),
        None,
        None,
    ));
}

fn chat_chunk(
    response_id: &str,
    model: Option<&str>,
    delta: Value,
    finish_reason: Option<&str>,
    usage: Option<Value>,
) -> Value {
    let mut chunk = serde_json::json!({
        "id": response_id,
        "choices": [{
            "delta": delta,
            "finish_reason": finish_reason,
        }],
    });
    if let Some(model) = model {
        chunk["model"] = Value::String(model.to_string());
    }
    if let Some(usage) = usage {
        chunk["usage"] = usage;
    }
    chunk
}

#[derive(Debug)]
struct ToolCallState {
    index: u64,
    has_argument_delta: bool,
    custom_input_open: bool,
    custom_input_saw_delta: bool,
}

fn ensure_tool_state<'a>(
    chunks: &mut Vec<Value>,
    response_id: &str,
    model: Option<&str>,
    states: &'a mut std::collections::HashMap<String, ToolCallState>,
    next_tool_index: &mut u64,
    call_id: &str,
    name: &str,
) -> &'a mut ToolCallState {
    if !states.contains_key(call_id) {
        let index = *next_tool_index;
        *next_tool_index += 1;
        push_tool_start_chunk(chunks, response_id, model, index, call_id, name);
        states.insert(
            call_id.to_string(),
            ToolCallState {
                index,
                has_argument_delta: false,
                custom_input_open: false,
                custom_input_saw_delta: false,
            },
        );
    }
    states.get_mut(call_id).expect("state inserted")
}

fn json_string_fragment(text: &str) -> String {
    let quoted = serde_json::to_string(text).expect("serialize text as JSON string");
    quoted
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .unwrap_or(quoted.as_str())
        .to_string()
}

fn tool_call_from_item(item: &Value) -> Option<(String, String, String, Option<String>)> {
    let item_type = item.get("type").and_then(Value::as_str)?;
    match item_type {
        "function_call" => {
            let call_id = item.get("call_id").and_then(Value::as_str)?.to_string();
            let name = tool_name_from_item(item)?;
            let arguments = item
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let item_id = item.get("id").and_then(Value::as_str).map(str::to_string);
            Some((call_id, name, arguments, item_id))
        }
        "custom_tool_call" => {
            let call_id = item.get("call_id").and_then(Value::as_str)?.to_string();
            let name = item.get("name").and_then(Value::as_str)?.to_string();
            let input = item
                .get("input")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let arguments = if input.is_empty() {
                String::new()
            } else {
                serde_json::to_string(&serde_json::json!({ "input": input })).ok()?
            };
            let item_id = item.get("id").and_then(Value::as_str).map(str::to_string);
            Some((call_id, name, arguments, item_id))
        }
        "tool_search_call" => {
            let call_id = item.get("call_id").and_then(Value::as_str)?.to_string();
            let arguments = serde_json::to_string(
                item.get("arguments")
                    .unwrap_or(&Value::Object(serde_json::Map::new())),
            )
            .ok()?;
            let item_id = item.get("id").and_then(Value::as_str).map(str::to_string);
            Some((call_id, "tool_search".to_string(), arguments, item_id))
        }
        _ => None,
    }
}

fn tool_name_from_item(item: &Value) -> Option<String> {
    let name = item.get("name").and_then(Value::as_str)?;
    match item.get("namespace").and_then(Value::as_str) {
        Some(namespace) if !namespace.is_empty() => Some(format!("{namespace}__{name}")),
        Some(_) | None => Some(name.to_string()),
    }
}

fn message_item_text(item: &Value) -> Option<String> {
    let text = item
        .get("content")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|content| {
            if !matches!(
                content.get("type").and_then(Value::as_str),
                Some("output_text" | "input_text")
            ) {
                return None;
            }
            content.get("text").and_then(Value::as_str)
        })
        .collect::<Vec<_>>()
        .join("");
    (!text.is_empty()).then_some(text)
}

fn reasoning_item_texts(item: &Value) -> Vec<String> {
    item.get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(
            item.get("summary")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        )
        .filter_map(|content| content.get("text").and_then(Value::as_str))
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .collect()
}

fn chat_usage_from_response_completed(event: &Value) -> Option<Value> {
    let usage = event.pointer("/response/usage")?;
    if usage.is_null() {
        return None;
    }
    let mut chat_usage = serde_json::json!({
        "prompt_tokens": usage
            .get("input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        "completion_tokens": usage
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        "total_tokens": usage
            .get("total_tokens")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
    });
    if let Some(cached_tokens) = usage
        .pointer("/input_tokens_details/cached_tokens")
        .and_then(Value::as_u64)
    {
        chat_usage["prompt_tokens_details"] = serde_json::json!({ "cached_tokens": cached_tokens });
    }
    Some(chat_usage)
}

pub async fn mount_chat_completions_sse_once_match<M>(
    server: &MockServer,
    matcher: M,
    body: String,
) -> ResponseMock
where
    M: wiremock::Match + Send + Sync + 'static,
{
    let (mock, response_mock) = chat_completions_mock();
    mock.and(matcher)
        .respond_with(sse_response(body))
        .up_to_n_times(1)
        .mount(server)
        .await;
    response_mock
}

pub async fn mount_chat_completions_sse_once(server: &MockServer, body: String) -> ResponseMock {
    let (mock, response_mock) = chat_completions_mock();
    mock.respond_with(sse_response(body))
        .up_to_n_times(1)
        .mount(server)
        .await;
    response_mock
}

pub async fn mount_chat_completions_text_once(server: &MockServer, text: &str) -> ResponseMock {
    mount_chat_completions_sse_once(server, chat_completions_text_sse(text)).await
}

pub async fn mount_sse_once(server: &MockServer, body: String) -> ResponseMock {
    let (mock, response_mock) = base_mock();
    mock.respond_with(sse_response(body))
        .up_to_n_times(1)
        .mount(server)
        .await;
    response_mock
}

pub async fn mount_responses_sse_once(server: &MockServer, body: String) -> ResponseMock {
    let response_mock = ResponseMock::new();
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .and(response_mock.clone())
        .respond_with(raw_sse_response(body))
        .up_to_n_times(1)
        .mount(server)
        .await;
    response_mock
}

/// Mounts a sequence of raw Responses SSE bodies and serves them in order for
/// each POST to `/v1/responses`.
pub async fn mount_responses_sse_sequence(
    server: &MockServer,
    bodies: Vec<String>,
) -> ResponseMock {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    struct SeqResponder {
        num_calls: AtomicUsize,
        responses: Vec<String>,
    }

    impl Respond for SeqResponder {
        fn respond(&self, _: &wiremock::Request) -> ResponseTemplate {
            let call_num = self.num_calls.fetch_add(1, Ordering::SeqCst);
            match self.responses.get(call_num) {
                Some(body) => raw_sse_response(body.clone()),
                None => panic!("no response for {call_num}"),
            }
        }
    }

    let num_calls = bodies.len();
    let responder = SeqResponder {
        num_calls: AtomicUsize::new(0),
        responses: bodies,
    };

    let response_mock = ResponseMock::new();
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .and(response_mock.clone())
        .respond_with(responder)
        .up_to_n_times(num_calls as u64)
        .expect(num_calls as u64)
        .mount(server)
        .await;
    response_mock
}

/// Mounts a sequence of response templates for each POST to `/v1/responses`.
pub async fn mount_responses_sequence(
    server: &MockServer,
    responses: Vec<ResponseTemplate>,
) -> ResponseMock {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    struct SeqResponder {
        num_calls: AtomicUsize,
        responses: Vec<ResponseTemplate>,
    }

    impl Respond for SeqResponder {
        fn respond(&self, _: &wiremock::Request) -> ResponseTemplate {
            let call_num = self.num_calls.fetch_add(1, Ordering::SeqCst);
            self.responses
                .get(call_num)
                .unwrap_or_else(|| panic!("no response for {call_num}"))
                .clone()
        }
    }

    let num_calls = responses.len();
    let responder = SeqResponder {
        num_calls: AtomicUsize::new(0),
        responses,
    };

    let response_mock = ResponseMock::new();
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .and(response_mock.clone())
        .respond_with(responder)
        .up_to_n_times(num_calls as u64)
        .expect(num_calls as u64)
        .mount(server)
        .await;
    response_mock
}

pub fn raw_sse_response(body: String) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_raw(body, "text/event-stream")
}

pub async fn mount_models_once(server: &MockServer, body: ModelsResponse) -> ModelsMock {
    let (mock, models_mock) = models_mock();
    mock.respond_with(
        ResponseTemplate::new(200)
            .insert_header("content-type", "application/json")
            .set_body_json(body.clone()),
    )
    .up_to_n_times(1)
    .mount(server)
    .await;
    models_mock
}

pub async fn mount_models_once_with_delay(
    server: &MockServer,
    body: ModelsResponse,
    delay: Duration,
) -> ModelsMock {
    let (mock, models_mock) = models_mock();
    mock.respond_with(
        ResponseTemplate::new(200)
            .insert_header("content-type", "application/json")
            .set_body_json(body.clone())
            .set_delay(delay),
    )
    .up_to_n_times(1)
    .mount(server)
    .await;
    models_mock
}

pub async fn mount_models_once_with_etag(
    server: &MockServer,
    body: ModelsResponse,
    etag: &str,
) -> ModelsMock {
    let (mock, models_mock) = models_mock();
    mock.respond_with(
        ResponseTemplate::new(200)
            .insert_header("content-type", "application/json")
            // ModelsClient reads the ETag header, not a JSON field.
            .insert_header("ETag", etag)
            .set_body_json(body.clone()),
    )
    .up_to_n_times(1)
    .mount(server)
    .await;
    models_mock
}

pub async fn start_mock_server() -> MockServer {
    let server = MockServer::builder()
        .body_print_limit(BodyPrintLimit::Limited(80_000))
        .start()
        .await;

    // Provide a default `/models` response so tests remain hermetic when the client queries it.
    let _ = mount_models_once(&server, crate::test_codex_exec::exec_test_model_catalog()).await;

    server
}

/// Starts a lightweight WebSocket server for realtime-style streaming tests.
///
/// Each connection consumes a queue of request/event sequences. For each
/// request message, the server records the payload and streams the matching
/// events as WebSocket text frames before moving to the next request.
pub async fn start_websocket_server(connections: Vec<Vec<Vec<Value>>>) -> WebSocketTestServer {
    let connections = connections
        .into_iter()
        .map(|requests| WebSocketConnectionConfig {
            requests,
            response_headers: Vec::new(),
            accept_delay: None,
            close_after_requests: true,
        })
        .collect();
    start_websocket_server_with_headers(connections).await
}

pub async fn start_websocket_server_with_headers(
    connections: Vec<WebSocketConnectionConfig>,
) -> WebSocketTestServer {
    let start = std::time::Instant::now();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind websocket server");
    let addr = listener.local_addr().expect("websocket server address");
    let uri = format!("ws://{addr}");
    let connections_log = Arc::new(Mutex::new(Vec::new()));
    let handshakes_log = Arc::new(Mutex::new(Vec::new()));
    let request_log_updated = Arc::new(Notify::new());
    let requests = Arc::clone(&connections_log);
    let handshakes = Arc::clone(&handshakes_log);
    let request_log = Arc::clone(&request_log_updated);
    let preflight_connection_available = connections
        .first()
        .is_some_and(|connection| connection.requests.is_empty());
    let connections = Arc::new(Mutex::new(VecDeque::from(connections)));
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    let task = tokio::spawn(async move {
        loop {
            let accept_res = tokio::select! {
                _ = &mut shutdown_rx => return,
                accept_res = listener.accept() => accept_res,
            };
            let (stream, _) = match accept_res {
                Ok(value) => value,
                Err(_) => return,
            };
            let connection = {
                let mut pending = connections.lock().unwrap();
                pending.pop_front()
            };

            let Some(connection) = connection else {
                continue;
            };

            if let Some(delay) = connection.accept_delay {
                tokio::time::sleep(delay).await;
            }

            let response_headers = connection.response_headers.clone();
            let handshake_log = Arc::clone(&handshakes);
            let callback = move |req: &Request, mut response: Response| {
                let headers = req
                    .headers()
                    .iter()
                    .filter_map(|(name, value)| {
                        value
                            .to_str()
                            .ok()
                            .map(|value| (name.as_str().to_string(), value.to_string()))
                    })
                    .collect();
                handshake_log.lock().unwrap().push(WebSocketHandshake {
                    uri: req.uri().to_string(),
                    headers,
                });

                let headers_mut = response.headers_mut();
                for (name, value) in &response_headers {
                    if let (Ok(name), Ok(value)) = (
                        HeaderName::from_bytes(name.as_bytes()),
                        HeaderValue::from_str(value),
                    ) {
                        headers_mut.insert(name, value);
                    }
                }

                Ok(response)
            };

            let mut ws_stream = match accept_hdr_async_with_config(
                stream,
                callback,
                Some(websocket_accept_config()),
            )
            .await
            {
                Ok(ws) => ws,
                Err(_) => continue,
            };

            let connection_index = {
                let mut log = requests.lock().unwrap();
                log.push(Vec::new());
                log.len() - 1
            };
            let close_after_requests = connection.close_after_requests;
            for request_events in connection.requests {
                let Some(Ok(message)) = ws_stream.next().await else {
                    break;
                };
                if let Some(body) = parse_ws_request_body(message) {
                    let mut log = requests.lock().unwrap();
                    if let Some(connection_log) = log.get_mut(connection_index) {
                        connection_log.push(WebSocketRequest { body });
                        let request_index = connection_log.len() - 1;
                        let request = &connection_log[request_index];
                        let request_body = request.body_json();
                        eprintln!(
                            "[ws test server +{}ms] connection={} received request={} type={:?} role={:?} text={:?} data={:?}",
                            start.elapsed().as_millis(),
                            connection_index,
                            request_index,
                            request_body.get("type").and_then(Value::as_str),
                            request_body
                                .get("item")
                                .and_then(|item| item.get("role"))
                                .and_then(Value::as_str),
                            request_body
                                .get("item")
                                .and_then(|item| item.get("content"))
                                .and_then(Value::as_array)
                                .and_then(|content| content.first())
                                .and_then(|content| content.get("text"))
                                .and_then(Value::as_str),
                            request_body
                                .get("item")
                                .and_then(|item| item.get("content"))
                                .and_then(Value::as_array)
                                .and_then(|content| content.first())
                                .and_then(|content| content.get("data"))
                                .and_then(Value::as_str),
                        );
                    }
                    request_log.notify_waiters();
                }

                eprintln!(
                    "[ws test server +{}ms] connection={} sending batch_size={} event_types={:?} audio_data={:?}",
                    start.elapsed().as_millis(),
                    connection_index,
                    request_events.len(),
                    request_events
                        .iter()
                        .map(|event| event.get("type").and_then(Value::as_str))
                        .collect::<Vec<_>>(),
                    request_events
                        .iter()
                        .find_map(|event| event.get("delta").and_then(Value::as_str)),
                );
                for event in &request_events {
                    let Ok(payload) = serde_json::to_string(event) else {
                        continue;
                    };
                    if ws_stream.send(Message::Text(payload.into())).await.is_err() {
                        break;
                    }
                }
            }

            if close_after_requests {
                let _ = ws_stream.close(None).await;
            } else {
                let _ = shutdown_rx.await;
                return;
            }

            if connections.lock().unwrap().is_empty() {
                return;
            }
        }
    });

    WebSocketTestServer {
        uri,
        connections: connections_log,
        handshakes: handshakes_log,
        request_log_updated,
        preflight_connection_available,
        shutdown: shutdown_tx,
        task,
    }
}

fn parse_ws_request_body(message: Message) -> Option<Value> {
    match message {
        Message::Text(text) => serde_json::from_str(&text).ok(),
        Message::Binary(bytes) => serde_json::from_slice(&bytes).ok(),
        _ => None,
    }
}

fn websocket_accept_config() -> WebSocketConfig {
    let mut extensions = ExtensionsConfig::default();
    extensions.permessage_deflate = Some(DeflateConfig::default());

    let mut config = WebSocketConfig::default();
    config.extensions = extensions;
    config
}

#[derive(Clone)]
pub struct FunctionCallResponseMocks {
    pub function_call: ResponseMock,
    pub completion: ResponseMock,
}

pub async fn mount_function_call_agent_response(
    server: &MockServer,
    call_id: &str,
    arguments: &str,
    tool_name: &str,
) -> FunctionCallResponseMocks {
    let first_response = sse(vec![
        ev_response_created("resp-1"),
        ev_function_call(call_id, tool_name, arguments),
        ev_completed("resp-1"),
    ]);
    let function_call = mount_sse_once(server, first_response).await;

    let second_response = sse(vec![
        ev_assistant_message("msg-1", "done"),
        ev_completed("resp-2"),
    ]);
    let completion = mount_sse_once(server, second_response).await;

    FunctionCallResponseMocks {
        function_call,
        completion,
    }
}

/// Mounts a sequence of legacy SSE response bodies and serves them in order for each
/// POST to `/v1/chat/completions`. Panics if more requests are received than bodies
/// provided. Also asserts the exact number of expected calls.
pub async fn mount_sse_sequence(server: &MockServer, bodies: Vec<String>) -> ResponseMock {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    struct SeqResponder {
        num_calls: AtomicUsize,
        responses: Vec<String>,
    }

    impl Respond for SeqResponder {
        fn respond(&self, _: &wiremock::Request) -> ResponseTemplate {
            let call_num = self.num_calls.fetch_add(1, Ordering::SeqCst);
            match self.responses.get(call_num) {
                Some(body) => sse_response(body.clone()),
                None => panic!("no response for {call_num}"),
            }
        }
    }

    let num_calls = bodies.len();
    let responder = SeqResponder {
        num_calls: AtomicUsize::new(0),
        responses: bodies,
    };

    let (mock, response_mock) = base_mock();
    mock.respond_with(responder)
        .up_to_n_times(num_calls as u64)
        .expect(num_calls as u64)
        .mount(server)
        .await;

    response_mock
}

/// POST to `/v1/chat/completions`. Panics if more requests are received than
/// bodies provided. Also asserts the exact number of expected calls.
pub async fn mount_chat_completions_sse_sequence(
    server: &MockServer,
    bodies: Vec<String>,
) -> ResponseMock {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

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
                None => panic!("no response for {call_num}"),
            }
        }
    }

    let num_calls = bodies.len();
    let responder = SeqResponder {
        num_calls: AtomicUsize::new(0),
        responses: bodies,
    };

    let (mock, response_mock) = chat_completions_mock();
    mock.respond_with(responder)
        .up_to_n_times(num_calls as u64)
        .expect(num_calls as u64)
        .mount(server)
        .await;

    response_mock
}

/// Mounts a sequence of responses for each POST to `/v1/chat/completions`.
/// Panics if more requests are received than responses provided.
pub async fn mount_response_sequence(
    server: &MockServer,
    responses: Vec<ResponseTemplate>,
) -> ResponseMock {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    struct SeqResponder {
        num_calls: AtomicUsize,
        responses: Vec<ResponseTemplate>,
    }

    impl Respond for SeqResponder {
        fn respond(&self, _: &wiremock::Request) -> ResponseTemplate {
            let call_num = self.num_calls.fetch_add(1, Ordering::SeqCst);
            self.responses
                .get(call_num)
                .unwrap_or_else(|| panic!("no response for {call_num}"))
                .clone()
        }
    }

    let num_calls = responses.len();
    let responder = SeqResponder {
        num_calls: AtomicUsize::new(0),
        responses,
    };

    let (mock, response_mock) = base_mock();
    mock.respond_with(responder)
        .up_to_n_times(num_calls as u64)
        .expect(num_calls as u64)
        .mount(server)
        .await;
    response_mock
}

/// Validate invariants on the request body sent to `/v1/chat/completions`.
///
/// For Chat Completions bodies, every `tool` message must match a prior
/// `assistant.tool_calls[].id`, and every tool call in the request history must
/// have a matching `tool` output.
///
/// For legacy fixture bodies that still use Responses-shaped `input`, keep the
/// older call/output symmetry checks.
fn validate_request_body_invariants(request: &wiremock::Request) {
    // Skip GET requests (e.g., /models)
    if request.method != "POST" || !request.url.path().ends_with("/chat/completions") {
        return;
    }
    let body_bytes = decode_body_bytes(
        &request.body,
        request
            .headers
            .get("content-encoding")
            .and_then(|value| value.to_str().ok()),
    );
    let Ok(body): Result<Value, _> = serde_json::from_slice(&body_bytes) else {
        return;
    };

    use std::collections::HashSet;

    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        let mut tool_calls = HashSet::new();
        let mut tool_outputs = HashSet::new();
        for message in messages {
            if let Some(message_tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
                for tool_call in message_tool_calls {
                    let Some(id) = tool_call
                        .get("id")
                        .and_then(Value::as_str)
                        .filter(|id| !id.is_empty())
                    else {
                        panic!("assistant tool call with empty id should be dropped");
                    };
                    tool_calls.insert(id.to_string());
                }
            }

            if message.get("role").and_then(Value::as_str) == Some("tool") {
                let Some(id) = message
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                else {
                    panic!("tool output with empty tool_call_id should be dropped");
                };
                assert!(
                    tool_calls.contains(id),
                    "tool output without matching assistant tool call: {id}",
                );
                tool_outputs.insert(id.to_string());
            }
        }

        let missing_outputs = tool_calls
            .difference(&tool_outputs)
            .cloned()
            .collect::<HashSet<_>>();
        assert!(
            missing_outputs.is_empty(),
            "Tool call output is missing for call ids: {missing_outputs:?}",
        );
        return;
    }

    let Some(items) = body.get("input").and_then(Value::as_array) else {
        panic!("messages array not found in request");
    };

    fn get_call_id(item: &Value) -> Option<&str> {
        item.get("call_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
    }

    fn gather_ids(items: &[Value], kind: &str) -> HashSet<String> {
        items
            .iter()
            .filter(|item| item.get("type").and_then(Value::as_str) == Some(kind))
            .filter_map(get_call_id)
            .map(str::to_string)
            .collect()
    }

    fn gather_output_ids(items: &[Value], kind: &str, missing_msg: &str) -> HashSet<String> {
        items
            .iter()
            .filter(|item| item.get("type").and_then(Value::as_str) == Some(kind))
            .map(|item| {
                let Some(id) = get_call_id(item) else {
                    panic!("{missing_msg}");
                };
                id.to_string()
            })
            .collect()
    }

    fn gather_tool_search_output_ids(items: &[Value]) -> HashSet<String> {
        items
            .iter()
            .filter(|item| item.get("type").and_then(Value::as_str) == Some("tool_search_output"))
            .filter_map(|item| {
                if let Some(id) = get_call_id(item) {
                    return Some(id.to_string());
                }
                if item.get("execution").and_then(Value::as_str) == Some("server") {
                    return None;
                }
                panic!("orphan tool_search_output with empty call_id should be dropped");
            })
            .collect()
    }

    let function_calls = gather_ids(items, "function_call");
    let tool_search_calls = gather_ids(items, "tool_search_call");
    let custom_tool_calls = gather_ids(items, "custom_tool_call");
    let local_shell_calls = gather_ids(items, "local_shell_call");
    let function_call_outputs = gather_output_ids(
        items,
        "function_call_output",
        "orphan function_call_output with empty call_id should be dropped",
    );
    let tool_search_outputs = gather_tool_search_output_ids(items);
    let custom_tool_call_outputs = gather_output_ids(
        items,
        "custom_tool_call_output",
        "orphan custom_tool_call_output with empty call_id should be dropped",
    );

    for cid in &function_call_outputs {
        assert!(
            function_calls.contains(cid) || local_shell_calls.contains(cid),
            "function_call_output without matching call in input: {cid}",
        );
    }
    for cid in &custom_tool_call_outputs {
        assert!(
            custom_tool_calls.contains(cid),
            "custom_tool_call_output without matching call in input: {cid}",
        );
    }
    for cid in &tool_search_outputs {
        assert!(
            tool_search_calls.contains(cid),
            "tool_search_output without matching call in input: {cid}",
        );
    }

    for cid in &function_calls {
        assert!(
            function_call_outputs.contains(cid),
            "Function call output is missing for call id: {cid}",
        );
    }
    for cid in &custom_tool_calls {
        assert!(
            custom_tool_call_outputs.contains(cid),
            "Custom tool call output is missing for call id: {cid}",
        );
    }
    for cid in &tool_search_calls {
        assert!(
            tool_search_outputs.contains(cid),
            "Tool search output is missing for call id: {cid}",
        );
    }
}
