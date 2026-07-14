use codex_code_mode::ToolDefinition as CodeModeToolDefinition;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use std::collections::BTreeMap;

pub(crate) fn create_code_mode_tool(
    enabled_tools: &[CodeModeToolDefinition],
    deferred_tools: &[CodeModeToolDefinition],
    namespace_descriptions: &BTreeMap<String, codex_code_mode::ToolNamespaceDescription>,
    code_mode_only: bool,
) -> ToolSpec {
    ToolSpec::Function(ResponsesApiTool {
        name: codex_code_mode::PUBLIC_TOOL_NAME.to_string(),
        description: codex_code_mode::build_exec_tool_description(
            enabled_tools,
            deferred_tools,
            namespace_descriptions,
            code_mode_only,
        ),
        strict: false,
        parameters: JsonSchema::object(
            BTreeMap::from([(
                "input".to_string(),
                JsonSchema::string(Some("JavaScript source to execute.".to_string())),
            )]),
            Some(vec!["input".to_string()]),
            Some(false.into()),
        ),
        output_schema: None,
        defer_loading: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_tools::ToolName;
    use pretty_assertions::assert_eq;

    #[test]
    fn create_code_mode_tool_matches_expected_spec() {
        let enabled_tools = vec![codex_code_mode::ToolDefinition {
            name: "update_plan".to_string(),
            tool_name: ToolName::plain("update_plan"),
            description: "Update the plan".to_string(),
            kind: codex_code_mode::CodeModeToolKind::Function,
            input_schema: None,
            output_schema: None,
        }];

        assert_eq!(
            create_code_mode_tool(
                &enabled_tools,
                &[],
                &BTreeMap::new(),
                /*code_mode_only*/ true,
            ),
            ToolSpec::Function(ResponsesApiTool {
                name: codex_code_mode::PUBLIC_TOOL_NAME.to_string(),
                description: codex_code_mode::build_exec_tool_description(
                    &enabled_tools,
                    &[],
                    &BTreeMap::new(),
                    /*code_mode_only*/ true,
                ),
                strict: false,
                parameters: JsonSchema::object(
                    BTreeMap::from([(
                        "input".to_string(),
                        JsonSchema::string(Some("JavaScript source to execute.".to_string())),
                    )]),
                    Some(vec!["input".to_string()]),
                    Some(false.into()),
                ),
                output_schema: None,
                defer_loading: None,
            })
        );
    }
}
