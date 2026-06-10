use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::WriteStdinHandler;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::PostToolUsePayload;
use crate::tools::registry::PreToolUsePayload;
use crate::tools::registry::ToolExecutor;
use codex_tools::MONITOR_TOOL_NAME;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::astral_core_tool_by_name;
use codex_tools::parse_tool_input_schema_without_compaction;
use serde_json::Map;
use serde_json::Value;

pub struct AstralMonitorHandler {
    write_stdin: WriteStdinHandler,
}

impl AstralMonitorHandler {
    pub(crate) fn new() -> Self {
        Self {
            write_stdin: WriteStdinHandler,
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for AstralMonitorHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(MONITOR_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        let tool = astral_core_tool_by_name(MONITOR_TOOL_NAME).unwrap_or_else(|| {
            panic!("astral core tool `{MONITOR_TOOL_NAME}` should have a schema");
        });
        let parameters = parse_tool_input_schema_without_compaction(&tool.input_schema)
            .unwrap_or_else(|err| {
                panic!("astral core tool `{MONITOR_TOOL_NAME}` schema should parse: {err}");
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
        self.write_stdin
            .handle(to_write_stdin_invocation(invocation)?)
            .await
    }
}

impl CoreToolRuntime for AstralMonitorHandler {
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

fn to_write_stdin_invocation(
    mut invocation: ToolInvocation,
) -> Result<ToolInvocation, FunctionCallError> {
    let ToolPayload::Function { arguments } = invocation.payload else {
        return Err(FunctionCallError::RespondToModel(
            "Monitor handler received unsupported payload".to_string(),
        ));
    };

    invocation.tool_name = ToolName::plain("write_stdin");
    invocation.payload = ToolPayload::Function {
        arguments: rewrite_monitor_arguments(&arguments)?,
    };
    Ok(invocation)
}

fn rewrite_monitor_arguments(arguments: &str) -> Result<String, FunctionCallError> {
    let value: Value = serde_json::from_str(arguments).map_err(|err| {
        FunctionCallError::RespondToModel(format!("failed to parse Monitor arguments: {err}"))
    })?;
    let Value::Object(mut object) = value else {
        return Err(FunctionCallError::RespondToModel(
            "Monitor arguments must be an object".to_string(),
        ));
    };

    move_field_if_absent(&mut object, "task_id", "session_id");
    move_field_if_absent(&mut object, "shell_id", "session_id");

    serde_json::to_string(&object).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to serialize rewritten Monitor arguments: {err}"
        ))
    })
}

fn move_field_if_absent(object: &mut Map<String, Value>, from: &str, to: &str) {
    if object.contains_key(to) {
        return;
    }
    if let Some(value) = object.remove(from) {
        object.insert(to.to_string(), value);
    }
}
