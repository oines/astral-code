use std::collections::BTreeSet;

use codex_api::agent_protocol::AgentMessage;
use codex_api::agent_protocol::AgentRequest;
use codex_api::agent_protocol::ContentBlock;
use codex_api::agent_protocol::MessageRole;
use codex_api::agent_protocol::RequestMetadata;
use codex_api::agent_protocol::ToolResultContent;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::AnthropicCacheFoldState;

#[test]
fn cached_fold_keeps_recent_five_eligible_tool_results() {
    let mut state = AnthropicCacheFoldState::default();
    let request = request_with_tool_results("Read", 6, Some("astral:test"));

    let options = state
        .options_for_request(&request)
        .expect("fold options should be enabled");

    assert_eq!(
        options.cache_reference_tool_use_ids,
        (1..=6)
            .map(|index| format!("toolu_{index}"))
            .collect::<BTreeSet<_>>()
    );
    assert_eq!(options.pinned_cache_edits.len(), 1);
    assert_eq!(options.pinned_cache_edits[0].user_message_index, 11);
    assert_eq!(
        options.pinned_cache_edits[0].cache_references,
        vec!["toolu_1"]
    );

    let options = state
        .options_for_request(&request)
        .expect("pinned edits should replay");
    assert_eq!(options.pinned_cache_edits.len(), 1);
    assert_eq!(
        options.pinned_cache_edits[0].cache_references,
        vec!["toolu_1"]
    );
}

#[test]
fn cached_fold_requires_prompt_cache_key() {
    let mut state = AnthropicCacheFoldState::default();
    let request = request_with_tool_results("Read", 6, None);

    assert_eq!(state.options_for_request(&request), None);
}

#[test]
fn cached_fold_does_not_fold_arbitrary_mcp_tools() {
    let mut state = AnthropicCacheFoldState::default();
    let request = request_with_tool_results("mcp__filesystem__read_file", 6, Some("astral:test"));

    assert_eq!(state.options_for_request(&request), None);
}

#[test]
fn cached_fold_disable_stops_later_options() {
    let mut state = AnthropicCacheFoldState::default();
    state.disable();
    let request = request_with_tool_results("Read", 6, Some("astral:test"));

    assert_eq!(state.options_for_request(&request), None);
}

fn request_with_tool_results(
    tool_name: &str,
    count: usize,
    prompt_cache_key: Option<&str>,
) -> AgentRequest {
    AgentRequest {
        model: "astral-large".to_string(),
        messages: (1..=count)
            .flat_map(|index| {
                [
                    AgentMessage {
                        role: MessageRole::Assistant,
                        content: vec![ContentBlock::ToolUse {
                            id: format!("toolu_{index}"),
                            name: tool_name.to_string(),
                            input: json!({}),
                        }],
                        id: None,
                    },
                    AgentMessage {
                        role: MessageRole::User,
                        content: vec![ContentBlock::ToolResult {
                            tool_use_id: format!("toolu_{index}"),
                            content: vec![ToolResultContent::Text {
                                text: format!("result {index}"),
                            }],
                            is_error: false,
                        }],
                        id: None,
                    },
                ]
            })
            .collect(),
        metadata: RequestMetadata {
            prompt_cache_key: prompt_cache_key.map(str::to_string),
            ..RequestMetadata::default()
        },
        ..AgentRequest::default()
    }
}
