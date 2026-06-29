use std::collections::HashSet;

use crate::Prompt;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::approx_token_count;
use codex_utils_output_truncation::truncate_text;

use super::SessionMemoryState;

pub(super) const DEFAULT_SUMMARY: &str = r#"# Session Title
_A short and distinctive 5-10 word descriptive title for the session. Super info dense, no filler_

# Current State
_What is actively being worked on right now? Pending tasks not yet completed. Immediate next steps._

# Task specification
_What did the user ask to build? Any design decisions or other explanatory context_

# Files and Functions
_What are the important files? In short, what do they contain and why are they relevant?_

# Workflow
_What bash commands are usually run and in what order? How to interpret their output if not obvious?_

# Errors & Corrections
_Errors encountered and how they were fixed. What did the user correct? What approaches failed and should not be tried again?_

# Codebase and System Documentation
_What are the important system components? How do they work/fit together?_

# Learnings
_What has worked well? What has not? What to avoid? Do not duplicate items from other sections_

# Key results
_If the user asked a specific output such as an answer to a question, a table, or other document, repeat the exact result here_

# Worklog
_Step by step, what was attempted, done? Very terse summary for each step_
"#;

const MAX_RAW_TAIL_TOKENS: usize = 40_000;
const MIN_RAW_TAIL_TOKENS: i64 = 10_000;
const MIN_RAW_TAIL_TEXT_ITEMS: usize = 5;
const MAX_SESSION_MEMORY_SECTION_TOKENS: usize = 2_000;
const MAX_SESSION_MEMORY_TOTAL_TOKENS: usize = 12_000;
const MIN_EXISTING_SUMMARY_TOKENS_FOR_COLLAPSE_GUARD: usize = 2_000;
const MIN_REWRITTEN_SUMMARY_TOKENS: usize = 500;
pub(super) const MAX_COMPACT_SUMMARY_BODY_TOKENS: usize = 9_500;
const REQUIRED_SUMMARY_HEADINGS: &[&str] = &[
    "# Current State",
    "# Task specification",
    "# Files and Functions",
    "# Workflow",
    "# Errors & Corrections",
    "# Codebase and System Documentation",
    "# Learnings",
    "# Key results",
    "# Worklog",
];

#[derive(Clone, Debug)]
pub(super) struct ExtractionBoundary {
    pub(super) index: usize,
    pub(super) fingerprint: String,
    pub(super) tokens: i64,
    pub(super) tool_calls: usize,
}

pub(super) fn extraction_boundary(input: &[ResponseItem]) -> Option<ExtractionBoundary> {
    let index = input.len().checked_sub(1)?;
    let item = input.get(index)?;
    Some(ExtractionBoundary {
        index,
        fingerprint: item_fingerprint(item),
        tokens: estimate_items_tokens(input),
        tool_calls: count_tool_calls(input),
    })
}

pub(super) fn raw_tail_after_summary_boundary(
    items: &[ResponseItem],
    state: &SessionMemoryState,
) -> CodexResult<Vec<ResponseItem>> {
    let last_compaction_index = items
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, item)| is_compaction_boundary(item).then_some(index));
    let floor = last_compaction_index.map_or(0, |index| index.saturating_add(1));
    let start = match state.last_summary_index {
        Some(boundary_index) => {
            let boundary = items
                .get(boundary_index)
                .ok_or_else(|| CodexErr::Fatal("session memory boundary not found".to_string()))?;
            let expected_fingerprint =
                state.last_summary_fingerprint.as_deref().ok_or_else(|| {
                    CodexErr::Fatal("session memory boundary fingerprint missing".to_string())
                })?;
            if item_fingerprint(boundary) != expected_fingerprint {
                return Err(CodexErr::Fatal(
                    "session memory boundary fingerprint mismatch".to_string(),
                ));
            }
            boundary_index.saturating_add(1)
        }
        None => items.len(),
    };
    let start = calculate_tail_start(items, start.max(floor), floor);
    let tail = items[start..].to_vec();
    validate_tail_pairs(&tail)?;
    Ok(tail)
}

pub(super) fn validate_summary(summary: &str) -> CodexResult<()> {
    let trimmed = summary.trim();
    if trimmed.is_empty() || trimmed == DEFAULT_SUMMARY.trim() {
        return Err(CodexErr::Fatal(
            "session memory summary is missing or still the template".to_string(),
        ));
    }
    for heading in REQUIRED_SUMMARY_HEADINGS {
        if !trimmed.lines().any(|line| line.trim() == *heading) {
            return Err(CodexErr::Fatal(format!(
                "session memory summary is missing required heading {heading}"
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_tail_budget(tail: &[ResponseItem]) -> CodexResult<()> {
    let tokens = estimate_items_tokens(tail);
    if usize::try_from(tokens).unwrap_or(usize::MAX) > MAX_RAW_TAIL_TOKENS {
        return Err(CodexErr::Fatal(format!(
            "session memory raw tail exceeds {MAX_RAW_TAIL_TOKENS} tokens"
        )));
    }
    Ok(())
}

pub(super) fn validate_post_compact_budget(
    items: &[ResponseItem],
    token_limit: i64,
) -> CodexResult<()> {
    let tokens = estimate_items_tokens(items);
    if tokens > token_limit {
        return Err(CodexErr::Fatal(format!(
            "session memory compacted history exceeds {token_limit} tokens"
        )));
    }
    Ok(())
}

pub(super) fn validate_post_extraction_summary(
    previous_summary: &str,
    updated_summary: &str,
) -> CodexResult<()> {
    validate_summary(updated_summary)?;

    let previous_tokens = approx_token_count(previous_summary);
    let updated_tokens = approx_token_count(updated_summary);
    if previous_summary.trim() != DEFAULT_SUMMARY.trim()
        && previous_tokens >= MIN_EXISTING_SUMMARY_TOKENS_FOR_COLLAPSE_GUARD
        && updated_tokens < MIN_REWRITTEN_SUMMARY_TOKENS
        && updated_tokens.saturating_mul(4) < previous_tokens
    {
        return Err(CodexErr::Fatal(
            "session memory extraction collapsed existing summary unexpectedly".to_string(),
        ));
    }

    Ok(())
}

pub(super) fn truncate_summary_for_compact(summary: &str) -> (String, bool) {
    let mut was_truncated = false;
    let mut output_lines = Vec::new();
    let mut current_header = String::new();
    let mut current_lines = Vec::new();

    for line in summary.lines() {
        if line.starts_with('#') {
            let section = flush_summary_section(&current_header, &current_lines);
            was_truncated |= section.was_truncated;
            output_lines.extend(section.lines);
            current_header = line.to_string();
            current_lines.clear();
        } else {
            current_lines.push(line.to_string());
        }
    }

    let section = flush_summary_section(&current_header, &current_lines);
    was_truncated |= section.was_truncated;
    output_lines.extend(section.lines);

    let section_bounded = output_lines.join("\n");
    if approx_token_count(&section_bounded) <= MAX_COMPACT_SUMMARY_BODY_TOKENS {
        return (section_bounded, was_truncated);
    }

    (
        truncate_text(
            &section_bounded,
            TruncationPolicy::Tokens(MAX_COMPACT_SUMMARY_BODY_TOKENS),
        ),
        true,
    )
}

pub(super) fn summary_budget_reminder(summary: &str) -> String {
    let total_tokens = approx_token_count(summary);
    let oversized_sections = summarize_oversized_sections(summary);
    if total_tokens <= MAX_SESSION_MEMORY_TOTAL_TOKENS && oversized_sections.is_empty() {
        return String::new();
    }

    let mut reminder = String::new();
    let over_budget = total_tokens > MAX_SESSION_MEMORY_TOTAL_TOKENS;
    if total_tokens > MAX_SESSION_MEMORY_TOTAL_TOKENS {
        reminder.push_str(&format!(
            "\n\nCRITICAL: The session memory file is currently ~{total_tokens} tokens, which exceeds the maximum of {MAX_SESSION_MEMORY_TOTAL_TOKENS} tokens. You MUST condense the file to fit within this budget. Aggressively shorten oversized sections by removing less important details, merging related items, and summarizing older entries. Prioritize keeping \"Current State\" and \"Errors & Corrections\" accurate and detailed."
        ));
    }
    if !oversized_sections.is_empty() {
        let heading = if over_budget {
            "Oversized sections to condense"
        } else {
            "IMPORTANT: The following sections exceed the per-section limit and MUST be condensed"
        };
        reminder.push_str(&format!(
            "\n\n{heading}:\n{}",
            oversized_sections.join("\n")
        ));
    }
    reminder
}

pub(super) fn format_session_memory_summary(
    summary: &str,
    was_truncated_for_compact: bool,
) -> String {
    let mut formatted = format!(
        "This session was summarized using the session memory file. Use it as durable context for the conversation so far.\n\n{}",
        summary.trim()
    );
    if was_truncated_for_compact {
        formatted.push_str("\n\nSome session memory sections were truncated for compact length. The full session memory file may contain additional detail.");
    }
    formatted.push_str(
        "\n\nRecent raw transcript messages after this session-memory summary follow below.",
    );
    formatted
}

fn calculate_tail_start(items: &[ResponseItem], requested_start: usize, floor: usize) -> usize {
    let mut start = requested_start.min(items.len()).max(floor.min(items.len()));
    let mut tokens = estimate_items_tokens(&items[start..]);
    let mut text_items = count_text_items(&items[start..]);

    while start > floor && (tokens < MIN_RAW_TAIL_TOKENS || text_items < MIN_RAW_TAIL_TEXT_ITEMS) {
        let candidate_start = start.saturating_sub(1);
        let candidate_tokens = estimate_items_tokens(&items[candidate_start..start]);
        if tokens > 0
            && usize::try_from(tokens.saturating_add(candidate_tokens)).unwrap_or(usize::MAX)
                > MAX_RAW_TAIL_TOKENS
        {
            break;
        }
        start = candidate_start;
        tokens = tokens.saturating_add(candidate_tokens);
        text_items = text_items.saturating_add(count_text_items(&items[start..start + 1]));
    }

    adjust_start_to_preserve_pairs(items, start, floor)
}

fn adjust_start_to_preserve_pairs(items: &[ResponseItem], start: usize, floor: usize) -> usize {
    let mut adjusted_start = start;
    let function_output_ids: HashSet<&str> = items[start..]
        .iter()
        .filter_map(|item| match item {
            ResponseItem::FunctionCallOutput { call_id, .. } => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    let custom_output_ids: HashSet<&str> = items[start..]
        .iter()
        .filter_map(|item| match item {
            ResponseItem::CustomToolCallOutput { call_id, .. } => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    let tool_search_output_ids: HashSet<&str> = items[start..]
        .iter()
        .filter_map(|item| match item {
            ResponseItem::ToolSearchOutput { call_id, .. } => call_id.as_deref(),
            _ => None,
        })
        .collect();

    for index in (floor..start).rev() {
        let item = &items[index];
        match item {
            ResponseItem::FunctionCall { call_id, .. }
            | ResponseItem::LocalShellCall {
                call_id: Some(call_id),
                ..
            } if function_output_ids.contains(call_id.as_str()) => {
                adjusted_start = index;
            }
            ResponseItem::CustomToolCall { call_id, .. }
                if custom_output_ids.contains(call_id.as_str()) =>
            {
                adjusted_start = index;
            }
            ResponseItem::ToolSearchCall {
                call_id: Some(call_id),
                ..
            } if tool_search_output_ids.contains(call_id.as_str()) => {
                adjusted_start = index;
            }
            _ => {}
        }
    }

    adjusted_start
}

struct SummarySection {
    lines: Vec<String>,
    was_truncated: bool,
}

fn flush_summary_section(header: &str, lines: &[String]) -> SummarySection {
    if header.is_empty() {
        return SummarySection {
            lines: lines.to_vec(),
            was_truncated: false,
        };
    }

    let mut output = vec![header.to_string()];
    let mut section_text = String::new();
    for line in lines {
        let candidate = if section_text.is_empty() {
            line.clone()
        } else {
            format!("{section_text}\n{line}")
        };
        if approx_token_count(&candidate) > MAX_SESSION_MEMORY_SECTION_TOKENS {
            output.push("[... section truncated for length ...]".to_string());
            return SummarySection {
                lines: output,
                was_truncated: true,
            };
        }
        section_text = candidate;
        output.push(line.clone());
    }

    SummarySection {
        lines: output,
        was_truncated: false,
    }
}

fn summarize_oversized_sections(summary: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut current_header = String::new();
    let mut current_lines = Vec::new();

    for line in summary.lines() {
        if line.starts_with('#') {
            collect_oversized_section(&mut sections, &current_header, &current_lines);
            current_header = line.to_string();
            current_lines.clear();
        } else {
            current_lines.push(line.to_string());
        }
    }

    collect_oversized_section(&mut sections, &current_header, &current_lines);
    sections
}

fn collect_oversized_section(sections: &mut Vec<String>, header: &str, lines: &[String]) {
    if header.is_empty() {
        return;
    }
    let tokens = approx_token_count(&lines.join("\n"));
    if tokens > MAX_SESSION_MEMORY_SECTION_TOKENS {
        sections.push(format!(
            "- \"{header}\" is ~{tokens} tokens (limit: {MAX_SESSION_MEMORY_SECTION_TOKENS})"
        ));
    }
}

fn validate_tail_pairs(tail: &[ResponseItem]) -> CodexResult<()> {
    let function_call_ids: HashSet<&str> = tail
        .iter()
        .filter_map(function_call_output_pair_start_id)
        .collect();
    let custom_call_ids: HashSet<&str> = tail
        .iter()
        .filter_map(|item| match item {
            ResponseItem::CustomToolCall { call_id, .. } => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    let tool_search_call_ids: HashSet<&str> = tail
        .iter()
        .filter_map(|item| match item {
            ResponseItem::ToolSearchCall {
                call_id: Some(call_id),
                ..
            } => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    let function_output_ids: HashSet<&str> = tail
        .iter()
        .filter_map(|item| match item {
            ResponseItem::FunctionCallOutput { call_id, .. } => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    let custom_output_ids: HashSet<&str> = tail
        .iter()
        .filter_map(|item| match item {
            ResponseItem::CustomToolCallOutput { call_id, .. } => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    let tool_search_output_ids: HashSet<&str> = tail
        .iter()
        .filter_map(|item| match item {
            ResponseItem::ToolSearchOutput {
                call_id: Some(call_id),
                ..
            } => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    for item in tail {
        match item {
            ResponseItem::FunctionCall { call_id, .. }
            | ResponseItem::LocalShellCall {
                call_id: Some(call_id),
                ..
            } if !function_output_ids.contains(call_id.as_str()) => {
                return Err(CodexErr::Fatal(
                    "session memory raw tail would split a function call pair".to_string(),
                ));
            }
            ResponseItem::CustomToolCall { call_id, .. }
                if !custom_output_ids.contains(call_id.as_str()) =>
            {
                return Err(CodexErr::Fatal(
                    "session memory raw tail would split a custom tool call pair".to_string(),
                ));
            }
            ResponseItem::ToolSearchCall {
                call_id: Some(call_id),
                ..
            } if !tool_search_output_ids.contains(call_id.as_str()) => {
                return Err(CodexErr::Fatal(
                    "session memory raw tail would split a tool search pair".to_string(),
                ));
            }
            ResponseItem::FunctionCallOutput { call_id, .. } => {
                if !function_call_ids.contains(call_id.as_str()) {
                    return Err(CodexErr::Fatal(
                        "session memory raw tail would split a tool call pair".to_string(),
                    ));
                }
            }
            ResponseItem::CustomToolCallOutput { call_id, .. } => {
                if !custom_call_ids.contains(call_id.as_str()) {
                    return Err(CodexErr::Fatal(
                        "session memory raw tail would split a custom tool call pair".to_string(),
                    ));
                }
            }
            ResponseItem::ToolSearchOutput {
                call_id: Some(call_id),
                ..
            } if !tool_search_call_ids.contains(call_id.as_str()) => {
                return Err(CodexErr::Fatal(
                    "session memory raw tail would split a tool search pair".to_string(),
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

fn function_call_output_pair_start_id(item: &ResponseItem) -> Option<&str> {
    match item {
        ResponseItem::FunctionCall { call_id, .. } => Some(call_id.as_str()),
        ResponseItem::LocalShellCall {
            call_id: Some(call_id),
            ..
        } => Some(call_id.as_str()),
        _ => None,
    }
}

pub(super) fn count_tool_calls(items: &[ResponseItem]) -> usize {
    items
        .iter()
        .filter(|item| {
            matches!(
                item,
                ResponseItem::FunctionCall { .. }
                    | ResponseItem::CustomToolCall { .. }
                    | ResponseItem::ToolSearchCall { .. }
                    | ResponseItem::LocalShellCall { .. }
            )
        })
        .count()
}

fn count_text_items(items: &[ResponseItem]) -> usize {
    items
        .iter()
        .filter(|item| match item {
            ResponseItem::Message { content, .. } => content.iter().any(|content| match content {
                ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                    !text.trim().is_empty()
                }
                ContentItem::InputImage { .. } => false,
            }),
            _ => false,
        })
        .count()
}

pub(super) fn estimate_prompt_tokens(prompt: &Prompt) -> i64 {
    let base_tokens =
        i64::try_from(approx_token_count(&prompt.base_instructions.text)).unwrap_or(i64::MAX);
    base_tokens.saturating_add(estimate_items_tokens(&prompt.input))
}

fn estimate_items_tokens(items: &[ResponseItem]) -> i64 {
    items
        .iter()
        .map(|item| {
            serde_json::to_string(item)
                .map(|text| i64::try_from(approx_token_count(&text)).unwrap_or(i64::MAX))
                .unwrap_or_default()
        })
        .fold(0i64, i64::saturating_add)
}

pub(super) fn item_fingerprint(item: &ResponseItem) -> String {
    let bytes = serde_json::to_vec(item).unwrap_or_default();
    let digest = codex_utils_cache::sha1_digest(&bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn is_compaction_boundary(item: &ResponseItem) -> bool {
    matches!(
        item,
        ResponseItem::Compaction { .. } | ResponseItem::ContextCompaction { .. }
    )
}
