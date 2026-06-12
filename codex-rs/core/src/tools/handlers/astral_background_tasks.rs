use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::WriteStdinHandler;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::PostToolUsePayload;
use crate::tools::registry::PreToolUsePayload;
use crate::tools::registry::ToolExecutor;
use codex_tools::LIST_BACKGROUND_TASKS_TOOL_NAME;
use codex_tools::READ_TASK_OUTPUT_TOOL_NAME;
use codex_tools::ResponsesApiTool;
use codex_tools::SEND_TASK_INPUT_TOOL_NAME;
use codex_tools::STOP_BACKGROUND_TASK_TOOL_NAME;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::astral_core_tool_by_name;
use codex_tools::parse_tool_input_schema_without_compaction;
use serde_json::Map;
use serde_json::Number;
use serde_json::Value;

pub(crate) struct AstralReadTaskOutputHandler {
    write_stdin: WriteStdinHandler,
}

impl AstralReadTaskOutputHandler {
    pub(crate) fn new() -> Self {
        Self {
            write_stdin: WriteStdinHandler,
        }
    }
}

pub(crate) struct AstralSendTaskInputHandler {
    write_stdin: WriteStdinHandler,
}

impl AstralSendTaskInputHandler {
    pub(crate) fn new() -> Self {
        Self {
            write_stdin: WriteStdinHandler,
        }
    }
}

pub(crate) struct AstralListBackgroundTasksHandler;

pub(crate) struct AstralStopBackgroundTaskHandler;

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for AstralReadTaskOutputHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(READ_TASK_OUTPUT_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        astral_task_tool_spec(READ_TASK_OUTPUT_TOOL_NAME)
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        self.write_stdin
            .handle(to_write_stdin_invocation(
                invocation,
                TaskIoMode::ReadOutput,
            )?)
            .await
    }
}

impl CoreToolRuntime for AstralReadTaskOutputHandler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }

    fn pre_tool_use_payload(&self, _invocation: &ToolInvocation) -> Option<PreToolUsePayload> {
        None
    }

    fn post_tool_use_payload(
        &self,
        invocation: &ToolInvocation,
        result: &dyn ToolOutput,
    ) -> Option<PostToolUsePayload> {
        self.write_stdin.post_tool_use_payload(invocation, result)
    }
}

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for AstralSendTaskInputHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(SEND_TASK_INPUT_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        astral_task_tool_spec(SEND_TASK_INPUT_TOOL_NAME)
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        self.write_stdin
            .handle(to_write_stdin_invocation(
                invocation,
                TaskIoMode::SendInput,
            )?)
            .await
    }
}

impl CoreToolRuntime for AstralSendTaskInputHandler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }

    fn pre_tool_use_payload(&self, _invocation: &ToolInvocation) -> Option<PreToolUsePayload> {
        None
    }

    fn post_tool_use_payload(
        &self,
        invocation: &ToolInvocation,
        result: &dyn ToolOutput,
    ) -> Option<PostToolUsePayload> {
        self.write_stdin.post_tool_use_payload(invocation, result)
    }
}

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for AstralListBackgroundTasksHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(LIST_BACKGROUND_TASKS_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        astral_task_tool_spec(LIST_BACKGROUND_TASKS_TOOL_NAME)
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        let tasks = invocation
            .session
            .services
            .unified_exec_manager
            .list_background_tasks()
            .await;
        let output = serde_json::to_string_pretty(&serde_json::json!({ "tasks": tasks })).map_err(
            |err| {
                FunctionCallError::RespondToModel(format!(
                    "failed to serialize background task list: {err}"
                ))
            },
        )?;
        Ok(boxed_tool_output(FunctionToolOutput::from_text(
            output,
            Some(true),
        )))
    }
}

impl CoreToolRuntime for AstralListBackgroundTasksHandler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }

    fn pre_tool_use_payload(&self, _invocation: &ToolInvocation) -> Option<PreToolUsePayload> {
        None
    }
}

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for AstralStopBackgroundTaskHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(STOP_BACKGROUND_TASK_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        astral_task_tool_spec(STOP_BACKGROUND_TASK_TOOL_NAME)
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        let ToolPayload::Function { arguments } = &invocation.payload else {
            return Err(FunctionCallError::RespondToModel(
                "StopBackgroundTask handler received unsupported payload".to_string(),
            ));
        };
        let task_id = parse_task_id(arguments, STOP_BACKGROUND_TASK_TOOL_NAME)?;
        let stopped = invocation
            .session
            .services
            .unified_exec_manager
            .terminate_process(task_id)
            .await
            .map_err(|err| {
                FunctionCallError::RespondToModel(format!("StopBackgroundTask failed: {err}"))
            })?;
        let output = serde_json::to_string_pretty(&serde_json::json!({
            "task_id": stopped.process_id,
            "status": "stopped",
            "command": stopped.command,
        }))
        .map_err(|err| {
            FunctionCallError::RespondToModel(format!(
                "failed to serialize stopped task result: {err}"
            ))
        })?;
        Ok(boxed_tool_output(FunctionToolOutput::from_text(
            output,
            Some(true),
        )))
    }
}

impl CoreToolRuntime for AstralStopBackgroundTaskHandler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }

    fn pre_tool_use_payload(&self, _invocation: &ToolInvocation) -> Option<PreToolUsePayload> {
        None
    }
}

enum TaskIoMode {
    ReadOutput,
    SendInput,
}

fn astral_task_tool_spec(name: &str) -> ToolSpec {
    let tool = astral_core_tool_by_name(name).unwrap_or_else(|| {
        panic!("astral core tool `{name}` should have a schema");
    });
    let parameters = parse_tool_input_schema_without_compaction(&tool.input_schema)
        .unwrap_or_else(|err| panic!("astral core tool `{name}` schema should parse: {err}"));

    ToolSpec::Function(ResponsesApiTool {
        name: tool.name,
        description: tool.description,
        strict: false,
        defer_loading: None,
        parameters,
        output_schema: None,
    })
}

fn to_write_stdin_invocation(
    mut invocation: ToolInvocation,
    mode: TaskIoMode,
) -> Result<ToolInvocation, FunctionCallError> {
    let ToolPayload::Function { arguments } = invocation.payload else {
        return Err(FunctionCallError::RespondToModel(
            "task IO handler received unsupported payload".to_string(),
        ));
    };

    invocation.tool_name = ToolName::plain("write_stdin");
    invocation.payload = ToolPayload::Function {
        arguments: rewrite_task_io_arguments(&arguments, mode)?,
    };
    Ok(invocation)
}

fn rewrite_task_io_arguments(
    arguments: &str,
    mode: TaskIoMode,
) -> Result<String, FunctionCallError> {
    let mut object = parse_arguments_object(arguments)?;
    let task_id = take_task_id(&mut object, "task IO")?;
    object.insert(
        "session_id".to_string(),
        Value::Number(Number::from(task_id)),
    );

    match mode {
        TaskIoMode::ReadOutput => {
            object.insert("chars".to_string(), Value::String(String::new()));
        }
        TaskIoMode::SendInput => {
            let input = object
                .remove("input")
                .or_else(|| object.remove("chars"))
                .and_then(|value| value.as_str().map(str::to_string))
                .ok_or_else(|| {
                    FunctionCallError::RespondToModel(
                        "SendTaskInput requires string field `input`".to_string(),
                    )
                })?;
            if input.is_empty() {
                return Err(FunctionCallError::RespondToModel(
                    "SendTaskInput `input` must not be empty; use ReadTaskOutput to poll output"
                        .to_string(),
                ));
            }
            object.insert("chars".to_string(), Value::String(input));
        }
    }

    serde_json::to_string(&object).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to serialize rewritten task IO arguments: {err}"
        ))
    })
}

fn parse_task_id(arguments: &str, tool_name: &str) -> Result<i32, FunctionCallError> {
    let mut object = parse_arguments_object(arguments)?;
    take_task_id(&mut object, tool_name)
}

fn parse_arguments_object(arguments: &str) -> Result<Map<String, Value>, FunctionCallError> {
    let value: Value = serde_json::from_str(arguments).map_err(|err| {
        FunctionCallError::RespondToModel(format!("failed to parse task tool arguments: {err}"))
    })?;
    let Value::Object(object) = value else {
        return Err(FunctionCallError::RespondToModel(
            "task tool arguments must be an object".to_string(),
        ));
    };
    Ok(object)
}

fn take_task_id(
    object: &mut Map<String, Value>,
    tool_name: &str,
) -> Result<i32, FunctionCallError> {
    let Some(value) = object
        .remove("task_id")
        .or_else(|| object.remove("session_id"))
        .or_else(|| object.remove("shell_id"))
    else {
        return Err(FunctionCallError::RespondToModel(format!(
            "{tool_name} requires `task_id`"
        )));
    };

    match value {
        Value::Number(number) => number.as_i64().and_then(|value| i32::try_from(value).ok()),
        Value::String(value) => value.trim().parse::<i32>().ok(),
        _ => None,
    }
    .ok_or_else(|| {
        FunctionCallError::RespondToModel(format!("{tool_name} task_id must be a numeric id"))
    })
}

#[cfg(test)]
#[path = "astral_background_tasks_tests.rs"]
mod tests;
