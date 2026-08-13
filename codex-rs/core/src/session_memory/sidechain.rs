use std::path::Path;
use std::sync::Arc;

use crate::client_common::ModelStreamEvent;
use crate::config::ToolSurface;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::tools::context::ToolPayload;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::TranscriptItem;
use codex_rollout_trace::InferenceTraceContext;
use codex_tools::EDIT_TOOL_NAME;
use codex_utils_output_truncation::approx_token_count;
use futures::StreamExt;

use super::ExtractionCandidate;
use super::SessionMemoryStore;
use super::SessionMemoryToolContext;
use super::tail::ExtractionBoundary;
use super::tail::estimate_prompt_tokens;
use super::tail::summary_budget_reminder;

mod code_mode;
mod editor;
mod prompt;

use code_mode::SidechainCodeModeRuntime;
use editor::SummaryEditor;
use editor::apply_summary_edit;
use editor::apply_summary_patch;
use editor::deny_tool_message;
#[cfg(test)]
pub(super) use prompt::substitute_prompt_variables;
pub(super) use prompt::updater_prompt;

const MAX_SIDECHAIN_TOOL_ROUNDS: usize = 6;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SummaryUpdateTool {
    Edit,
    ApplyPatch,
}

pub(super) fn estimate_extraction_input_tokens(
    candidate: &ExtractionCandidate,
    turn_context: &TurnContext,
    store: &SessionMemoryStore,
    current_summary: &str,
) -> i64 {
    let update_tool = SummaryUpdateTool::for_context(&candidate.tool_context);
    let budget_reminder = summary_budget_reminder(current_summary);
    let update_prompt = updater_prompt(
        turn_context.session_memory_update_prompt.as_deref(),
        update_tool,
        &store.summary_path,
        current_summary,
        &budget_reminder,
    );
    let tool_tokens = serde_json::to_string(&candidate.prompt.tools)
        .map(|tools| i64::try_from(approx_token_count(&tools)).unwrap_or(i64::MAX))
        .unwrap_or_default();
    let schema_tokens = candidate
        .prompt
        .output_schema
        .as_ref()
        .map(|schema| i64::try_from(approx_token_count(&schema.to_string())).unwrap_or(i64::MAX))
        .unwrap_or_default();
    estimate_prompt_tokens(&candidate.prompt)
        .saturating_add(i64::try_from(approx_token_count(&update_prompt)).unwrap_or(i64::MAX))
        .saturating_add(tool_tokens)
        .saturating_add(schema_tokens)
}

impl SummaryUpdateTool {
    fn for_context(tool_context: &SessionMemoryToolContext) -> Self {
        match tool_context.surface {
            ToolSurface::Claude => Self::Edit,
            ToolSurface::Codex => Self::ApplyPatch,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Edit => EDIT_TOOL_NAME,
            Self::ApplyPatch => "apply_patch",
        }
    }
}

pub(super) async fn run_extraction(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    store: SessionMemoryStore,
    mut candidate: ExtractionCandidate,
    boundary: ExtractionBoundary,
    current_summary: String,
) -> CodexResult<ExtractionBoundary> {
    run_extraction_inner(
        sess,
        turn_context,
        &store,
        &mut candidate,
        boundary,
        &current_summary,
    )
    .await
}

async fn run_extraction_inner(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    store: &SessionMemoryStore,
    candidate: &mut ExtractionCandidate,
    boundary: ExtractionBoundary,
    current_summary: &str,
) -> CodexResult<ExtractionBoundary> {
    let edit_state = Arc::new(SummaryEditor::new(current_summary.to_string()));
    let update_tool = SummaryUpdateTool::for_context(&candidate.tool_context);
    let budget_reminder = summary_budget_reminder(current_summary);
    candidate.prompt.input.push(TranscriptItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: updater_prompt(
                turn_context.session_memory_update_prompt.as_deref(),
                update_tool,
                &store.summary_path,
                current_summary,
                &budget_reminder,
            ),
        }],
        phase: None,
    });

    let code_mode_runtime = matches!(
        candidate.tool_context.mode,
        codex_protocol::openai_models::ToolMode::CodeMode
            | codex_protocol::openai_models::ToolMode::CodeModeOnly
    )
    .then(|| {
        SidechainCodeModeRuntime::new(
            Arc::clone(&sess),
            Arc::clone(&turn_context),
            candidate.tool_context.code_mode_tool_definitions.clone(),
            store.summary_path.clone(),
            Arc::clone(&edit_state),
        )
    });

    let mut client_session = sess.services.model_client.new_session();
    let extraction_result = 'extraction: {
        for _ in 0..MAX_SIDECHAIN_TOOL_ROUNDS {
            let mut needs_follow_up = false;
            let mut stream = match client_session
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
                .await
            {
                Ok(stream) => stream,
                Err(err) => break 'extraction Err(err),
            };

            while let Some(event) = stream.next().await {
                let event = match event {
                    Ok(event) => event,
                    Err(err) => break 'extraction Err(err),
                };
                match event {
                    ModelStreamEvent::Created
                    | ModelStreamEvent::OutputItemAdded(_)
                    | ModelStreamEvent::ServerModel(_)
                    | ModelStreamEvent::ModelVerifications(_)
                    | ModelStreamEvent::TurnModerationMetadata(_)
                    | ModelStreamEvent::ServerReasoningIncluded(_)
                    | ModelStreamEvent::Warning(_)
                    | ModelStreamEvent::RateLimits(_)
                    | ModelStreamEvent::ModelsEtag(_)
                    | ModelStreamEvent::OutputTextDelta(_)
                    | ModelStreamEvent::ToolCallInputDelta { .. }
                    | ModelStreamEvent::ReasoningSummaryDelta { .. }
                    | ModelStreamEvent::ReasoningSummaryPartAdded { .. }
                    | ModelStreamEvent::ReasoningContentDelta { .. } => {}
                    ModelStreamEvent::Completed { .. } => {}
                    ModelStreamEvent::OutputItemDone(item) => {
                        let outcome = match handle_sidechain_item(
                            &turn_context,
                            update_tool,
                            &store.summary_path,
                            &item,
                            &edit_state,
                            code_mode_runtime.as_ref(),
                        )
                        .await
                        {
                            Ok(outcome) => outcome,
                            Err(err) => break 'extraction Err(err),
                        };
                        candidate.prompt.input.push(item);
                        if let Some(output) = outcome.output {
                            candidate.prompt.input.push(output);
                            if outcome.edited_summary {
                                break 'extraction Ok(boundary);
                            } else {
                                needs_follow_up = true;
                            }
                        }
                    }
                }
            }

            if edit_state.revision().await > 0 {
                break 'extraction Ok(boundary);
            }

            if !needs_follow_up {
                break 'extraction Err(CodexErr::Fatal(format!(
                    "session memory extraction completed without updating summary.md with {}",
                    update_tool.name()
                )));
            }
        }

        Err(CodexErr::Fatal(
            "session memory extraction exceeded tool-call rounds".to_string(),
        ))
    };
    if let Some(runtime) = &code_mode_runtime {
        runtime.shutdown().await;
    }
    extraction_result
}

async fn handle_sidechain_item(
    turn_context: &TurnContext,
    update_tool: SummaryUpdateTool,
    summary_path: &Path,
    item: &TranscriptItem,
    edit_state: &Arc<SummaryEditor>,
    code_mode_runtime: Option<&SidechainCodeModeRuntime>,
) -> CodexResult<SidechainItemOutcome> {
    if let Some(runtime) = code_mode_runtime
        && let Some(outcome) = runtime.handle_item(item).await
    {
        return Ok(SidechainItemOutcome {
            output: Some(outcome.output),
            edited_summary: outcome.edited_summary,
        });
    }
    match item {
        TranscriptItem::FunctionCall {
            name,
            arguments,
            call_id,
            ..
        } if update_tool == SummaryUpdateTool::Edit && name == EDIT_TOOL_NAME => {
            let result =
                apply_summary_edit(turn_context, summary_path, arguments, edit_state).await;
            Ok(SidechainItemOutcome {
                output: Some(TranscriptItem::FunctionCallOutput {
                    call_id: call_id.clone(),
                    output: FunctionCallOutputPayload::from_text(result.text),
                }),
                edited_summary: result.edited_summary,
            })
        }
        TranscriptItem::FunctionCall {
            name,
            arguments,
            call_id,
            ..
        } if update_tool == SummaryUpdateTool::ApplyPatch && name == "apply_patch" => {
            let payload = ToolPayload::Function {
                arguments: arguments.clone(),
            };
            let result = apply_summary_patch(summary_path, &payload, edit_state).await;
            Ok(SidechainItemOutcome {
                output: Some(TranscriptItem::FunctionCallOutput {
                    call_id: call_id.clone(),
                    output: FunctionCallOutputPayload::from_text(result.text),
                }),
                edited_summary: result.edited_summary,
            })
        }
        TranscriptItem::CustomToolCall {
            call_id,
            name,
            input,
            ..
        } if update_tool == SummaryUpdateTool::ApplyPatch && name == "apply_patch" => {
            let payload = ToolPayload::Custom {
                input: input.clone(),
            };
            let result = apply_summary_patch(summary_path, &payload, edit_state).await;
            Ok(SidechainItemOutcome {
                output: Some(TranscriptItem::CustomToolCallOutput {
                    call_id: call_id.clone(),
                    name: Some(name.clone()),
                    output: FunctionCallOutputPayload::from_text(result.text),
                }),
                edited_summary: result.edited_summary,
            })
        }
        TranscriptItem::FunctionCall { call_id, .. } => Ok(SidechainItemOutcome {
            output: Some(TranscriptItem::FunctionCallOutput {
                call_id: call_id.clone(),
                output: FunctionCallOutputPayload::from_text(deny_tool_message(
                    update_tool,
                    summary_path,
                )),
            }),
            edited_summary: false,
        }),
        TranscriptItem::CustomToolCall { call_id, name, .. } => Ok(SidechainItemOutcome {
            output: Some(TranscriptItem::CustomToolCallOutput {
                call_id: call_id.clone(),
                name: Some(name.clone()),
                output: FunctionCallOutputPayload::from_text(deny_tool_message(
                    update_tool,
                    summary_path,
                )),
            }),
            edited_summary: false,
        }),
        TranscriptItem::ToolSearchCall {
            call_id, execution, ..
        } => Ok(SidechainItemOutcome {
            output: Some(TranscriptItem::ToolSearchOutput {
                call_id: call_id.clone(),
                status: "failed".to_string(),
                execution: execution.clone(),
                tools: Vec::new(),
            }),
            edited_summary: false,
        }),
        TranscriptItem::LocalShellCall {
            call_id: Some(call_id),
            ..
        } => Ok(SidechainItemOutcome {
            output: Some(TranscriptItem::FunctionCallOutput {
                call_id: call_id.clone(),
                output: FunctionCallOutputPayload::from_text(deny_tool_message(
                    update_tool,
                    summary_path,
                )),
            }),
            edited_summary: false,
        }),
        TranscriptItem::Message { .. }
        | TranscriptItem::AgentMessage { .. }
        | TranscriptItem::Reasoning { .. }
        | TranscriptItem::LocalShellCall { call_id: None, .. }
        | TranscriptItem::FunctionCallOutput { .. }
        | TranscriptItem::CustomToolCallOutput { .. }
        | TranscriptItem::ToolSearchOutput { .. }
        | TranscriptItem::WebSearchCall { .. }
        | TranscriptItem::ImageGenerationCall { .. }
        | TranscriptItem::Compaction { .. }
        | TranscriptItem::CompactionTrigger
        | TranscriptItem::ContextCompaction { .. }
        | TranscriptItem::Other => Ok(SidechainItemOutcome {
            output: None,
            edited_summary: false,
        }),
    }
}

struct SidechainItemOutcome {
    output: Option<TranscriptItem>,
    edited_summary: bool,
}

#[cfg(test)]
#[path = "sidechain_tests.rs"]
mod tests;
