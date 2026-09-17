use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::get_context_remaining_spec::GET_CONTEXT_REMAINING_TOOL_NAME;
use crate::tools::handlers::get_context_remaining_spec::create_get_context_remaining_tool;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_tools::ToolName;
use codex_tools::ToolSpec;

pub struct GetContextRemainingHandler;

impl ToolExecutor<ToolInvocation> for GetContextRemainingHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(GET_CONTEXT_REMAINING_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        create_get_context_remaining_tool()
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(async move {
            if !matches!(invocation.payload, ToolPayload::Function { .. }) {
                return Err(FunctionCallError::RespondToModel(
                    "get_context_remaining handler received unsupported payload".to_string(),
                ));
            }
            let status = crate::session::context_window::context_window_token_status(
                invocation.session.as_ref(),
                invocation.turn.as_ref(),
            )
            .await;
            let message = match status.base_window_tokens_remaining {
                Some(tokens) => format!("You have {tokens} tokens left in this context window."),
                None => "The remaining context-window token budget is unavailable.".to_string(),
            };
            Ok(boxed_tool_output(FunctionToolOutput::from_text(
                message,
                Some(true),
            )))
        })
    }
}

impl CoreToolRuntime for GetContextRemainingHandler {}
