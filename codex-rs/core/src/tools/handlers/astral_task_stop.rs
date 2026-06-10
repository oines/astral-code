use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::multi_agents_v2::InterruptAgentHandler;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_tools::ResponsesApiTool;
use codex_tools::TASK_STOP_TOOL_NAME;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::astral_core_tool_by_name;
use codex_tools::parse_tool_input_schema_without_compaction;
use serde::Deserialize;
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
        self.interrupt_agent
            .handle(to_interrupt_agent_invocation(invocation)?)
            .await
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
    let args: AstralTaskStopArgs = parse_arguments(arguments)?;
    let target = args.task_id.or(args.shell_id).ok_or_else(|| {
        FunctionCallError::RespondToModel(
            "TaskStop requires `task_id` or `shell_id` to identify the target".to_string(),
        )
    })?;

    serde_json::to_string(&json!({ "target": target })).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to serialize rewritten TaskStop arguments: {err}"
        ))
    })
}
