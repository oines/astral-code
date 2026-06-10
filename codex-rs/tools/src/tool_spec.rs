use crate::FreeformTool;
use crate::JsonSchema;
use crate::LoadableToolSpec;
use crate::ResponsesApiNamespace;
use crate::ResponsesApiNamespaceTool;
use crate::ResponsesApiTool;
use codex_agent_protocol::AgentTool;
use codex_protocol::config_types::WebSearchContextSize;
use codex_protocol::config_types::WebSearchFilters as ConfigWebSearchFilters;
use codex_protocol::config_types::WebSearchUserLocation as ConfigWebSearchUserLocation;
use codex_protocol::config_types::WebSearchUserLocationType;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use thiserror::Error;

/// When serialized as JSON, this produces a valid "Tool" in the OpenAI
/// Responses API.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type")]
pub enum ToolSpec {
    #[serde(rename = "function")]
    Function(ResponsesApiTool),
    #[serde(rename = "namespace")]
    Namespace(ResponsesApiNamespace),
    #[serde(rename = "tool_search")]
    ToolSearch {
        execution: String,
        description: String,
        parameters: JsonSchema,
    },
    #[serde(rename = "image_generation")]
    ImageGeneration { output_format: String },
    // TODO: Understand why we get an error on web_search although the API docs
    // say it's supported.
    // https://platform.openai.com/docs/guides/tools-web-search?api-mode=responses#:~:text=%7B%20type%3A%20%22web_search%22%20%7D%2C
    // The `external_web_access` field determines whether the web search is over
    // cached or live content.
    // https://platform.openai.com/docs/guides/tools-web-search#live-internet-access
    #[serde(rename = "web_search")]
    WebSearch {
        #[serde(skip_serializing_if = "Option::is_none")]
        external_web_access: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        filters: Option<ResponsesApiWebSearchFilters>,
        #[serde(skip_serializing_if = "Option::is_none")]
        user_location: Option<ResponsesApiWebSearchUserLocation>,
        #[serde(skip_serializing_if = "Option::is_none")]
        search_context_size: Option<WebSearchContextSize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        search_content_types: Option<Vec<String>>,
    },
    #[serde(rename = "custom")]
    Freeform(FreeformTool),
}

impl ToolSpec {
    pub fn name(&self) -> &str {
        match self {
            ToolSpec::Function(tool) => tool.name.as_str(),
            ToolSpec::Namespace(namespace) => namespace.name.as_str(),
            ToolSpec::ToolSearch { .. } => "tool_search",
            ToolSpec::ImageGeneration { .. } => "image_generation",
            ToolSpec::WebSearch { .. } => "web_search",
            ToolSpec::Freeform(tool) => tool.name.as_str(),
        }
    }
}

impl From<LoadableToolSpec> for ToolSpec {
    fn from(value: LoadableToolSpec) -> Self {
        match value {
            LoadableToolSpec::Function(tool) => ToolSpec::Function(tool),
            LoadableToolSpec::Namespace(namespace) => ToolSpec::Namespace(namespace),
        }
    }
}

/// Returns JSON values that are compatible with Function Calling in the
/// Responses API:
/// https://platform.openai.com/docs/guides/function-calling?api-mode=responses
pub fn create_tools_json_for_responses_api(
    tools: &[ToolSpec],
) -> Result<Vec<Value>, serde_json::Error> {
    let mut tools_json = Vec::new();

    for tool in tools {
        let json = serde_json::to_value(tool)?;
        tools_json.push(json);
    }

    Ok(tools_json)
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum AgentToolSpecError {
    #[error("tool {tool_name} input schema could not be serialized: {message}")]
    SchemaSerialization { tool_name: String, message: String },
    #[error("provider-neutral tools require unique names; {name} was declared more than once")]
    DuplicateToolName { name: String },
    #[error("{name} cannot be converted to a provider-neutral function tool")]
    UnsupportedTool { name: String },
}

/// Converts first-party tool specifications into provider-neutral function
/// tools for model adapters that do not understand Responses hosted tools.
pub fn create_agent_tools_for_provider_neutral_request(
    tools: &[ToolSpec],
) -> Result<Vec<AgentTool>, AgentToolSpecError> {
    let mut agent_tools = Vec::new();
    let mut seen_names = BTreeSet::new();

    for spec in tools {
        match spec {
            ToolSpec::Function(tool) => {
                push_agent_tool(
                    &mut agent_tools,
                    &mut seen_names,
                    responses_api_tool_to_agent_tool(tool, BTreeMap::new())?,
                )?;
            }
            ToolSpec::Namespace(namespace) => {
                for tool in &namespace.tools {
                    match tool {
                        ResponsesApiNamespaceTool::Function(tool) => {
                            let metadata = BTreeMap::from([
                                (
                                    "namespace".to_string(),
                                    Value::String(namespace.name.clone()),
                                ),
                                (
                                    "namespaceDescription".to_string(),
                                    Value::String(namespace.description.clone()),
                                ),
                            ]);
                            push_agent_tool(
                                &mut agent_tools,
                                &mut seen_names,
                                responses_api_tool_to_agent_tool(tool, metadata)?,
                            )?;
                        }
                    }
                }
            }
            ToolSpec::ToolSearch {
                execution,
                description,
                parameters,
            } => {
                let agent_tool = AgentTool {
                    name: spec.name().to_string(),
                    description: description.clone(),
                    input_schema: schema_to_value(spec.name(), parameters)?,
                    metadata: BTreeMap::from([(
                        "execution".to_string(),
                        Value::String(execution.clone()),
                    )]),
                };
                push_agent_tool(&mut agent_tools, &mut seen_names, agent_tool)?;
            }
            ToolSpec::ImageGeneration { .. }
            | ToolSpec::WebSearch { .. }
            | ToolSpec::Freeform(_) => {
                return Err(AgentToolSpecError::UnsupportedTool {
                    name: spec.name().to_string(),
                });
            }
        }
    }

    Ok(agent_tools)
}

fn responses_api_tool_to_agent_tool(
    tool: &ResponsesApiTool,
    mut metadata: BTreeMap<String, Value>,
) -> Result<AgentTool, AgentToolSpecError> {
    if tool.strict {
        metadata.insert("strict".to_string(), Value::Bool(true));
    }
    if let Some(defer_loading) = tool.defer_loading {
        metadata.insert("deferLoading".to_string(), Value::Bool(defer_loading));
    }
    if let Some(output_schema) = &tool.output_schema {
        metadata.insert("outputSchema".to_string(), output_schema.clone());
    }

    Ok(AgentTool {
        name: tool.name.clone(),
        description: tool.description.clone(),
        input_schema: schema_to_value(&tool.name, &tool.parameters)?,
        metadata,
    })
}

fn schema_to_value(tool_name: &str, parameters: &JsonSchema) -> Result<Value, AgentToolSpecError> {
    serde_json::to_value(parameters).map_err(|err| AgentToolSpecError::SchemaSerialization {
        tool_name: tool_name.to_string(),
        message: err.to_string(),
    })
}

fn push_agent_tool(
    agent_tools: &mut Vec<AgentTool>,
    seen_names: &mut BTreeSet<String>,
    tool: AgentTool,
) -> Result<(), AgentToolSpecError> {
    if !seen_names.insert(tool.name.clone()) {
        return Err(AgentToolSpecError::DuplicateToolName { name: tool.name });
    }

    agent_tools.push(tool);
    Ok(())
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ResponsesApiWebSearchFilters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_domains: Option<Vec<String>>,
}

impl From<ConfigWebSearchFilters> for ResponsesApiWebSearchFilters {
    fn from(filters: ConfigWebSearchFilters) -> Self {
        Self {
            allowed_domains: filters.allowed_domains,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ResponsesApiWebSearchUserLocation {
    #[serde(rename = "type")]
    pub r#type: WebSearchUserLocationType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

impl From<ConfigWebSearchUserLocation> for ResponsesApiWebSearchUserLocation {
    fn from(user_location: ConfigWebSearchUserLocation) -> Self {
        Self {
            r#type: user_location.r#type,
            country: user_location.country,
            region: user_location.region,
            city: user_location.city,
            timezone: user_location.timezone,
        }
    }
}

#[cfg(test)]
#[path = "tool_spec_tests.rs"]
mod tests;
