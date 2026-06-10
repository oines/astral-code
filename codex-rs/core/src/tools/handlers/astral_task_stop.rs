use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::multi_agents_common::tool_output_code_mode_result;
use crate::tools::handlers::multi_agents_common::tool_output_json_text;
use crate::tools::handlers::multi_agents_common::tool_output_response_item;
use crate::tools::handlers::multi_agents_v2::InterruptAgentHandler;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use crate::unified_exec::TerminatedProcess;
use crate::unified_exec::UnifiedExecError;
use codex_protocol::models::ResponseInputItem;
use codex_tools::ResponsesApiTool;
use codex_tools::TASK_STOP_TOOL_NAME;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::astral_core_tool_by_name;
use codex_tools::parse_tool_input_schema_without_compaction;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value as JsonValue;
use serde_json::json;

pub struct AstralTaskStopHandler {
    interrupt_agent: InterruptAgentHandler,
}

impl AstralTaskStopHandler {
    pub(crate) fn new() -> Self {
        Self {
            interrupt_agent: InterruptAgentHandler,
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for AstralTaskStopHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TASK_STOP_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        let tool = astral_core_tool_by_name(TASK_STOP_TOOL_NAME).unwrap_or_else(|| {
            panic!("astral core tool `{TASK_STOP_TOOL_NAME}` should have a schema");
        });
        let parameters = parse_tool_input_schema_without_compaction(&tool.input_schema)
            .unwrap_or_else(|err| {
                panic!("astral core tool `{TASK_STOP_TOOL_NAME}` schema should parse: {err}");
            });

        ToolSpec::Function(ResponsesApiTool {
            name: tool.name,
            description: tool.description,
            strict: false,
            defer_loading: None,
            parameters,
            output_schema: None,
        })
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        let target = task_stop_target(&invocation.payload)?;
        if let Some(process_id) = target.shell_process_id()? {
            match invocation
                .session
                .services
                .unified_exec_manager
                .terminate_process(process_id)
                .await
            {
                Ok(process) => {
                    return Ok(boxed_tool_output(TaskStopResult::from_shell(process)));
                }
                Err(UnifiedExecError::UnknownProcessId { .. })
                    if target.hint == TargetHint::TaskId => {}
                Err(err) => {
                    return Err(FunctionCallError::RespondToModel(format!(
                        "TaskStop failed: {err}"
                    )));
                }
            }
        }

        self.interrupt_agent
            .handle(to_interrupt_agent_invocation(invocation)?)
            .await?;
        Ok(boxed_tool_output(TaskStopResult::from_agent(target.id)))
    }
}

impl CoreToolRuntime for AstralTaskStopHandler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }
}

#[derive(Deserialize)]
struct AstralTaskStopArgs {
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    shell_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TargetHint {
    TaskId,
    ShellId,
}

struct TaskStopTarget {
    id: String,
    hint: TargetHint,
}

impl TaskStopTarget {
    fn shell_process_id(&self) -> Result<Option<i32>, FunctionCallError> {
        match self.hint {
            TargetHint::TaskId => Ok(self.id.parse::<i32>().ok()),
            TargetHint::ShellId => self.id.parse::<i32>().map(Some).map_err(|_| {
                FunctionCallError::RespondToModel(
                    "TaskStop `shell_id` must be a numeric Bash session id".to_string(),
                )
            }),
        }
    }
}

#[derive(Debug, Serialize)]
struct TaskStopResult {
    message: String,
    task_id: String,
    task_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
}

impl TaskStopResult {
    fn from_shell(process: TerminatedProcess) -> Self {
        let task_id = process.process_id.to_string();
        Self {
            message: format!(
                "Successfully stopped task: {} ({})",
                task_id, process.command
            ),
            task_id,
            task_type: "shell".to_string(),
            command: Some(process.command),
        }
    }

    fn from_agent(task_id: String) -> Self {
        Self {
            message: format!("Successfully stopped task: {task_id}"),
            task_id,
            task_type: "agent".to_string(),
            command: None,
        }
    }
}

impl ToolOutput for TaskStopResult {
    fn log_preview(&self) -> String {
        tool_output_json_text(self, TASK_STOP_TOOL_NAME)
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, payload: &ToolPayload) -> ResponseInputItem {
        tool_output_response_item(call_id, payload, self, Some(true), TASK_STOP_TOOL_NAME)
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> JsonValue {
        tool_output_code_mode_result(self, TASK_STOP_TOOL_NAME)
    }
}

fn to_interrupt_agent_invocation(
    mut invocation: ToolInvocation,
) -> Result<ToolInvocation, FunctionCallError> {
    let ToolPayload::Function { arguments } = invocation.payload else {
        return Err(FunctionCallError::RespondToModel(
            "TaskStop handler received unsupported payload".to_string(),
        ));
    };

    invocation.tool_name = ToolName::plain("interrupt_agent");
    invocation.payload = ToolPayload::Function {
        arguments: rewrite_task_stop_arguments(&arguments)?,
    };
    Ok(invocation)
}

fn rewrite_task_stop_arguments(arguments: &str) -> Result<String, FunctionCallError> {
    let target = task_stop_target_from_arguments(arguments)?;

    serde_json::to_string(&json!({ "target": target.id })).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to serialize rewritten TaskStop arguments: {err}"
        ))
    })
}

fn task_stop_target(payload: &ToolPayload) -> Result<TaskStopTarget, FunctionCallError> {
    let ToolPayload::Function { arguments } = payload else {
        return Err(FunctionCallError::RespondToModel(
            "TaskStop handler received unsupported payload".to_string(),
        ));
    };
    task_stop_target_from_arguments(arguments)
}

fn task_stop_target_from_arguments(arguments: &str) -> Result<TaskStopTarget, FunctionCallError> {
    let args: AstralTaskStopArgs = parse_arguments(arguments)?;
    if let Some(task_id) = non_empty(args.task_id) {
        return Ok(TaskStopTarget {
            id: task_id,
            hint: TargetHint::TaskId,
        });
    }
    if let Some(shell_id) = non_empty(args.shell_id) {
        return Ok(TaskStopTarget {
            id: shell_id,
            hint: TargetHint::ShellId,
        });
    }
    Err(FunctionCallError::RespondToModel(
        "TaskStop requires `task_id` or `shell_id` to identify the target".to_string(),
    ))
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
#[path = "astral_task_stop_tests.rs"]
mod tests;
