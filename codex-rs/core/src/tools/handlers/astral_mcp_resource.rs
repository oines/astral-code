use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::ListMcpResourcesHandler;
use crate::tools::handlers::ReadMcpResourceHandler;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_tools::LIST_MCP_RESOURCES_TOOL_NAME;
use codex_tools::READ_MCP_RESOURCE_TOOL_NAME;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::astral_core_tool_by_name;
use codex_tools::parse_tool_input_schema_without_compaction;

pub struct AstralListMcpResourcesHandler {
    list_resources: ListMcpResourcesHandler,
}

impl AstralListMcpResourcesHandler {
    pub(crate) fn new() -> Self {
        Self {
            list_resources: ListMcpResourcesHandler,
        }
    }

    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        self.list_resources
            .handle(to_internal_invocation(invocation, "list_mcp_resources")?)
            .await
    }
}

impl ToolExecutor<ToolInvocation> for AstralListMcpResourcesHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(LIST_MCP_RESOURCES_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        astral_mcp_resource_tool_spec(LIST_MCP_RESOURCES_TOOL_NAME)
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(invocation))
    }
}

impl CoreToolRuntime for AstralListMcpResourcesHandler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }
}

pub struct AstralReadMcpResourceHandler {
    read_resource: ReadMcpResourceHandler,
}

impl AstralReadMcpResourceHandler {
    pub(crate) fn new() -> Self {
        Self {
            read_resource: ReadMcpResourceHandler,
        }
    }

    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        self.read_resource
            .handle(to_internal_invocation(invocation, "read_mcp_resource")?)
            .await
    }
}

impl ToolExecutor<ToolInvocation> for AstralReadMcpResourceHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(READ_MCP_RESOURCE_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        astral_mcp_resource_tool_spec(READ_MCP_RESOURCE_TOOL_NAME)
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(invocation))
    }
}

impl CoreToolRuntime for AstralReadMcpResourceHandler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }
}

fn astral_mcp_resource_tool_spec(tool_name: &str) -> ToolSpec {
    let tool = astral_core_tool_by_name(tool_name).unwrap_or_else(|| {
        panic!("astral core tool `{tool_name}` should have a schema");
    });
    let parameters = parse_tool_input_schema_without_compaction(&tool.input_schema)
        .unwrap_or_else(|err| panic!("astral core tool `{tool_name}` schema should parse: {err}"));

    ToolSpec::Function(ResponsesApiTool {
        name: tool.name,
        description: tool.description,
        strict: false,
        defer_loading: None,
        parameters,
        output_schema: None,
    })
}

fn to_internal_invocation(
    mut invocation: ToolInvocation,
    internal_tool_name: &'static str,
) -> Result<ToolInvocation, FunctionCallError> {
    if !matches!(invocation.payload, ToolPayload::Function { .. }) {
        return Err(FunctionCallError::RespondToModel(format!(
            "{internal_tool_name} received unsupported payload"
        )));
    }

    invocation.tool_name = ToolName::plain(internal_tool_name);
    Ok(invocation)
}
