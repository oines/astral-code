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
    let prompt_summary = truncate_text(
        &current_summary,
        TruncationPolicy::Tokens(MAX_COMPACT_SUMMARY_BODY_TOKENS),
    );
    let budget_reminder = summary_budget_reminder(&current_summary);
    candidate.prompt.input.push(ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: updater_prompt(&store.summary_path, &prompt_summary, &budget_reminder),
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
            validate_post_extraction_summary(&current_summary, &summary)?;
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

fn updater_prompt(summary_path: &Path, current_summary: &str, budget_reminder: &str) -> String {
    format!(
        r#"This is an internal session-memory update.

Update only this file using the Edit tool:
{summary_path}

Do not call Bash, Read, Write, MCP tools, tool search, or any other tool. Preserve the exact top-level headings:

- Current State
- Worklog
- Errors
- Files
- Key Results

Keep the summary concise, factual, and useful for a future context compaction. Update it incrementally from the conversation context above. Include durable decisions, current task state, files touched or inspected, important errors, and key results. Remove stale details when they are no longer useful. Keep each section under roughly 2,000 tokens and the whole file under roughly 10,000 tokens.{budget_reminder}

Current summary.md:

```markdown
{current_summary}
```
"#,
        summary_path = summary_path.display()
    )
}
