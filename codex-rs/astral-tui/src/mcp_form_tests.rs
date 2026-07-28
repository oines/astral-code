use std::time::Duration;
use std::time::Instant;

use codex_app_server_protocol::McpElicitationSchema;
use codex_app_server_protocol::McpServerElicitationAction;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use pretty_assertions::assert_eq;
use ratatui::layout::Rect;
use serde_json::json;

use super::McpFormEvent;
use super::McpFormHit;
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

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn pointer_activates_a_choice_on_the_second_click() {
    let schema: McpElicitationSchema = serde_json::from_value(json!({
        "type": "object",
        "properties": {
            "confirmed": {
                "type": "boolean"
            }
        },
        "required": ["confirmed"]
    }))
    .expect("valid MCP form schema");
    let mut state = McpFormState::default();
    state.sync(&schema);
    state.observe_rows(vec![(McpFormHit::Choice(1), Rect::new(3, 5, 24, 1))]);
    let now = Instant::now();

    assert_eq!(
        state.handle_mouse_at(
            &schema,
            mouse(MouseEventKind::Down(MouseButton::Left), 4, 5),
            now,
        ),
        McpFormEvent::Redraw
    );
    assert_eq!(
        state.handle_mouse_at(
            &schema,
            mouse(MouseEventKind::Down(MouseButton::Left), 4, 5),
            now + Duration::from_millis(1),
        ),
        McpFormEvent::Submit(super::response(
            McpServerElicitationAction::Accept,
            Some(json!({"confirmed": false})),
        ))
    );
}

#[test]
fn pointer_hover_tracks_only_rendered_form_rows() {
    let schema: McpElicitationSchema = serde_json::from_value(json!({
        "type": "object",
        "properties": {
            "confirmed": {
                "type": "boolean"
            }
        }
    }))
    .expect("valid MCP form schema");
    let mut state = McpFormState::default();
    state.sync(&schema);
    state.observe_rows(vec![(McpFormHit::Choice(0), Rect::new(3, 5, 24, 1))]);

    assert_eq!(
        state.handle_mouse_at(&schema, mouse(MouseEventKind::Moved, 4, 5), Instant::now()),
        McpFormEvent::Redraw
    );
    assert_eq!(state.hovered(), Some(McpFormHit::Choice(0)));
    assert_eq!(
        state.handle_mouse_at(&schema, mouse(MouseEventKind::Moved, 4, 8), Instant::now()),
        McpFormEvent::Redraw
    );
    assert_eq!(state.hovered(), None);
}

#[test]
fn digits_select_and_horizontal_keys_navigate_select_fields() {
    let schema: McpElicitationSchema = serde_json::from_value(json!({
        "type": "object",
        "properties": {
            "first": {
                "type": "string",
                "enum": ["safe", "fast"]
            },
            "second": {
                "type": "boolean"
            }
        }
    }))
    .expect("valid MCP form schema");
    let mut state = McpFormState::default();

    assert_eq!(
        state.handle_key(
            &schema,
            KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
        ),
        McpFormEvent::Redraw
    );
    assert_eq!(state.current_index(), 1);
    assert_eq!(
        state.handle_key(&schema, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
        McpFormEvent::Redraw
    );
    assert_eq!(state.current_index(), 0);
}
