use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::PlanHandler;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_tools::ResponsesApiTool;
use codex_tools::TODO_WRITE_TOOL_NAME;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::astral_core_tool_by_name;
use codex_tools::parse_tool_input_schema_without_compaction;
use serde::Deserialize;
use serde_json::json;

const TODO_WRITE_SUCCESS_MESSAGE: &str = concat!(
    "Todos have been modified successfully. ",
    "Ensure that you continue to use the todo list to track your progress. ",
    "Please proceed with the current tasks if applicable"
);

pub struct AstralTodoWriteHandler {
    plan: PlanHandler,
}

impl AstralTodoWriteHandler {
    pub(crate) fn new() -> Self {
        Self { plan: PlanHandler }
    }

    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        self.plan.handle(to_plan_invocation(invocation)?).await?;
        Ok(boxed_tool_output(FunctionToolOutput::from_text(
            TODO_WRITE_SUCCESS_MESSAGE.to_string(),
            Some(true),
        )))
    }
}

impl ToolExecutor<ToolInvocation> for AstralTodoWriteHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TODO_WRITE_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        let tool = astral_core_tool_by_name(TODO_WRITE_TOOL_NAME).unwrap_or_else(|| {
            panic!("astral core tool `{TODO_WRITE_TOOL_NAME}` should have a schema");
        });
        let parameters = parse_tool_input_schema_without_compaction(&tool.input_schema)
            .unwrap_or_else(|err| {
                panic!("astral core tool `{TODO_WRITE_TOOL_NAME}` schema should parse: {err}");
            });

        ToolSpec::Function(ResponsesApiTool {
            name: tool.name,
            description: tool.description,
            strict: true,
            defer_loading: None,
            parameters,
            output_schema: None,
        })
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(invocation))
    }
}

impl CoreToolRuntime for AstralTodoWriteHandler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }
}

#[derive(Deserialize)]
struct TodoWriteArgs {
    todos: Vec<TodoWriteItem>,
    #[serde(default)]
    explanation: Option<String>,
}

#[derive(Deserialize)]
struct TodoWriteItem {
    content: String,
    status: String,
    #[serde(rename = "activeForm")]
    active_form: String,
}

fn to_plan_invocation(mut invocation: ToolInvocation) -> Result<ToolInvocation, FunctionCallError> {
    let ToolPayload::Function { arguments } = invocation.payload else {
        return Err(FunctionCallError::RespondToModel(
            "TodoWrite handler received unsupported payload".to_string(),
        ));
    };

    invocation.tool_name = ToolName::plain("update_plan");
    invocation.payload = ToolPayload::Function {
        arguments: rewrite_todo_write_arguments(&arguments)?,
    };
    Ok(invocation)
}

fn rewrite_todo_write_arguments(arguments: &str) -> Result<String, FunctionCallError> {
    let args: TodoWriteArgs = parse_arguments(arguments)?;
    let plan = args
        .todos
        .into_iter()
        .map(|todo| {
            json!({
                "step": todo.content,
                "status": todo.status,
                "activeForm": todo.active_form,
            })
        })
        .collect::<Vec<_>>();

    serde_json::to_string(&json!({
        "explanation": args.explanation,
        "plan": plan,
    }))
    .map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to serialize rewritten TodoWrite arguments: {err}"
        ))
    })
}

#[cfg(test)]
#[path = "astral_todo_write_tests.rs"]
mod tests;
