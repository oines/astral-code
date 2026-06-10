use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::multi_agents_v2::SendMessageHandler;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_tools::ResponsesApiTool;
use codex_tools::SEND_MESSAGE_TOOL_NAME;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::astral_core_tool_by_name;
use codex_tools::parse_tool_input_schema_without_compaction;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

pub struct AstralSendMessageHandler {
    send_message: SendMessageHandler,
}

impl AstralSendMessageHandler {
    pub(crate) fn new() -> Self {
        Self {
            send_message: SendMessageHandler,
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for AstralSendMessageHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(SEND_MESSAGE_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        let tool = astral_core_tool_by_name(SEND_MESSAGE_TOOL_NAME).unwrap_or_else(|| {
            panic!("astral core tool `{SEND_MESSAGE_TOOL_NAME}` should have a schema");
        });
        let parameters = parse_tool_input_schema_without_compaction(&tool.input_schema)
            .unwrap_or_else(|err| {
                panic!("astral core tool `{SEND_MESSAGE_TOOL_NAME}` schema should parse: {err}");
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
        self.send_message
            .handle(to_send_message_invocation(invocation)?)
            .await
    }
}

impl CoreToolRuntime for AstralSendMessageHandler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }
}

#[derive(Deserialize)]
struct AstralSendMessageArgs {
    to: String,
    message: Value,
}

fn to_send_message_invocation(
    mut invocation: ToolInvocation,
) -> Result<ToolInvocation, FunctionCallError> {
    let ToolPayload::Function { arguments } = invocation.payload else {
        return Err(FunctionCallError::RespondToModel(
            "SendMessage handler received unsupported payload".to_string(),
        ));
    };

    invocation.tool_name = ToolName::plain("send_message");
    invocation.payload = ToolPayload::Function {
        arguments: rewrite_send_message_arguments(&arguments)?,
    };
    Ok(invocation)
}

fn rewrite_send_message_arguments(arguments: &str) -> Result<String, FunctionCallError> {
    let args: AstralSendMessageArgs = parse_arguments(arguments)?;
    let message = match args.message {
        Value::String(message) => message,
        other => other.to_string(),
    };

    serde_json::to_string(&json!({
        "target": args.to,
        "message": message,
    }))
    .map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to serialize rewritten SendMessage arguments: {err}"
        ))
    })
}
