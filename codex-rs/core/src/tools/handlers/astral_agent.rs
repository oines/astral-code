use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::multi_agents_common::tool_output_code_mode_result;
use crate::tools::handlers::multi_agents_common::tool_output_json_text;
use crate::tools::handlers::multi_agents_common::tool_output_response_item;
use crate::tools::handlers::multi_agents_spec::SpawnAgentToolOptions;
use crate::tools::handlers::multi_agents_v2::SpawnAgentHandler;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_protocol::models::ResponseInputItem;
use codex_tools::AGENT_TOOL_NAME;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::astral_core_tool_by_name;
use codex_tools::parse_tool_input_schema_without_compaction;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value as JsonValue;
use serde_json::Value;
use serde_json::json;

pub struct AstralAgentHandler {
    spawn_agent: SpawnAgentHandler,
}

impl AstralAgentHandler {
    pub(crate) fn new(options: SpawnAgentToolOptions) -> Self {
        Self {
            spawn_agent: SpawnAgentHandler::new(options),
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for AstralAgentHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(AGENT_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        let tool = astral_core_tool_by_name(AGENT_TOOL_NAME).unwrap_or_else(|| {
            panic!("astral core tool `{AGENT_TOOL_NAME}` should have a schema");
        });
        let parameters = parse_tool_input_schema_without_compaction(&tool.input_schema)
            .unwrap_or_else(|err| {
                panic!("astral core tool `{AGENT_TOOL_NAME}` schema should parse: {err}");
            });
        let description = match self.spawn_agent.spec() {
            ToolSpec::Function(source_tool)
                if !source_tool.description.trim().is_empty()
                    && source_tool.description != tool.description =>
            {
                format!("{}\n\n{}", tool.description, source_tool.description)
            }
            _ => tool.description,
        };

        ToolSpec::Function(ResponsesApiTool {
            name: tool.name,
            description,
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
        let spawn_invocation = to_spawn_agent_invocation(invocation)?;
        let spawn_payload = spawn_invocation.payload.clone();
        let spawn_output = self.spawn_agent.handle(spawn_invocation).await?;
        Ok(boxed_tool_output(AstralAgentResult::from_spawn_result(
            spawn_output.code_mode_result(&spawn_payload),
        )?))
    }
}

impl CoreToolRuntime for AstralAgentHandler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }
}

#[derive(Deserialize)]
struct AstralAgentArgs {
    description: String,
    prompt: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    subagent_type: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<Value>,
    #[serde(default)]
    service_tier: Option<String>,
    #[serde(default)]
    fork_turns: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct AstralAgentResult {
    task_id: String,
    task_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    nickname: Option<String>,
}

impl AstralAgentResult {
    fn from_spawn_result(value: JsonValue) -> Result<Self, FunctionCallError> {
        let object = value.as_object().ok_or_else(|| {
            FunctionCallError::RespondToModel(
                "Agent failed to parse spawn_agent result object".to_string(),
            )
        })?;
        let task_name = object
            .get("task_name")
            .or_else(|| object.get("task_id"))
            .or_else(|| object.get("agentId"))
            .and_then(JsonValue::as_str)
            .ok_or_else(|| {
                FunctionCallError::RespondToModel(
                    "Agent result is missing task_name/task_id".to_string(),
                )
            })?
            .to_string();
        let nickname = object
            .get("nickname")
            .and_then(JsonValue::as_str)
            .map(str::to_string);
        Ok(Self {
            task_id: task_name.clone(),
            task_name,
            nickname,
        })
    }
}

impl ToolOutput for AstralAgentResult {
    fn log_preview(&self) -> String {
        tool_output_json_text(self, AGENT_TOOL_NAME)
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, payload: &ToolPayload) -> ResponseInputItem {
        tool_output_response_item(call_id, payload, self, Some(true), AGENT_TOOL_NAME)
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> JsonValue {
        tool_output_code_mode_result(self, AGENT_TOOL_NAME)
    }
}

fn to_spawn_agent_invocation(
    mut invocation: ToolInvocation,
) -> Result<ToolInvocation, FunctionCallError> {
    let ToolPayload::Function { arguments } = invocation.payload else {
        return Err(FunctionCallError::RespondToModel(
            "Agent handler received unsupported payload".to_string(),
        ));
    };

    invocation.tool_name = ToolName::plain("spawn_agent");
    invocation.payload = ToolPayload::Function {
        arguments: rewrite_agent_arguments(&arguments)?,
    };
    Ok(invocation)
}

fn rewrite_agent_arguments(arguments: &str) -> Result<String, FunctionCallError> {
    let args: AstralAgentArgs = parse_arguments(arguments)?;
    serde_json::to_string(&json!({
        "message": args.prompt,
        "task_name": task_name(args.name.as_deref().unwrap_or(&args.description)),
        "agent_type": args.subagent_type,
        "model": args.model,
        "reasoning_effort": args.reasoning_effort,
        "service_tier": args.service_tier,
        "fork_turns": args.fork_turns,
    }))
    .map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to serialize rewritten Agent arguments: {err}"
        ))
    })
}

fn task_name(source: &str) -> String {
    let mut task_name = String::new();
    let mut last_was_separator = false;
    for ch in source.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            task_name.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator && !task_name.is_empty() {
            task_name.push('_');
            last_was_separator = true;
        }
    }

    while task_name.ends_with('_') {
        task_name.pop();
    }

    if task_name.is_empty() || task_name == "root" {
        "agent".to_string()
    } else {
        task_name
    }
}

#[cfg(test)]
#[path = "astral_agent_tests.rs"]
mod tests;
