use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::RequestPermissionsHandler;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_tools::REQUEST_PERMISSIONS_TOOL_NAME;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::astral_core_tool_by_name;
use codex_tools::parse_tool_input_schema_without_compaction;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

pub struct AstralRequestPermissionsHandler {
    request_permissions: RequestPermissionsHandler,
}

impl AstralRequestPermissionsHandler {
    pub(crate) fn new() -> Self {
        Self {
            request_permissions: RequestPermissionsHandler,
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for AstralRequestPermissionsHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(REQUEST_PERMISSIONS_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        let tool = astral_core_tool_by_name(REQUEST_PERMISSIONS_TOOL_NAME).unwrap_or_else(|| {
            panic!("astral core tool `{REQUEST_PERMISSIONS_TOOL_NAME}` should have a schema");
        });
        let parameters = parse_tool_input_schema_without_compaction(&tool.input_schema)
            .unwrap_or_else(|err| {
                panic!(
                    "astral core tool `{REQUEST_PERMISSIONS_TOOL_NAME}` schema should parse: {err}"
                );
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
        self.request_permissions
            .handle(to_request_permissions_invocation(invocation)?)
            .await
    }
}

impl CoreToolRuntime for AstralRequestPermissionsHandler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }
}

#[derive(Deserialize)]
struct AstralRequestPermissionsArgs {
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    input: Value,
    #[serde(default)]
    permissions: Option<Value>,
    #[serde(default, rename = "environment_id", alias = "environmentId")]
    environment_id: Option<String>,
}

fn to_request_permissions_invocation(
    mut invocation: ToolInvocation,
) -> Result<ToolInvocation, FunctionCallError> {
    let ToolPayload::Function { arguments } = invocation.payload else {
        return Err(FunctionCallError::RespondToModel(
            "RequestPermissions handler received unsupported payload".to_string(),
        ));
    };

    invocation.tool_name = ToolName::plain("request_permissions");
    invocation.payload = ToolPayload::Function {
        arguments: rewrite_request_permissions_arguments(&arguments)?,
    };
    Ok(invocation)
}

fn rewrite_request_permissions_arguments(arguments: &str) -> Result<String, FunctionCallError> {
    let args: AstralRequestPermissionsArgs = parse_arguments(arguments)?;
    let permissions = args
        .permissions
        .or_else(|| args.input.get("additional_permissions").cloned())
        .or_else(|| args.input.get("additionalPermissions").cloned())
        .or_else(|| args.input.get("permissions").cloned())
        .ok_or_else(|| {
            FunctionCallError::RespondToModel(
                "RequestPermissions requires `permissions`, or an `input` object containing permissions"
                    .to_string(),
            )
        })?;

    serde_json::to_string(&json!({
        "environment_id": args.environment_id,
        "reason": args.reason,
        "permissions": permissions,
    }))
    .map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to serialize rewritten RequestPermissions arguments: {err}"
        ))
    })
}
