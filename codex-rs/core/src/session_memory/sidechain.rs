use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use crate::client_common::ResponseEvent;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseItem;
use codex_rollout_trace::InferenceTraceContext;
use codex_tools::EDIT_TOOL_NAME;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::truncate_text;
use codex_utils_path::paths_match_after_normalization;
use futures::StreamExt;
use serde::Deserialize;

use super::ExtractionCandidate;
use super::SessionMemoryStore;
use super::tail::ExtractionBoundary;
use super::tail::MAX_COMPACT_SUMMARY_BODY_TOKENS;
use super::tail::summary_budget_reminder;
use super::tail::validate_post_extraction_summary;

const MAX_SIDECHAIN_TOOL_ROUNDS: usize = 3;

pub(super) async fn run_extraction(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    store: SessionMemoryStore,
    mut candidate: ExtractionCandidate,
    boundary: ExtractionBoundary,
) -> CodexResult<ExtractionBoundary> {
    let current_summary = store.read_summary().await?;
    let result = run_extraction_inner(
        sess,
        turn_context,
        &store,
        &mut candidate,
        boundary,
        &current_summary,
    )
    .await;
    if result.is_err() {
        let _ = tokio::fs::write(&store.summary_path, current_summary).await;
    }
    result
}

async fn run_extraction_inner(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    store: &SessionMemoryStore,
    candidate: &mut ExtractionCandidate,
    boundary: ExtractionBoundary,
    current_summary: &str,
) -> CodexResult<ExtractionBoundary> {
    let prompt_summary = truncate_text(
        current_summary,
        TruncationPolicy::Tokens(MAX_COMPACT_SUMMARY_BODY_TOKENS),
    );
    let budget_reminder = summary_budget_reminder(current_summary);
    candidate.prompt.input.push(ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: updater_prompt(
                turn_context.session_memory_update_prompt.as_deref(),
                &store.summary_path,
                &prompt_summary,
                &budget_reminder,
            ),
        }],
        phase: None,
    });

    let mut client_session = sess.services.model_client.new_session();
    for _ in 0..MAX_SIDECHAIN_TOOL_ROUNDS {
        let mut needs_follow_up = false;
        let mut stream = client_session
            .stream(
                turn_context.provider.clone(),
                &candidate.prompt,
                &turn_context.model_info,
                &turn_context.session_telemetry,
                turn_context.reasoning_effort.clone(),
                turn_context.reasoning_summary,
                turn_context.config.service_tier.clone(),
                None,
                &InferenceTraceContext::disabled(),
                false,
            )
            .await?;

        while let Some(event) = stream.next().await {
            match event? {
                ResponseEvent::Created
                | ResponseEvent::OutputItemAdded(_)
                | ResponseEvent::ServerModel(_)
                | ResponseEvent::ModelVerifications(_)
                | ResponseEvent::TurnModerationMetadata(_)
                | ResponseEvent::ServerReasoningIncluded(_)
                | ResponseEvent::RateLimits(_)
                | ResponseEvent::ModelsEtag(_)
                | ResponseEvent::OutputTextDelta(_)
                | ResponseEvent::ToolCallInputDelta { .. }
                | ResponseEvent::ReasoningSummaryDelta { .. }
                | ResponseEvent::ReasoningSummaryPartAdded { .. }
                | ResponseEvent::ReasoningContentDelta { .. } => {}
                ResponseEvent::Completed { .. } => {}
                ResponseEvent::OutputItemDone(item) => {
                    let output =
                        handle_sidechain_item(&turn_context, &store.summary_path, &item).await?;
                    candidate.prompt.input.push(item);
                    if let Some(output) = output {
                        candidate.prompt.input.push(output);
                        needs_follow_up = true;
                    }
                }
            }
        }

        if !needs_follow_up {
            let summary = store.read_summary().await?;
            validate_post_extraction_summary(
                current_summary,
                &summary,
                turn_context.session_memory_template(),
            )?;
            return Ok(boundary);
        }
    }

    Err(CodexErr::Fatal(
        "session memory extraction exceeded tool-call rounds".to_string(),
    ))
}

async fn handle_sidechain_item(
    turn_context: &TurnContext,
    summary_path: &Path,
    item: &ResponseItem,
) -> CodexResult<Option<ResponseItem>> {
    match item {
        ResponseItem::FunctionCall {
            name,
            arguments,
            call_id,
            ..
        } if name == EDIT_TOOL_NAME => {
            let text = apply_summary_edit(turn_context, summary_path, arguments).await;
            Ok(Some(ResponseItem::FunctionCallOutput {
                call_id: call_id.clone(),
                output: FunctionCallOutputPayload::from_text(text),
            }))
        }
        ResponseItem::FunctionCall { call_id, .. } => Ok(Some(ResponseItem::FunctionCallOutput {
            call_id: call_id.clone(),
            output: FunctionCallOutputPayload::from_text(deny_tool_message(summary_path)),
        })),
        ResponseItem::CustomToolCall { call_id, name, .. } => {
            Ok(Some(ResponseItem::CustomToolCallOutput {
                call_id: call_id.clone(),
                name: Some(name.clone()),
                output: FunctionCallOutputPayload::from_text(deny_tool_message(summary_path)),
            }))
        }
        ResponseItem::ToolSearchCall {
            call_id, execution, ..
        } => Ok(Some(ResponseItem::ToolSearchOutput {
            call_id: call_id.clone(),
            status: "failed".to_string(),
            execution: execution.clone(),
            tools: Vec::new(),
        })),
        ResponseItem::LocalShellCall {
            call_id: Some(call_id),
            ..
        } => Ok(Some(ResponseItem::FunctionCallOutput {
            call_id: call_id.clone(),
            output: FunctionCallOutputPayload::from_text(deny_tool_message(summary_path)),
        })),
        ResponseItem::Message { .. }
        | ResponseItem::AgentMessage { .. }
        | ResponseItem::Reasoning { .. }
        | ResponseItem::LocalShellCall { call_id: None, .. }
        | ResponseItem::FunctionCallOutput { .. }
        | ResponseItem::CustomToolCallOutput { .. }
        | ResponseItem::ToolSearchOutput { .. }
        | ResponseItem::WebSearchCall { .. }
        | ResponseItem::ImageGenerationCall { .. }
        | ResponseItem::Compaction { .. }
        | ResponseItem::CompactionTrigger
        | ResponseItem::ContextCompaction { .. }
        | ResponseItem::Other => Ok(None),
    }
}

#[derive(Deserialize)]
struct EditArgs {
    file_path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

async fn apply_summary_edit(
    turn_context: &TurnContext,
    summary_path: &Path,
    arguments: &str,
) -> String {
    let args: EditArgs = match serde_json::from_str(arguments) {
        Ok(args) => args,
        Err(err) => return format!("Invalid Edit arguments: {err}"),
    };

    let target_path = resolve_edit_path(turn_context, &args.file_path);
    if !paths_match_after_normalization(&target_path, summary_path) {
        return deny_tool_message(summary_path);
    }
    if args.old_string == args.new_string {
        return "No changes to make: old_string and new_string are exactly the same.".to_string();
    }

    let current = tokio::fs::read_to_string(summary_path)
        .await
        .unwrap_or_default();
    if args.old_string.is_empty() {
        if !current.trim().is_empty() {
            return "Cannot create new file - file already exists.".to_string();
        }
        return match tokio::fs::write(summary_path, args.new_string).await {
            Ok(()) => format!(
                "The file {} has been updated successfully.",
                summary_path.display()
            ),
            Err(err) => format!("Failed to update file: {err}"),
        };
    }

    let occurrences = current.matches(&args.old_string).count();
    if occurrences == 0 {
        return format!(
            "String to replace not found in file.\nString: {}",
            args.old_string
        );
    }
    if occurrences > 1 && !args.replace_all {
        return format!(
            "Found {occurrences} matches of the string to replace, but replace_all is false. To replace all occurrences, set replace_all to true. To replace only one occurrence, please provide more context to uniquely identify the instance.\nString: {}",
            args.old_string
        );
    }

    let updated = if args.replace_all {
        current.replace(&args.old_string, &args.new_string)
    } else {
        current.replacen(&args.old_string, &args.new_string, 1)
    };
    match tokio::fs::write(summary_path, updated).await {
        Ok(()) if args.replace_all => format!(
            "The file {} has been updated. All occurrences were successfully replaced.",
            summary_path.display()
        ),
        Ok(()) => format!(
            "The file {} has been updated successfully.",
            summary_path.display()
        ),
        Err(err) => format!("Failed to update file: {err}"),
    }
}

fn resolve_edit_path(turn_context: &TurnContext, file_path: &str) -> PathBuf {
    let path = PathBuf::from(file_path);
    if path.is_absolute() {
        path
    } else {
        turn_context
            .environments
            .single_local_environment_cwd()
            .unwrap_or(&turn_context.config.cwd)
            .join(path)
            .to_path_buf()
    }
}

fn deny_tool_message(summary_path: &Path) -> String {
    format!(
        "Denied: session-memory extraction may only use Edit on {}.",
        summary_path.display()
    )
}

const DEFAULT_UPDATE_PROMPT: &str = r#"IMPORTANT: This message and these instructions are NOT part of the actual user conversation. Do NOT include any references to "note-taking", "session notes extraction", or these update instructions in the notes content.

Based on the user conversation above (EXCLUDING this note-taking instruction message as well as system prompt, AGENTS.md entries, or any past session summaries), update the session notes file.

The file {{notesPath}} has already been read for you. Here are its current contents:
<current_notes_content>
{{currentNotes}}
</current_notes_content>

Your ONLY task is to use the Edit tool to update the notes file, then stop. You can make multiple edits (update every section as needed) - make all Edit tool calls in parallel in a single message. Do not call any other tools.

CRITICAL RULES FOR EDITING:
- The file must maintain its exact structure with all sections, headers, and italic descriptions intact
-- NEVER modify, delete, or add section headers (the lines starting with '#' like # Task specification)
-- NEVER modify or delete the italic _section description_ lines (these are the lines in italics immediately following each header - they start and end with underscores)
-- The italic _section descriptions_ are TEMPLATE INSTRUCTIONS that must be preserved exactly as-is - they guide what content belongs in each section
-- ONLY update the actual content that appears BELOW the italic _section descriptions_ within each existing section
-- Do NOT add any new sections, summaries, or information outside the existing structure
- Do NOT reference this note-taking process or instructions anywhere in the notes
- It's OK to skip updating a section if there are no substantial new insights to add. Do not add filler content like "No info yet", just leave sections blank/unedited if appropriate.
- Write DETAILED, INFO-DENSE content for each section - include specifics like file paths, function names, error messages, exact commands, technical details, etc.
- For "Key results", include the complete, exact output the user requested (e.g., full table, full answer, etc.)
- Do not include information that's already in the AGENTS.md files included in the context
- Keep each section under ~2,000 tokens/words - if a section is approaching this limit, condense it by cycling out less important details while preserving the most critical information
- Focus on actionable, specific information that would help someone understand or recreate the work discussed in the conversation
- IMPORTANT: Always update "Current State" to reflect the most recent work - this is critical for continuity after compaction

Use the Edit tool with file_path: {{notesPath}}

STRUCTURE PRESERVATION REMINDER:
Each section has TWO parts that must be preserved exactly as they appear in the current file:
1. The section header (line starting with #)
2. The italic description line (the _italicized text_ immediately after the header - this is a template instruction)

You ONLY update the actual content that comes AFTER these two preserved lines. The italic description lines starting and ending with underscores are part of the template structure, NOT content to be edited or removed.

REMEMBER: Use the Edit tool in parallel and stop. Do not continue after the edits. Only include insights from the actual user conversation, never from these note-taking instructions. Do not delete or change section headers or italic _section descriptions_.
"#;

fn updater_prompt(
    custom_prompt: Option<&str>,
    summary_path: &Path,
    current_summary: &str,
    budget_reminder: &str,
) -> String {
    let template = custom_prompt.unwrap_or(DEFAULT_UPDATE_PROMPT);
    let prompt = substitute_prompt_variables(
        template,
        summary_path.to_string_lossy().as_ref(),
        current_summary,
    );
    if prompt.ends_with('\n') || budget_reminder.is_empty() {
        format!("{prompt}{budget_reminder}")
    } else {
        format!("{prompt}\n{budget_reminder}")
    }
}

pub(super) fn substitute_prompt_variables(
    template: &str,
    notes_path: &str,
    current_notes: &str,
) -> String {
    let mut output = String::with_capacity(template.len() + current_notes.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        let Some(end) = after_open.find("}}") else {
            output.push_str(&rest[start..]);
            return output;
        };
        let key = &after_open[..end];
        match key {
            "currentNotes" => output.push_str(current_notes),
            "notesPath" => output.push_str(notes_path),
            _ => {
                output.push_str("{{");
                output.push_str(key);
                output.push_str("}}");
            }
        }
        rest = &after_open[end + 2..];
    }
    output.push_str(rest);
    output
}
