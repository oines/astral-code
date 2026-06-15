use super::AgentToolSpecError;
use super::ResponsesApiNamespace;
use super::ResponsesApiWebSearchFilters;
use super::ResponsesApiWebSearchUserLocation;
use super::ToolSpec;
use crate::AdditionalProperties;
use crate::FreeformTool;
use crate::FreeformToolFormat;
use crate::JsonSchema;
use crate::ResponsesApiNamespaceTool;
use crate::ResponsesApiTool;
use crate::create_agent_tools_for_provider_neutral_request;
use crate::create_tools_json_for_responses_api;
use crate::provider_neutral_tool_name_for_tool_name;
use codex_agent_protocol::AgentTool;
use codex_protocol::ToolName;
use codex_protocol::config_types::WebSearchContextSize;
use codex_protocol::config_types::WebSearchFilters as ConfigWebSearchFilters;
use codex_protocol::config_types::WebSearchUserLocation as ConfigWebSearchUserLocation;
use codex_protocol::config_types::WebSearchUserLocationType;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn tool_spec_name_covers_all_variants() {
    assert_eq!(
        ToolSpec::Function(ResponsesApiTool {
            name: "lookup_order".to_string(),
            description: "Look up an order".to_string(),
            strict: false,
            defer_loading: None,
            parameters: JsonSchema::object(
                BTreeMap::new(),
                /*required*/ None,
                /*additional_properties*/ None
            ),
            output_schema: None,
        })
        .name(),
        "lookup_order"
    );
    assert_eq!(
        ToolSpec::Namespace(ResponsesApiNamespace {
            name: "mcp__demo__".to_string(),
            description: "Demo tools".to_string(),
            tools: Vec::new(),
        })
        .name(),
        "mcp__demo__"
    );
    assert_eq!(
        ToolSpec::ToolSearch {
            execution: "sync".to_string(),
            description: "Search for tools".to_string(),
            parameters: JsonSchema::object(
                BTreeMap::new(),
                /*required*/ None,
                /*additional_properties*/ None
            ),
        }
        .name(),
        "tool_search"
    );
    assert_eq!(
        ToolSpec::ImageGeneration {
            output_format: "png".to_string(),
        }
        .name(),
        "image_generation"
    );
    assert_eq!(
        ToolSpec::WebSearch {
            external_web_access: Some(true),
            filters: None,
            user_location: None,
            search_context_size: None,
            search_content_types: None,
        }
        .name(),
        "web_search"
    );
    assert_eq!(
        ToolSpec::Freeform(FreeformTool {
            name: "exec".to_string(),
            description: "Run a command".to_string(),
            format: FreeformToolFormat {
                r#type: "grammar".to_string(),
                syntax: "lark".to_string(),
                definition: "start: \"exec\"".to_string(),
            },
        })
        .name(),
        "exec"
    );
}

#[test]
fn web_search_config_converts_to_responses_api_types() {
    assert_eq!(
        ResponsesApiWebSearchFilters::from(ConfigWebSearchFilters {
            allowed_domains: Some(vec!["example.com".to_string()]),
        }),
        ResponsesApiWebSearchFilters {
            allowed_domains: Some(vec!["example.com".to_string()]),
        }
    );
    assert_eq!(
        ResponsesApiWebSearchUserLocation::from(ConfigWebSearchUserLocation {
            r#type: WebSearchUserLocationType::Approximate,
            country: Some("US".to_string()),
            region: Some("California".to_string()),
            city: Some("San Francisco".to_string()),
            timezone: Some("America/Los_Angeles".to_string()),
        }),
        ResponsesApiWebSearchUserLocation {
            r#type: WebSearchUserLocationType::Approximate,
            country: Some("US".to_string()),
            region: Some("California".to_string()),
            city: Some("San Francisco".to_string()),
            timezone: Some("America/Los_Angeles".to_string()),
        }
    );
}

#[test]
fn create_tools_json_for_responses_api_includes_top_level_name() {
    assert_eq!(
        create_tools_json_for_responses_api(&[ToolSpec::Function(ResponsesApiTool {
            name: "demo".to_string(),
            description: "A demo tool".to_string(),
            strict: false,
            defer_loading: None,
            parameters: JsonSchema::object(
                BTreeMap::from([("foo".to_string(), JsonSchema::string(/*description*/ None),)]),
                /*required*/ None,
                /*additional_properties*/ None
            ),
            output_schema: None,
        })])
        .expect("serialize tools"),
        vec![json!({
            "type": "function",
            "name": "demo",
            "description": "A demo tool",
            "strict": false,
            "parameters": {
                "type": "object",
                "properties": {
                    "foo": { "type": "string" }
                },
            },
        })]
    );
}

#[test]
fn create_agent_tools_converts_function_tools() {
    assert_eq!(
        create_agent_tools_for_provider_neutral_request(&[ToolSpec::Function(ResponsesApiTool {
            name: "demo".to_string(),
            description: "A demo tool".to_string(),
            strict: true,
            defer_loading: Some(true),
            parameters: JsonSchema::object(
                BTreeMap::from([("foo".to_string(), JsonSchema::string(/*description*/ None),)]),
                Some(vec!["foo".to_string()]),
                Some(AdditionalProperties::Boolean(false))
            ),
            output_schema: Some(json!({ "type": "object" })),
        })])
        .expect("convert tools"),
        vec![AgentTool {
            name: "demo".to_string(),
            description: "A demo tool".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "foo": { "type": "string" }
                },
                "required": ["foo"],
                "additionalProperties": false,
            }),
            metadata: BTreeMap::from([
                ("deferLoading".to_string(), json!(true)),
                ("outputSchema".to_string(), json!({ "type": "object" })),
                ("strict".to_string(), json!(true)),
            ]),
        }]
    );
}

#[test]
fn create_agent_tools_flattens_namespace_tools() {
    assert_eq!(
        create_agent_tools_for_provider_neutral_request(&[ToolSpec::Namespace(
            ResponsesApiNamespace {
                name: "mcp__demo__".to_string(),
                description: "Demo tools".to_string(),
                tools: vec![ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                    name: "lookup_order".to_string(),
                    description: "Look up an order".to_string(),
                    strict: false,
                    defer_loading: None,
                    parameters: JsonSchema::object(
                        BTreeMap::from([(
                            "order_id".to_string(),
                            JsonSchema::string(/*description*/ None),
                        )]),
                        /*required*/ None,
                        /*additional_properties*/ None,
                    ),
                    output_schema: None,
                })],
            }
        )])
        .expect("convert tools"),
        vec![AgentTool {
            name: "mcp__demo____lookup_order".to_string(),
            description: "Look up an order".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "order_id": { "type": "string" },
                },
            }),
            metadata: BTreeMap::from([
                ("namespace".to_string(), json!("mcp__demo__")),
                ("namespaceDescription".to_string(), json!("Demo tools")),
                ("originalName".to_string(), json!("lookup_order")),
            ]),
        }]
    );
}

#[test]
fn provider_neutral_tool_name_preserves_namespace_identity() {
    assert_eq!(
        provider_neutral_tool_name_for_tool_name(&ToolName::namespaced(
            "mcp__codex_apps__gmail",
            "_send_email"
        )),
        "mcp__codex_apps__gmail___send_email"
    );
    assert_eq!(
        provider_neutral_tool_name_for_tool_name(&ToolName::plain("tool_search")),
        "tool_search"
    );
}

#[test]
fn create_agent_tools_converts_tool_search() {
    assert_eq!(
        create_agent_tools_for_provider_neutral_request(&[ToolSpec::ToolSearch {
            execution: "sync".to_string(),
            description: "Search app tools".to_string(),
            parameters: JsonSchema::object(
                BTreeMap::from([(
                    "query".to_string(),
                    JsonSchema::string(Some("Tool search query".to_string()),),
                )]),
                Some(vec!["query".to_string()]),
                Some(AdditionalProperties::Boolean(false))
            ),
        }])
        .expect("convert tools"),
        vec![AgentTool {
            name: "tool_search".to_string(),
            description: "Search app tools".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Tool search query",
                    }
                },
                "required": ["query"],
                "additionalProperties": false,
            }),
            metadata: BTreeMap::from([("execution".to_string(), json!("sync"))]),
        }]
    );
}

#[test]
fn create_agent_tools_converts_apply_patch_freeform_tool() {
    assert_eq!(
        create_agent_tools_for_provider_neutral_request(&[ToolSpec::Freeform(FreeformTool {
            name: "apply_patch".to_string(),
            description: "Apply a patch".to_string(),
            format: FreeformToolFormat {
                r#type: "grammar".to_string(),
                syntax: "lark".to_string(),
                definition: "start: \"patch\"".to_string(),
            },
        })])
        .expect("apply_patch freeform should convert to provider-neutral function"),
        vec![AgentTool {
            name: "apply_patch".to_string(),
            description: "Apply a patch".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string",
                        "description": "The raw apply_patch patch body.",
                    },
                },
                "required": ["input"],
                "additionalProperties": false,
            }),
            metadata: BTreeMap::new(),
        }]
    );
}

#[test]
fn create_agent_tools_rejects_duplicate_names() {
    assert_eq!(
        create_agent_tools_for_provider_neutral_request(&[
            ToolSpec::Function(ResponsesApiTool {
                name: "demo".to_string(),
                description: "First demo tool".to_string(),
                strict: false,
                defer_loading: None,
                parameters: JsonSchema::object(
                    BTreeMap::new(),
                    /*required*/ None,
                    /*additional_properties*/ None
                ),
                output_schema: None,
            }),
            ToolSpec::Function(ResponsesApiTool {
                name: "demo".to_string(),
                description: "Second demo tool".to_string(),
                strict: false,
                defer_loading: None,
                parameters: JsonSchema::object(
                    BTreeMap::new(),
                    /*required*/ None,
                    /*additional_properties*/ None
                ),
                output_schema: None,
            }),
        ]),
        Err(AgentToolSpecError::DuplicateToolName {
            name: "demo".to_string(),
        })
    );
}

#[test]
fn create_agent_tools_rejects_hosted_and_freeform_tools() {
    assert_eq!(
        create_agent_tools_for_provider_neutral_request(&[ToolSpec::WebSearch {
            external_web_access: Some(true),
            filters: None,
            user_location: None,
            search_context_size: None,
            search_content_types: None,
        }]),
        Err(AgentToolSpecError::UnsupportedTool {
            name: "web_search".to_string(),
        })
    );
    assert_eq!(
        create_agent_tools_for_provider_neutral_request(&[ToolSpec::Freeform(FreeformTool {
            name: "exec".to_string(),
            description: "Run a command".to_string(),
            format: FreeformToolFormat {
                r#type: "grammar".to_string(),
                syntax: "lark".to_string(),
                definition: "start: \"exec\"".to_string(),
            },
        })]),
        Err(AgentToolSpecError::UnsupportedTool {
            name: "exec".to_string(),
        })
    );
}

#[test]
fn namespace_tool_spec_serializes_expected_wire_shape() {
    assert_eq!(
        serde_json::to_value(ToolSpec::Namespace(ResponsesApiNamespace {
            name: "mcp__demo__".to_string(),
            description: "Demo tools".to_string(),
            tools: vec![ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                name: "lookup_order".to_string(),
                description: "Look up an order".to_string(),
                strict: false,
                defer_loading: None,
                parameters: JsonSchema::object(
                    BTreeMap::from([(
                        "order_id".to_string(),
                        JsonSchema::string(/*description*/ None),
                    )]),
                    /*required*/ None,
                    /*additional_properties*/ None,
                ),
                output_schema: None,
            })],
        }))
        .expect("serialize namespace tool"),
        json!({
            "type": "namespace",
            "name": "mcp__demo__",
            "description": "Demo tools",
            "tools": [
                {
                    "type": "function",
                    "name": "lookup_order",
                    "description": "Look up an order",
                    "strict": false,
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "order_id": { "type": "string" },
                        },
                    },
                },
            ],
        })
    );
}

#[test]
fn web_search_tool_spec_serializes_expected_wire_shape() {
    assert_eq!(
        serde_json::to_value(ToolSpec::WebSearch {
            external_web_access: Some(true),
            filters: Some(ResponsesApiWebSearchFilters {
                allowed_domains: Some(vec!["example.com".to_string()]),
            }),
            user_location: Some(ResponsesApiWebSearchUserLocation {
                r#type: WebSearchUserLocationType::Approximate,
                country: Some("US".to_string()),
                region: Some("California".to_string()),
                city: Some("San Francisco".to_string()),
                timezone: Some("America/Los_Angeles".to_string()),
            }),
            search_context_size: Some(WebSearchContextSize::High),
            search_content_types: Some(vec!["text".to_string(), "image".to_string()]),
        })
        .expect("serialize web_search"),
        json!({
            "type": "web_search",
            "external_web_access": true,
            "filters": {
                "allowed_domains": ["example.com"],
            },
            "user_location": {
                "type": "approximate",
                "country": "US",
                "region": "California",
                "city": "San Francisco",
                "timezone": "America/Los_Angeles",
            },
            "search_context_size": "high",
            "search_content_types": ["text", "image"],
        })
    );
}

#[test]
fn tool_search_tool_spec_serializes_expected_wire_shape() {
    assert_eq!(
        serde_json::to_value(ToolSpec::ToolSearch {
            execution: "sync".to_string(),
            description: "Search app tools".to_string(),
            parameters: JsonSchema::object(
                BTreeMap::from([(
                    "query".to_string(),
                    JsonSchema::string(Some("Tool search query".to_string()),),
                )]),
                Some(vec!["query".to_string()]),
                Some(AdditionalProperties::Boolean(false))
            ),
        })
        .expect("serialize tool_search"),
        json!({
            "type": "tool_search",
            "execution": "sync",
            "description": "Search app tools",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Tool search query",
                    }
                },
                "required": ["query"],
                "additionalProperties": false,
            },
        })
    );
}
