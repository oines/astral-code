use std::collections::BTreeMap;
use std::collections::BTreeSet;

use codex_api::agent_adapters::anthropic::AnthropicCacheFoldOptions;
use codex_api::agent_adapters::anthropic::AnthropicPinnedCacheEdits;
use codex_api::agent_protocol::AgentRequest;
use codex_api::agent_protocol::ContentBlock;
use codex_api::agent_protocol::MessageRole;
use codex_tools::BASH_TOOL_NAME;
use codex_tools::EDIT_TOOL_NAME;
use codex_tools::GLOB_TOOL_NAME;
use codex_tools::GREP_TOOL_NAME;
use codex_tools::READ_TOOL_NAME;
use codex_tools::WRITE_TOOL_NAME;

const RECENT_ELIGIBLE_TOOL_RESULTS_TO_KEEP: usize = 5;

#[derive(Debug, Default)]
pub(crate) struct AnthropicCacheFoldState {
    disabled: bool,
    registered_refs: BTreeSet<String>,
    deleted_refs: BTreeSet<String>,
    pinned_cache_edits: Vec<AnthropicPinnedCacheEdits>,
}

impl AnthropicCacheFoldState {
    pub(crate) fn options_for_request(
        &mut self,
        request: &AgentRequest,
    ) -> Option<AnthropicCacheFoldOptions> {
        if self.disabled || request.metadata.prompt_cache_key.is_none() {
            return None;
        }

        let eligible_refs = eligible_tool_result_refs(request);
        if eligible_refs.is_empty() {
            return None;
        }

        for tool_use_id in &eligible_refs {
            self.registered_refs.insert(tool_use_id.clone());
        }

        let keep_from = eligible_refs
            .len()
            .saturating_sub(RECENT_ELIGIBLE_TOOL_RESULTS_TO_KEEP);
        let mut new_delete_refs = Vec::new();
        for tool_use_id in eligible_refs.iter().take(keep_from) {
            if self.deleted_refs.insert(tool_use_id.clone()) {
                new_delete_refs.push(tool_use_id.clone());
            }
        }

        if !new_delete_refs.is_empty()
            && let Some(user_message_index) = last_projected_user_message_index(request)
        {
            self.pinned_cache_edits.push(AnthropicPinnedCacheEdits {
                user_message_index,
                cache_references: new_delete_refs,
            });
        }

        Some(AnthropicCacheFoldOptions {
            cache_reference_tool_use_ids: self.registered_refs.clone(),
            pinned_cache_edits: self.pinned_cache_edits.clone(),
        })
    }

    pub(crate) fn disable(&mut self) {
        self.disabled = true;
    }

    pub(crate) fn reset(&mut self) {
        self.registered_refs.clear();
        self.deleted_refs.clear();
        self.pinned_cache_edits.clear();
    }
}

fn last_projected_user_message_index(request: &AgentRequest) -> Option<usize> {
    let mut projected_index = None;
    let mut last_projected_role = None;
    for message in &request.messages {
        if matches!(message.role, MessageRole::System | MessageRole::Developer) {
            continue;
        }
        if last_projected_role != Some(&message.role) {
            projected_index = Some(projected_index.map_or(0, |index| index + 1));
            last_projected_role = Some(&message.role);
        }
    }

    request
        .messages
        .iter()
        .rev()
        .find(|message| !matches!(message.role, MessageRole::System | MessageRole::Developer))
        .and_then(|message| matches!(message.role, MessageRole::User).then_some(()))
        .and(projected_index)
}

fn eligible_tool_result_refs(request: &AgentRequest) -> Vec<String> {
    let mut tool_names_by_id = BTreeMap::new();
    for message in &request.messages {
        for block in &message.content {
            if let ContentBlock::ToolUse { id, name, .. } = block {
                tool_names_by_id.insert(id, name);
            }
        }
    }

    let mut refs = Vec::new();
    let mut seen = BTreeSet::new();
    for message in &request.messages {
        for block in &message.content {
            let ContentBlock::ToolResult { tool_use_id, .. } = block else {
                continue;
            };
            if !seen.insert(tool_use_id.clone()) {
                continue;
            }
            if tool_names_by_id
                .get(tool_use_id)
                .is_some_and(|name| is_cache_fold_eligible_tool_name(name))
            {
                refs.push(tool_use_id.clone());
            }
        }
    }
    refs
}

fn is_cache_fold_eligible_tool_name(name: &str) -> bool {
    matches!(
        name,
        BASH_TOOL_NAME
            | READ_TOOL_NAME
            | GREP_TOOL_NAME
            | GLOB_TOOL_NAME
            | EDIT_TOOL_NAME
            | WRITE_TOOL_NAME
            | "shell_command"
            | "exec_command"
            | "write_stdin"
    )
}

#[cfg(test)]
#[path = "anthropic_cache_fold_tests.rs"]
mod tests;
