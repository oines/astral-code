use std::collections::HashSet;

use crate::Prompt;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::models::ResponseItem;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::approx_token_count;
use codex_utils_output_truncation::truncate_text;

use super::SessionMemoryState;

pub(super) const DEFAULT_SUMMARY: &str = r#"# Session Memory

## Current State
- No durable session memory has been extracted yet.

## Worklog
- No work recorded yet.

## Errors
- None recorded.

## Files
- None recorded.

## Key Results
- None recorded.
"#;

const MAX_RAW_TAIL_TOKENS: usize = 40_000;
const MAX_POST_COMPACT_TOKENS: usize = 110_000;
const MAX_SESSION_MEMORY_SECTION_TOKENS: usize = 2_000;
const MAX_SESSION_MEMORY_TOTAL_TOKENS: usize = 10_000;
const MIN_EXISTING_SUMMARY_TOKENS_FOR_COLLAPSE_GUARD: usize = 2_000;
const MIN_REWRITTEN_SUMMARY_TOKENS: usize = 500;
pub(super) const MAX_COMPACT_SUMMARY_BODY_TOKENS: usize = 9_500;
const REQUIRED_SUMMARY_HEADINGS: &[&str] = &[
    "## Current State",
    "## Worklog",
    "## Errors",
    "## Files",
    "## Key Results",
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
    let boundary_index = state
        .last_summary_index
        .ok_or_else(|| CodexErr::Fatal("session memory boundary missing".to_string()))?;
    let boundary = items
        .get(boundary_index)
        .ok_or_else(|| CodexErr::Fatal("session memory boundary not found".to_string()))?;
    let expected_fingerprint = state.last_summary_fingerprint.as_deref().ok_or_else(|| {
        CodexErr::Fatal("session memory boundary fingerprint missing".to_string())
    })?;
    if item_fingerprint(boundary) != expected_fingerprint {
        return Err(CodexErr::Fatal(
            "session memory boundary fingerprint mismatch".to_string(),
        ));
    }

    let last_compaction_index = items
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, item)| is_compaction_boundary(item).then_some(index));
    let start = boundary_index
        .saturating_add(1)
        .max(last_compaction_index.map_or(0, |index| index.saturating_add(1)));
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

pub(super) fn validate_post_compact_budget(items: &[ResponseItem]) -> CodexResult<()> {
    let tokens = estimate_items_tokens(items);
    if usize::try_from(tokens).unwrap_or(usize::MAX) > MAX_POST_COMPACT_TOKENS {
        return Err(CodexErr::Fatal(format!(
            "session memory compacted history exceeds {MAX_POST_COMPACT_TOKENS} tokens"
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
    if total_tokens > MAX_SESSION_MEMORY_TOTAL_TOKENS {
        reminder.push_str(&format!(
            "\n\nCRITICAL: summary.md is currently ~{total_tokens} tokens, which exceeds the {MAX_SESSION_MEMORY_TOTAL_TOKENS} token budget. Condense it by removing stale details, merging older worklog entries, and preserving the most important Current State, Errors, Files, and Key Results."
        ));
    }
    if !oversized_sections.is_empty() {
        reminder.push_str(&format!(
            "\n\nOversized sections to condense (limit ~{MAX_SESSION_MEMORY_SECTION_TOKENS} tokens each):\n{}",
            oversized_sections.join("\n")
        ));
    }
    reminder
}

pub(super) fn format_session_memory_summary(
    summary: &str,
    was_truncated_for_compact: bool,
) -> String {
    let mut formatted = format!("Session memory summary:\n\n{}", summary.trim());
    if was_truncated_for_compact {
        formatted.push_str("\n\nSome session memory sections were truncated for compact length.");
    }
    formatted.push_str("\n\nRaw transcript tail after this summary follows below.");
    formatted
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
        sections.push(format!("- {header}: ~{tokens} tokens"));
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
