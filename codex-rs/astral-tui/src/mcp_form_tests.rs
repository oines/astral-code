use codex_app_server_protocol::McpElicitationSchema;
use codex_app_server_protocol::McpServerElicitationAction;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::McpFormEvent;
use super::McpFormState;
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

#[test]
fn optional_single_select_can_be_skipped() {
    let schema: McpElicitationSchema = serde_json::from_value(json!({
        "type": "object",
        "properties": {
            "mode": {
                "type": "string",
                "enum": ["safe", "fast"]
            }
        }
    }))
    .expect("valid MCP form schema");
    let mut state = McpFormState::default();

    let event = state.handle_key(&schema, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let McpFormEvent::Submit(response) = event else {
        panic!("expected a submitted form");
    };
    assert_eq!(response.action, McpServerElicitationAction::Accept);
    assert_eq!(response.content, Some(json!({})));
}

#[test]
fn optional_empty_multi_select_does_not_apply_min_items() {
    let schema: McpElicitationSchema = serde_json::from_value(json!({
        "type": "object",
        "properties": {
            "features": {
                "type": "array",
                "minItems": 2,
                "items": {
                    "type": "string",
                    "enum": ["search", "edit"]
                }
            }
        }
    }))
    .expect("valid MCP form schema");

    let fields = compile_fields(&schema);

    assert_eq!(fields[0].validate(), Ok(()));
}
