use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use codex_code_mode::CellId;
use codex_code_mode::CodeModeNestedToolCall;
use codex_code_mode::ExecuteRequest;
use codex_code_mode::RuntimeResponse;
use codex_code_mode::ToolDefinition;
use codex_code_mode::WaitOutcome;
use codex_code_mode::WaitRequest;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::TranscriptItem;
use codex_tools::ToolName;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use tokio_util::sync::CancellationToken;

use super::SummaryUpdateTool;
use super::editor::SummaryEditor;
use super::editor::apply_summary_patch;
use super::editor::deny_tool_message;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::tools::code_mode::CodeModeDispatchHost;
use crate::tools::code_mode::CodeModeDispatchWorker;
use crate::tools::code_mode::CodeModeService;
use crate::tools::code_mode::ExecContext;
use crate::tools::code_mode::build_nested_tool_payload;
use crate::tools::code_mode::handle_runtime_response;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolPayload;

pub(super) struct SidechainCodeModeRuntime {
    service: CodeModeService,
    _worker: CodeModeDispatchWorker,
    exec: ExecContext,
    enabled_tools: Vec<ToolDefinition>,
    edit_state: Arc<SummaryEditor>,
}

impl SidechainCodeModeRuntime {
    pub(super) fn new(
        session: Arc<Session>,
        turn: Arc<TurnContext>,
        enabled_tools: Vec<ToolDefinition>,
        summary_path: PathBuf,
        edit_state: Arc<SummaryEditor>,
    ) -> Self {
        let service = CodeModeService::new(session.services.code_mode_service.session_provider());
        let host = Arc::new(SummaryCodeModeHost {
            summary_path,
            edit_state: Arc::clone(&edit_state),
        });
        let worker = service.start_dispatch_worker(host);
        Self {
            service,
            _worker: worker,
            exec: ExecContext::new(session, turn),
            enabled_tools,
            edit_state,
        }
    }

    pub(super) async fn handle_item(&self, item: &TranscriptItem) -> Option<CodeModeItemOutcome> {
        match item {
            TranscriptItem::FunctionCall {
                name,
                arguments,
                call_id,
                ..
            } if name == codex_code_mode::PUBLIC_TOOL_NAME => {
                let payload = ToolPayload::Function {
                    arguments: arguments.clone(),
                };
                let input = function_exec_input(arguments);
                Some(self.execute(call_id, name, payload, input).await)
            }
            TranscriptItem::CustomToolCall {
                name,
                input,
                call_id,
                ..
            } if name == codex_code_mode::PUBLIC_TOOL_NAME => {
                let payload = ToolPayload::Custom {
                    input: input.clone(),
                };
                Some(
                    self.execute(call_id, name, payload, Ok(input.clone()))
                        .await,
                )
            }
            TranscriptItem::FunctionCall {
                name,
                arguments,
                call_id,
                ..
            } if name == codex_code_mode::WAIT_TOOL_NAME => {
                Some(self.wait(call_id, arguments).await)
            }
            _ => None,
        }
    }

    async fn execute(
        &self,
        call_id: &str,
        tool_name: &str,
        payload: ToolPayload,
        input: Result<String, String>,
    ) -> CodeModeItemOutcome {
        let output = match input.and_then(|input| codex_code_mode::parse_exec_source(&input)) {
            Ok(args) => {
                let started_at = Instant::now();
                match self
                    .service
                    .execute(ExecuteRequest {
                        tool_call_id: call_id.to_string(),
                        enabled_tools: self.enabled_tools.clone(),
                        source: args.code,
                        yield_time_ms: args.yield_time_ms,
                        max_output_tokens: args.max_output_tokens,
                    })
                    .await
                {
                    Ok(started_cell) => {
                        let cell_id = started_cell.cell_id.clone();
                        self.service.mark_cell_ready_for_dispatch(&cell_id);
                        match started_cell.initial_response().await {
                            Ok(response) => {
                                if !matches!(response, RuntimeResponse::Yielded { .. }) {
                                    self.service.finish_cell_dispatch(&cell_id);
                                }
                                handle_runtime_response(
                                    &self.exec,
                                    response,
                                    args.max_output_tokens,
                                    started_at,
                                )
                                .await
                            }
                            Err(err) => Err(err),
                        }
                    }
                    Err(err) => Err(err),
                }
            }
            Err(err) => Err(err),
        };
        self.outcome(call_id, Some(tool_name), &payload, output)
            .await
    }

    async fn wait(&self, call_id: &str, arguments: &str) -> CodeModeItemOutcome {
        let payload = ToolPayload::Function {
            arguments: arguments.to_string(),
        };
        let output = match serde_json::from_str::<WaitArguments>(arguments) {
            Ok(args) => {
                let started_at = Instant::now();
                let cell_id = CellId::new(args.cell_id);
                let response = if args.terminate {
                    self.service.terminate(cell_id).await
                } else {
                    self.service
                        .wait(WaitRequest {
                            cell_id,
                            yield_time_ms: args.yield_time_ms,
                        })
                        .await
                };
                match response {
                    Ok(wait_outcome) => {
                        if let WaitOutcome::LiveCell(response) = &wait_outcome
                            && !matches!(response, RuntimeResponse::Yielded { .. })
                        {
                            self.service.finish_cell_dispatch(runtime_cell_id(response));
                        }
                        handle_runtime_response(
                            &self.exec,
                            wait_outcome.into(),
                            args.max_tokens,
                            started_at,
                        )
                        .await
                    }
                    Err(err) => Err(err),
                }
            }
            Err(err) => Err(format!("failed to parse wait arguments: {err}")),
        };
        self.outcome(
            call_id,
            Some(codex_code_mode::WAIT_TOOL_NAME),
            &payload,
            output,
        )
        .await
    }

    async fn outcome(
        &self,
        call_id: &str,
        tool_name: Option<&str>,
        payload: &ToolPayload,
        output: Result<FunctionToolOutput, String>,
    ) -> CodeModeItemOutcome {
        let output =
            output.unwrap_or_else(|error| FunctionToolOutput::from_text(error, Some(false)));
        let edited_summary = self.edit_state.revision().await > 0;
        let output = FunctionCallOutputPayload {
            body: FunctionCallOutputBody::ContentItems(output.body),
            success: output.success,
        };
        let item = match payload {
            ToolPayload::Custom { .. } => TranscriptItem::CustomToolCallOutput {
                call_id: call_id.to_string(),
                name: tool_name.map(str::to_string),
                output,
            },
            ToolPayload::Function { .. } => TranscriptItem::FunctionCallOutput {
                call_id: call_id.to_string(),
                output,
            },
            ToolPayload::ToolSearch { .. } => unreachable!("exec and wait never use tool search"),
        };
        CodeModeItemOutcome {
            output: item,
            edited_summary,
        }
    }

    pub(super) async fn shutdown(&self) {
        let _ = self.service.shutdown().await;
    }
}

pub(super) struct CodeModeItemOutcome {
    pub(super) output: TranscriptItem,
    pub(super) edited_summary: bool,
}

struct SummaryCodeModeHost {
    summary_path: PathBuf,
    edit_state: Arc<SummaryEditor>,
}

impl CodeModeDispatchHost for SummaryCodeModeHost {
    async fn invoke_tool(
        &self,
        invocation: CodeModeNestedToolCall,
        _cancellation_token: CancellationToken,
    ) -> Result<JsonValue, String> {
        if invocation.tool_name != ToolName::plain("apply_patch") {
            return Err(deny_tool_message(
                SummaryUpdateTool::ApplyPatch,
                &self.summary_path,
            ));
        }
        let payload = build_nested_tool_payload(
            invocation.tool_kind,
            &invocation.tool_name,
            invocation.input,
        )?;
        let result = apply_summary_patch(&self.summary_path, &payload, &self.edit_state).await;
        if result.edited_summary {
            Ok(JsonValue::Object(serde_json::Map::new()))
        } else {
            Err(result.text)
        }
    }

    async fn notify(
        &self,
        _call_id: String,
        _cell_id: CellId,
        _text: String,
    ) -> Result<(), String> {
        Err("code mode notifications are not allowed during session-memory extraction".to_string())
    }
}

#[derive(Deserialize)]
struct ExecFunctionArguments {
    input: String,
}

fn function_exec_input(arguments: &str) -> Result<String, String> {
    serde_json::from_str::<ExecFunctionArguments>(arguments)
        .map(|arguments| arguments.input)
        .map_err(|err| format!("exec expects JSON arguments with an `input` string: {err}"))
}

#[derive(Deserialize)]
struct WaitArguments {
    cell_id: String,
    #[serde(default = "default_wait_yield_time_ms")]
    yield_time_ms: u64,
    #[serde(default)]
    max_tokens: Option<usize>,
    #[serde(default)]
    terminate: bool,
}

fn default_wait_yield_time_ms() -> u64 {
    codex_code_mode::DEFAULT_WAIT_YIELD_TIME_MS
}

fn runtime_cell_id(response: &RuntimeResponse) -> &CellId {
    match response {
        RuntimeResponse::Yielded { cell_id, .. }
        | RuntimeResponse::Terminated { cell_id, .. }
        | RuntimeResponse::Result { cell_id, .. } => cell_id,
    }
}
