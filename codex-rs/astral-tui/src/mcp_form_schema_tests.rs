use codex_app_server_protocol::McpElicitationSchema;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::McpFormFieldKind;
use super::McpFormFieldSchema;
use super::project_fields;

#[test]
fn projects_ordered_typed_fields_and_required_metadata() {
    let schema: McpElicitationSchema = serde_json::from_value(json!({
        "type": "object",
        "properties": {
            "confirmed": {
                "type": "boolean",
                "title": "Confirm changes",
                "description": "Apply these settings"
            },
            "features": {
                "type": "array",
                "items": {
                    "type": "string",
                    "enum": ["search", "edit"]
                }
            }
        },
        "required": ["confirmed"]
    }))
    .expect("valid MCP form schema");

    assert_eq!(
        project_fields(&schema),
        vec![
            McpFormFieldSchema {
                name: "confirmed".to_string(),
                title: "Confirm changes".to_string(),
                description: Some("Apply these settings".to_string()),
                required: true,
                kind: McpFormFieldKind::Boolean,
            },
            McpFormFieldSchema {
                name: "features".to_string(),
                title: "features".to_string(),
                description: None,
                required: false,
                kind: McpFormFieldKind::MultiSelect,
            },
        ]
    );
}
