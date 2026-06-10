use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::ToolSearchHandler;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_protocol::models::SearchToolCallParams;
use codex_tools::ResponsesApiTool;
use codex_tools::TOOL_SEARCH_FLAVOR_TOOL_NAME;
use codex_tools::TOOL_SEARCH_TOOL_NAME;
use codex_tools::ToolName;
use codex_tools::ToolSearchInfo;
use codex_tools::ToolSpec;
use codex_tools::astral_core_tool_by_name;
use codex_tools::parse_tool_input_schema_without_compaction;
use serde::Deserialize;

pub struct AstralToolSearchHandler {
    tool_search: ToolSearchHandler,
}

impl AstralToolSearchHandler {
    pub(crate) fn new(search_infos: Vec<ToolSearchInfo>) -> Self {
        Self {
            tool_search: ToolSearchHandler::new(search_infos),
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for AstralToolSearchHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TOOL_SEARCH_FLAVOR_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        let tool = astral_core_tool_by_name(TOOL_SEARCH_FLAVOR_TOOL_NAME).unwrap_or_else(|| {
            panic!("astral core tool `{TOOL_SEARCH_FLAVOR_TOOL_NAME}` should have a schema");
        });
        let parameters = parse_tool_input_schema_without_compaction(&tool.input_schema)
            .unwrap_or_else(|err| {
                panic!(
                    "astral core tool `{TOOL_SEARCH_FLAVOR_TOOL_NAME}` schema should parse: {err}"
                );
            });
        let source_spec = self.tool_search.spec();
        let description =
            append_source_description(tool.description, tool_search_description(&source_spec));

        ToolSpec::Function(ResponsesApiTool {
            name: tool.name,
            description,
            strict: false,
            defer_loading: None,
            parameters,
            output_schema: None,
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        self.tool_search
            .handle(to_tool_search_invocation(invocation)?)
            .await
    }
}

impl CoreToolRuntime for AstralToolSearchHandler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(
            payload,
            ToolPayload::Function { .. } | ToolPayload::ToolSearch { .. }
        )
    }
}

#[derive(Deserialize)]
struct AstralToolSearchArgs {
    query: String,
    #[serde(default)]
    max_results: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

fn to_tool_search_invocation(
    mut invocation: ToolInvocation,
) -> Result<ToolInvocation, FunctionCallError> {
    let arguments = match invocation.payload {
        ToolPayload::Function { arguments } => {
            let args: AstralToolSearchArgs = parse_arguments(&arguments)?;
            SearchToolCallParams {
                query: args.query,
                limit: args.max_results.or(args.limit),
            }
        }
        ToolPayload::ToolSearch { arguments } => arguments,
        ToolPayload::Custom { .. } => {
            return Err(FunctionCallError::RespondToModel(
                "ToolSearch handler received unsupported payload".to_string(),
            ));
        }
    };

    invocation.tool_name = ToolName::plain(TOOL_SEARCH_TOOL_NAME);
    invocation.payload = ToolPayload::ToolSearch { arguments };
    Ok(invocation)
}

fn tool_search_description(spec: &ToolSpec) -> Option<&str> {
    let ToolSpec::ToolSearch { description, .. } = spec else {
        return None;
    };
    Some(description.as_str())
}

fn append_source_description(description: String, source_description: Option<&str>) -> String {
    let Some(source_description) = source_description else {
        return description;
    };

    if source_description.trim().is_empty() || source_description == description {
        description
    } else {
        format!("{description}\n\n{source_description}")
    }
}
