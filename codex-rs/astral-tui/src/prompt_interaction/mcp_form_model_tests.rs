use codex_app_server_protocol::McpElicitationSchema;
use pretty_assertions::assert_eq;

use super::model::McpFormModel;
use super::model::McpFormProgress;

#[test]
fn validates_each_field_and_builds_exact_submission_content() {
    let schema = form_schema(serde_json::json!({
        "count": {"type": "integer", "minimum": 18},
        "enabled": {"type": "boolean", "default": true},
        "label": {"type": "string", "minLength": 2},
        "modes": {
            "type": "array",
            "items": {"type": "string", "enum": ["fast", "safe"]},
            "minItems": 1
        },
        "tier": {
            "type": "string",
            "oneOf": [
                {"const": "flash", "title": "Flash"},
                {"const": "pro", "title": "Pro"}
            ]
        }
    }));
    let mut form = McpFormModel::new(&schema);

    form.insert_text("17");
    assert_eq!(form.advance_or_complete(), McpFormProgress::Invalid);
    assert_eq!(form.error(), Some("Value must be at least 18"));
    form.clear_active();
    form.insert_text("21");
    assert_eq!(form.advance_or_complete(), McpFormProgress::Advanced);

    assert_eq!(form.advance_or_complete(), McpFormProgress::Advanced);
    form.insert_text("星河");
    assert!(form.backspace());
    assert_eq!(form.advance_or_complete(), McpFormProgress::Invalid);
    form.insert_text("河");
    assert_eq!(form.advance_or_complete(), McpFormProgress::Advanced);

    form.activate_choice();
    assert_eq!(form.advance_or_complete(), McpFormProgress::Advanced);
    form.set_choice_cursor(1);
    form.activate_choice();
    assert_eq!(
        form.advance_or_complete(),
        McpFormProgress::Complete(serde_json::json!({
            "count": 21,
            "enabled": true,
            "label": "星河",
            "modes": ["fast"],
            "tier": "pro"
        }))
    );
}

#[test]
fn navigation_and_unicode_editing_preserve_retained_drafts() {
    let schema = form_schema(serde_json::json!({
        "editor": {"type": "string", "minLength": 2},
        "mode": {"type": "string", "enum": ["flash", "pro"]},
        "tags": {
            "type": "array",
            "items": {"type": "string", "enum": ["a", "b"]},
            "minItems": 1
        }
    }));
    let mut form = McpFormModel::new(&schema);

    assert_eq!(form.field_count(), 3);
    assert_eq!(form.active_field_name(), Some("editor"));
    form.insert_text("A星B");
    assert!(form.move_text_cursor(-1));
    form.insert_text("河");
    assert!(form.move_text_cursor_to_edge(false));
    assert!(form.delete());
    assert!(form.move_text_cursor_to_edge(true));
    assert!(form.backspace());

    form.move_field(-1);
    assert_eq!(form.active_index(), 2);
    assert_eq!(form.choice_count(), 2);
    form.set_choice_cursor(99);
    assert_eq!(form.choice_cursor(), 1);
    form.activate_choice();
    form.clear_active();
    form.activate_choice();

    form.move_field(-1);
    form.set_choice_cursor(1);
    form.activate_choice();
    form.move_field(-1);
    assert_eq!(form.active_field_name(), Some("editor"));

    assert_eq!(form.advance_or_complete(), McpFormProgress::Advanced);
    assert_eq!(form.advance_or_complete(), McpFormProgress::Advanced);
    assert_eq!(
        form.advance_or_complete(),
        McpFormProgress::Complete(serde_json::json!({
            "editor": "星河",
            "mode": "pro",
            "tags": ["b"]
        }))
    );
}

#[test]
fn final_validation_recovers_skipped_required_and_omits_optional_fields() {
    let empty = form_schema(serde_json::json!({}));
    assert_eq!(
        McpFormModel::new(&empty).advance_or_complete(),
        McpFormProgress::Complete(serde_json::json!({}))
    );

    let schema: McpElicitationSchema = serde_json::from_value(serde_json::json!({
        "type": "object",
        "properties": {
            "a_required": {"type": "string"},
            "b_optional": {"type": "string"},
            "c_last": {"type": "boolean"}
        },
        "required": ["a_required", "c_last"]
    }))
    .expect("form schema should deserialize");
    let mut form = McpFormModel::new(&schema);

    form.move_field(-1);
    form.activate_choice();
    assert_eq!(form.advance_or_complete(), McpFormProgress::Invalid);
    assert_eq!(form.active_field_name(), Some("a_required"));
    assert_eq!(form.error(), Some("This field is required"));

    form.insert_text("ready");
    assert_eq!(form.advance_or_complete(), McpFormProgress::Advanced);
    assert_eq!(form.advance_or_complete(), McpFormProgress::Advanced);
    assert_eq!(
        form.advance_or_complete(),
        McpFormProgress::Complete(serde_json::json!({
            "a_required": "ready",
            "c_last": true
        }))
    );
}

fn form_schema(properties: serde_json::Value) -> McpElicitationSchema {
    let required = properties
        .as_object()
        .expect("properties should be an object")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    serde_json::from_value(serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required
    }))
    .expect("form schema should deserialize")
}
