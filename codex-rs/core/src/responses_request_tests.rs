use super::*;
use codex_model_provider_info::ResponsesBuiltinToolsKeyword;
use codex_tools::AdditionalProperties;
use codex_tools::JsonSchema;
use codex_tools::JsonSchemaPrimitiveType;
use codex_tools::JsonSchemaType;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use std::collections::BTreeMap;

#[test]
fn local_compaction_projects_to_plain_user_input() {
    let projected = project_responses_input(vec![TranscriptItem::LocalCompaction {
        text: "local summary".to_string(),
    }]);

    assert_eq!(
        projected,
        vec![TranscriptItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "local summary".to_string(),
            }],
            phase: None,
        }]
    );
    let json = serde_json::to_value(projected).expect("serialize projected input");
    assert_eq!(json[0]["type"], "message");
    assert!(json[0].get("encrypted_content").is_none());
}

#[test]
fn native_compaction_remains_opaque_responses_input() {
    let native = TranscriptItem::Compaction {
        encrypted_content: "opaque".to_string(),
    };

    assert_eq!(project_responses_input(vec![native.clone()]), vec![native]);
}

#[test]
fn encrypted_state_reset_preserves_visible_history() {
    let visible_user = TranscriptItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "keep me".to_string(),
        }],
        phase: None,
    };
    let encrypted_reasoning = TranscriptItem::Reasoning {
        id: "reasoning-1".to_string(),
        summary: Vec::new(),
        content: None,
        encrypted_content: Some("opaque".to_string()),
        provider_metadata: None,
    };
    let native_compaction = TranscriptItem::Compaction {
        encrypted_content: "opaque-compaction".to_string(),
    };
    let local_compaction = TranscriptItem::LocalCompaction {
        text: "visible summary".to_string(),
    };

    let (cleaned, removed) = strip_responses_encrypted_state(vec![
        visible_user.clone(),
        encrypted_reasoning,
        native_compaction,
        local_compaction.clone(),
    ]);

    assert_eq!(removed, 2);
    assert_eq!(cleaned, vec![visible_user, local_compaction]);
}

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

fn provider_web_search() -> ToolSpec {
    ToolSpec::WebSearch {
        external_web_access: None,
        filters: None,
        user_location: None,
        search_context_size: None,
        search_content_types: None,
    }
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
fn codex_oauth_relaxes_only_incompatible_strict_tools() {
    let mut tools = vec![
        ToolSpec::Function(ResponsesApiTool {
            name: "Read".to_string(),
            description: String::new(),
            strict: true,
            defer_loading: None,
            parameters: strict_object_schema(vec!["file_path"]),
            output_schema: None,
        }),
        ToolSpec::Function(ResponsesApiTool {
            name: "Complete".to_string(),
            description: String::new(),
            strict: true,
            defer_loading: None,
            parameters: strict_object_schema(vec!["file_path", "limit"]),
            output_schema: None,
        }),
        ToolSpec::Function(ResponsesApiTool {
            name: "Empty".to_string(),
            description: String::new(),
            strict: true,
            defer_loading: None,
            parameters: JsonSchema::default(),
            output_schema: None,
        }),
    ];
    let mut expected = tools.clone();
    let ToolSpec::Function(incompatible) = &mut expected[0] else {
        unreachable!();
    };
    incompatible.strict = false;
    let ToolSpec::Function(empty) = &mut expected[2] else {
        unreachable!();
    };
    empty.strict = false;

    relax_incompatible_strict_tools(&mut tools);

    assert_eq!(tools, expected);
}

fn web_namespace() -> ToolSpec {
    ToolSpec::Namespace(ResponsesApiNamespace {
        name: "web".to_string(),
        description: String::new(),
        tools: vec![
            ResponsesApiNamespaceTool::Function(local_api_tool("search")),
            ResponsesApiNamespaceTool::Function(local_api_tool("fetch")),
        ],
    })
}

#[test]
fn codex_oauth_flattens_the_reserved_web_namespace() {
    let mut tools = vec![web_namespace(), local_function("plain")];

    flatten_reserved_codex_namespaces(&mut tools);

    assert_eq!(
        tools,
        vec![
            local_function("web__search"),
            local_function("web__fetch"),
            local_function("plain"),
        ]
    );
}

#[test]
fn all_prefers_local_tool_on_exact_name_collision() {
    let local = local_function("web_search");
    let tools = vec![local.clone(), provider_web_search()];

    assert_eq!(
        select_tools(
            &tools,
            &ResponsesBuiltinTools::All(ResponsesBuiltinToolsKeyword::All)
        ),
        vec![local]
    );
}

#[test]
fn explicit_selection_prefers_provider_tool_on_exact_name_collision() {
    let hosted = provider_web_search();
    let tools = vec![local_function("web_search"), hosted.clone()];

    assert_eq!(
        select_tools(
            &tools,
            &ResponsesBuiltinTools::Selected(vec!["web_search".to_string()])
        ),
        vec![hosted]
    );
}

#[test]
fn explicit_web_search_removes_only_the_matching_namespaced_local_tool() {
    let hosted = provider_web_search();
    let tools = vec![web_namespace(), hosted.clone()];
    let ToolSpec::Namespace(expected_namespace) = web_namespace() else {
        unreachable!();
    };
    let expected = ToolSpec::Namespace(ResponsesApiNamespace {
        tools: vec![expected_namespace.tools[1].clone()],
        ..expected_namespace
    });

    assert_eq!(
        select_tools(
            &tools,
            &ResponsesBuiltinTools::Selected(vec!["web_search".to_string()])
        ),
        vec![expected, hosted]
    );
}

#[test]
fn disabling_provider_tools_preserves_client_executed_tool_search() {
    let client_tool_search = ToolSpec::ToolSearch {
        execution: "client".to_string(),
        description: String::new(),
        parameters: JsonSchema::default(),
    };
    let tools = vec![client_tool_search.clone(), provider_web_search()];

    assert_eq!(
        select_tools(&tools, &ResponsesBuiltinTools::Selected(Vec::new())),
        vec![client_tool_search]
    );
}
