use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::TranscriptItem;
use codex_protocol::openai_models::InputModality;
use codex_tools::TOOL_SEARCH_TOOL_NAME;
use codex_utils_image::PromptImageMode;
use codex_utils_image::load_for_prompt_bytes;
use std::collections::HashSet;
use std::path::Path;

use crate::util::error_or_panic;
use tracing::info;

const IMAGE_CONTENT_OMITTED_PLACEHOLDER: &str =
    "image content omitted because you do not support image input";
const INVALID_IMAGE_CONTENT_PLACEHOLDER: &str = "invalid image content omitted";

pub(crate) fn ensure_call_outputs_present(items: &mut Vec<TranscriptItem>) {
    // Collect synthetic outputs to insert immediately after their calls.
    // Store the insertion position (index of call) alongside the item so
    // we can insert in reverse order and avoid index shifting.
    let mut missing_outputs_to_insert: Vec<(usize, TranscriptItem)> = Vec::new();

    for (idx, item) in items.iter().enumerate() {
        match item {
            TranscriptItem::FunctionCall {
                call_id,
                name,
                namespace,
                ..
            } => {
                let has_output = if namespace.is_none() && name == TOOL_SEARCH_TOOL_NAME {
                    items.iter().any(|i| match i {
                        TranscriptItem::ToolSearchOutput {
                            call_id: Some(existing),
                            ..
                        } => existing == call_id,
                        _ => false,
                    })
                } else {
                    items.iter().any(|i| match i {
                        TranscriptItem::FunctionCallOutput {
                            call_id: existing, ..
                        } => existing == call_id,
                        _ => false,
                    })
                };

                if !has_output {
                    info!("Function call output is missing for call id: {call_id}");
                    missing_outputs_to_insert.push((
                        idx,
                        TranscriptItem::FunctionCallOutput {
                            call_id: call_id.clone(),
                            output: FunctionCallOutputPayload::from_text("aborted".to_string()),
                        },
                    ));
                }
            }
            TranscriptItem::ToolSearchCall {
                call_id: Some(call_id),
                ..
            } => {
                let has_output = items.iter().any(|i| match i {
                    TranscriptItem::ToolSearchOutput {
                        call_id: Some(existing),
                        ..
                    } => existing == call_id,
                    _ => false,
                });

                if !has_output {
                    info!("Tool search output is missing for call id: {call_id}");
                    missing_outputs_to_insert.push((
                        idx,
                        TranscriptItem::ToolSearchOutput {
                            call_id: Some(call_id.clone()),
                            status: "completed".to_string(),
                            execution: "client".to_string(),
                            tools: Vec::new(),
                        },
                    ));
                }
            }
            TranscriptItem::CustomToolCall { call_id, .. } => {
                let has_output = items.iter().any(|i| match i {
                    TranscriptItem::CustomToolCallOutput {
                        call_id: existing, ..
                    } => existing == call_id,
                    _ => false,
                });

                if !has_output {
                    error_or_panic(format!(
                        "Custom tool call output is missing for call id: {call_id}"
                    ));
                    missing_outputs_to_insert.push((
                        idx,
                        TranscriptItem::CustomToolCallOutput {
                            call_id: call_id.clone(),
                            name: None,
                            output: FunctionCallOutputPayload::from_text("aborted".to_string()),
                        },
                    ));
                }
            }
            // LocalShellCall is represented in upstream streams by a FunctionCallOutput
            TranscriptItem::LocalShellCall { call_id, .. } => {
                if let Some(call_id) = call_id.as_ref() {
                    let has_output = items.iter().any(|i| match i {
                        TranscriptItem::FunctionCallOutput {
                            call_id: existing, ..
                        } => existing == call_id,
                        _ => false,
                    });

                    if !has_output {
                        error_or_panic(format!(
                            "Local shell call output is missing for call id: {call_id}"
                        ));
                        missing_outputs_to_insert.push((
                            idx,
                            TranscriptItem::FunctionCallOutput {
                                call_id: call_id.clone(),
                                output: FunctionCallOutputPayload::from_text("aborted".to_string()),
                            },
                        ));
                    }
                }
            }
            _ => {}
        }
    }

    // Insert synthetic outputs in reverse index order to avoid re-indexing.
    for (idx, output_item) in missing_outputs_to_insert.into_iter().rev() {
        items.insert(idx + 1, output_item);
    }
}

pub(crate) fn remove_orphan_outputs(items: &mut Vec<TranscriptItem>) {
    let function_call_ids: HashSet<String> = items
        .iter()
        .filter_map(|i| match i {
            TranscriptItem::FunctionCall { call_id, .. } => Some(call_id.clone()),
            _ => None,
        })
        .collect();

    let tool_search_call_ids: HashSet<String> = items
        .iter()
        .filter_map(|i| match i {
            TranscriptItem::ToolSearchCall {
                call_id: Some(call_id),
                ..
            } => Some(call_id.clone()),
            TranscriptItem::FunctionCall {
                call_id,
                name,
                namespace: None,
                ..
            } if name == TOOL_SEARCH_TOOL_NAME => Some(call_id.clone()),
            _ => None,
        })
        .collect();

    let local_shell_call_ids: HashSet<String> = items
        .iter()
        .filter_map(|i| match i {
            TranscriptItem::LocalShellCall {
                call_id: Some(call_id),
                ..
            } => Some(call_id.clone()),
            _ => None,
        })
        .collect();

    let custom_tool_call_ids: HashSet<String> = items
        .iter()
        .filter_map(|i| match i {
            TranscriptItem::CustomToolCall { call_id, .. } => Some(call_id.clone()),
            _ => None,
        })
        .collect();

    items.retain(|item| match item {
        TranscriptItem::FunctionCallOutput { call_id, .. } => {
            let has_match =
                function_call_ids.contains(call_id) || local_shell_call_ids.contains(call_id);
            if !has_match {
                error_or_panic(format!(
                    "Orphan function call output for call id: {call_id}"
                ));
            }
            has_match
        }
        TranscriptItem::CustomToolCallOutput { call_id, .. } => {
            let has_match = custom_tool_call_ids.contains(call_id);
            if !has_match {
                error_or_panic(format!(
                    "Orphan custom tool call output for call id: {call_id}"
                ));
            }
            has_match
        }
        TranscriptItem::ToolSearchOutput { execution, .. } if execution == "server" => true,
        TranscriptItem::ToolSearchOutput {
            call_id: Some(call_id),
            ..
        } => {
            let has_match = tool_search_call_ids.contains(call_id);
            if !has_match {
                error_or_panic(format!("Orphan tool search output for call id: {call_id}"));
            }
            has_match
        }
        TranscriptItem::ToolSearchOutput { call_id: None, .. } => true,
        _ => true,
    });
}

pub(crate) fn remove_corresponding_for(items: &mut Vec<TranscriptItem>, item: &TranscriptItem) {
    match item {
        TranscriptItem::FunctionCall { call_id, .. } => {
            remove_first_matching(items, |i| {
                matches!(
                    i,
                    TranscriptItem::FunctionCallOutput {
                        call_id: existing, ..
                    } if existing == call_id
                )
            });
        }
        TranscriptItem::FunctionCallOutput { call_id, .. } => {
            if let Some(pos) = items.iter().position(|i| {
                matches!(i, TranscriptItem::FunctionCall { call_id: existing, .. } if existing == call_id)
            }) {
                items.remove(pos);
            } else if let Some(pos) = items.iter().position(|i| {
                matches!(i, TranscriptItem::LocalShellCall { call_id: Some(existing), .. } if existing == call_id)
            }) {
                items.remove(pos);
            }
        }
        TranscriptItem::ToolSearchCall {
            call_id: Some(call_id),
            ..
        } => {
            remove_first_matching(items, |i| {
                matches!(
                    i,
                    TranscriptItem::ToolSearchOutput {
                        call_id: Some(existing),
                        ..
                    } if existing == call_id
                )
            });
        }
        TranscriptItem::ToolSearchOutput {
            call_id: Some(call_id),
            ..
        } => {
            remove_first_matching(
                items,
                |i| {
                    matches!(
                        i,
                        TranscriptItem::ToolSearchCall {
                            call_id: Some(existing),
                            ..
                        } if existing == call_id
                    ) || matches!(
                        i,
                        TranscriptItem::FunctionCall {
                            call_id: existing,
                            name,
                            namespace: None,
                            ..
                        } if existing == call_id && name == TOOL_SEARCH_TOOL_NAME
                    )
                },
            );
        }
        TranscriptItem::CustomToolCall { call_id, .. } => {
            remove_first_matching(items, |i| {
                matches!(
                    i,
                    TranscriptItem::CustomToolCallOutput {
                        call_id: existing, ..
                    } if existing == call_id
                )
            });
        }
        TranscriptItem::CustomToolCallOutput { call_id, .. } => {
            remove_first_matching(
                items,
                |i| matches!(i, TranscriptItem::CustomToolCall { call_id: existing, .. } if existing == call_id),
            );
        }
        TranscriptItem::LocalShellCall {
            call_id: Some(call_id),
            ..
        } => {
            remove_first_matching(items, |i| {
                matches!(
                    i,
                    TranscriptItem::FunctionCallOutput {
                        call_id: existing, ..
                    } if existing == call_id
                )
            });
        }
        _ => {}
    }
}

fn remove_first_matching<F>(items: &mut Vec<TranscriptItem>, predicate: F)
where
    F: Fn(&TranscriptItem) -> bool,
{
    if let Some(pos) = items.iter().position(predicate) {
        items.remove(pos);
    }
}

/// Strip image content from messages and tool outputs when the model does not support images.
/// When `input_modalities` contains `InputModality::Image`, no stripping is performed.
pub(crate) fn strip_images_when_unsupported(
    input_modalities: &[InputModality],
    items: &mut [TranscriptItem],
) {
    let supports_images = input_modalities.contains(&InputModality::Image);
    if supports_images {
        return;
    }

    replace_images_with_placeholder(items, IMAGE_CONTENT_OMITTED_PLACEHOLDER);
}

/// Validate model-visible image inputs and canonicalize inline image MIME types from their
/// decoded bytes. Remote HTTP(S) images are left for the provider to fetch. Invalid or
/// provider-inaccessible image inputs become a compact text placeholder.
pub(crate) fn normalize_inline_images(items: &mut [TranscriptItem]) {
    for item in items.iter_mut() {
        match item {
            TranscriptItem::Message { content, .. } => {
                for content_item in content.iter_mut() {
                    let ContentItem::InputImage { image_url, .. } = content_item else {
                        continue;
                    };
                    match normalize_inline_image_url(image_url) {
                        InlineImageNormalization::Unchanged => {}
                        InlineImageNormalization::Normalized(normalized) => {
                            *image_url = normalized;
                        }
                        InlineImageNormalization::Invalid => {
                            *content_item = ContentItem::InputText {
                                text: INVALID_IMAGE_CONTENT_PLACEHOLDER.to_string(),
                            };
                        }
                    }
                }
            }
            TranscriptItem::FunctionCallOutput { output, .. }
            | TranscriptItem::CustomToolCallOutput { output, .. } => {
                let Some(content_items) = output.content_items_mut() else {
                    continue;
                };
                for content_item in content_items.iter_mut() {
                    let FunctionCallOutputContentItem::InputImage { image_url, .. } = content_item
                    else {
                        continue;
                    };
                    match normalize_inline_image_url(image_url) {
                        InlineImageNormalization::Unchanged => {}
                        InlineImageNormalization::Normalized(normalized) => {
                            *image_url = normalized;
                        }
                        InlineImageNormalization::Invalid => {
                            *content_item = FunctionCallOutputContentItem::InputText {
                                text: INVALID_IMAGE_CONTENT_PLACEHOLDER.to_string(),
                            };
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Replace every image currently visible to the model. Returns the number of replaced images.
pub(crate) fn replace_images_with_placeholder(
    items: &mut [TranscriptItem],
    placeholder: &str,
) -> usize {
    let mut replaced = 0;

    for item in items.iter_mut() {
        match item {
            TranscriptItem::Message { content, .. } => {
                for content_item in content.iter_mut() {
                    if matches!(content_item, ContentItem::InputImage { .. }) {
                        *content_item = ContentItem::InputText {
                            text: placeholder.to_string(),
                        };
                        replaced += 1;
                    }
                }
            }
            TranscriptItem::FunctionCallOutput { output, .. }
            | TranscriptItem::CustomToolCallOutput { output, .. } => {
                if let Some(content_items) = output.content_items_mut() {
                    for content_item in content_items.iter_mut() {
                        if matches!(
                            content_item,
                            FunctionCallOutputContentItem::InputImage { .. }
                        ) {
                            *content_item = FunctionCallOutputContentItem::InputText {
                                text: placeholder.to_string(),
                            };
                            replaced += 1;
                        }
                    }
                }
            }
            TranscriptItem::ImageGenerationCall { result, .. } => {
                if !result.is_empty() {
                    replaced += 1;
                }
                result.clear();
            }
            _ => {}
        }
    }

    replaced
}

enum InlineImageNormalization {
    Unchanged,
    Normalized(String),
    Invalid,
}

fn normalize_inline_image_url(image_url: &str) -> InlineImageNormalization {
    let is_http = image_url
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"));
    let is_https = image_url
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"));
    if is_http || is_https {
        return InlineImageNormalization::Unchanged;
    }

    let Some(prefix) = image_url.get(..5) else {
        return InlineImageNormalization::Invalid;
    };
    if !prefix.eq_ignore_ascii_case("data:") {
        return InlineImageNormalization::Invalid;
    }

    let Some((header, encoded)) = image_url.split_once(',') else {
        return InlineImageNormalization::Invalid;
    };
    let is_base64 = header
        .split(';')
        .skip(1)
        .any(|parameter| parameter.eq_ignore_ascii_case("base64"));
    if !is_base64 {
        return InlineImageNormalization::Invalid;
    }

    let Ok(bytes) = BASE64_STANDARD.decode(encoded) else {
        return InlineImageNormalization::Invalid;
    };
    let Ok(image) = load_for_prompt_bytes(
        Path::new("<inline-image>"),
        bytes,
        PromptImageMode::Original,
    ) else {
        return InlineImageNormalization::Invalid;
    };

    InlineImageNormalization::Normalized(image.into_data_url())
}
