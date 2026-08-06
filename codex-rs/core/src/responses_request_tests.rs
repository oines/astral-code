use super::*;
use codex_model_provider_info::ResponsesBuiltinToolsKeyword;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;

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
