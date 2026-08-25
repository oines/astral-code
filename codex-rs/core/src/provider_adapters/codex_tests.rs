use super::*;
use codex_protocol::config_types::ReasoningSummary;
use codex_tools::ResponsesApiNamespace;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;

fn local_api_tool(name: &str) -> ResponsesApiTool {
    ResponsesApiTool {
        name: name.to_string(),
        description: String::new(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::default(),
        output_schema: None,
    }
}

fn local_function(name: &str) -> ToolSpec {
    ToolSpec::Function(local_api_tool(name))
}

fn strict_object_schema(required: Vec<&str>) -> JsonSchema {
    JsonSchema {
        schema_type: Some(JsonSchemaType::Single(JsonSchemaPrimitiveType::Object)),
        properties: Some(BTreeMap::from([
            ("file_path".to_string(), JsonSchema::string(None)),
            ("limit".to_string(), JsonSchema::integer(None)),
        ])),
        required: Some(required.into_iter().map(str::to_string).collect()),
        additional_properties: Some(AdditionalProperties::Boolean(false)),
        ..Default::default()
    }
}

#[test]
fn codex_relaxes_only_incompatible_strict_tools() {
    let mut tools = vec![
        ToolSpec::Function(ResponsesApiTool {
            name: "Read".to_string(),
            strict: true,
            parameters: strict_object_schema(vec!["file_path"]),
            ..local_api_tool("unused")
        }),
        ToolSpec::Function(ResponsesApiTool {
            name: "Complete".to_string(),
            strict: true,
            parameters: strict_object_schema(vec!["file_path", "limit"]),
            ..local_api_tool("unused")
        }),
    ];

    relax_incompatible_strict_tools(&mut tools);

    assert_eq!(
        tools,
        vec![
            ToolSpec::Function(ResponsesApiTool {
                name: "Read".to_string(),
                strict: false,
                parameters: strict_object_schema(vec!["file_path"]),
                ..local_api_tool("unused")
            }),
            ToolSpec::Function(ResponsesApiTool {
                name: "Complete".to_string(),
                strict: true,
                parameters: strict_object_schema(vec!["file_path", "limit"]),
                ..local_api_tool("unused")
            }),
        ]
    );
}

#[test]
fn codex_flattens_the_reserved_web_namespace() {
    let mut prompt = Prompt {
        tools: vec![
            ToolSpec::Namespace(ResponsesApiNamespace {
                name: "web".to_string(),
                description: String::new(),
                tools: vec![
                    ResponsesApiNamespaceTool::Function(local_api_tool("search")),
                    ResponsesApiNamespaceTool::Function(local_api_tool("fetch")),
                ],
            }),
            local_function("plain"),
        ],
        ..Prompt::default()
    };

    flatten_reserved_namespaces(&mut prompt);

    assert_eq!(
        prompt.tools,
        vec![
            local_function("web__search"),
            local_function("web__fetch"),
            local_function("plain"),
        ]
    );
}

#[test]
fn responses_lite_sets_all_turns_reasoning_and_disables_parallel_tools() {
    let prompt = Prompt {
        parallel_tool_calls: true,
        ..Prompt::default()
    };
    let mut model_info = codex_models_manager::model_info::model_info_from_slug("gpt-test");
    model_info.use_responses_lite = true;

    let request = build_responses_request(ResponsesRequestParams {
        prompt: &prompt,
        model_info: &model_info,
        effort: None,
        summary: ReasoningSummary::None,
        service_tier: None,
        prompt_cache_key: "cache-key".to_string(),
    })
    .expect("Lite request should build");

    assert!(!request.parallel_tool_calls);
    assert_eq!(
        request.reasoning.and_then(|reasoning| reasoning.context),
        Some(ReasoningContext::AllTurns)
    );
}
