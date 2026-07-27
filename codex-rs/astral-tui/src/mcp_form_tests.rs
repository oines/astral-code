use codex_app_server_protocol::McpElicitationSchema;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::compile_fields;
use super::field::McpFormChoice;
use super::field::McpFormControl;

#[test]
fn compiles_defaults_and_titled_multi_select_options() {
    let schema: McpElicitationSchema = serde_json::from_value(json!({
        "type": "object",
        "properties": {
            "features": {
                "type": "array",
                "items": {
                    "anyOf": [
                        {"const": "search", "title": "Web search"},
                        {"const": "edit", "title": "File editing"}
                    ]
                },
                "default": ["edit"]
            }
        }
    }))
    .expect("valid MCP form schema");

    let fields = compile_fields(&schema);
    assert_eq!(fields.len(), 1);
    assert_eq!(
        fields[0].control,
        McpFormControl::Select {
            choices: vec![
                McpFormChoice {
                    label: "Web search".to_string()
                },
                McpFormChoice {
                    label: "File editing".to_string()
                }
            ],
            cursor: 1,
            selected: [1].into_iter().collect(),
            multiple: true,
        }
    );
}
