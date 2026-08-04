use codex_app_server_protocol::McpElicitationNumberSchema;
use codex_app_server_protocol::McpElicitationPrimitiveSchema;
use pretty_assertions::assert_eq;
use serde_json::Value;

use super::field::McpFormControl;
use super::field::McpFormField;

type ProjectionCase = (
    &'static str,
    bool,
    Option<&'static str>,
    Option<&'static str>,
);

#[rustfmt::skip]
const PROJECTION_CASES: &[ProjectionCase] = &[
    (r#"{"type":"string","enum":["fast","safe"],"enumNames":["Fast","Safe"],"default":"safe"}"#, true, Some(r#""safe""#), None),
    (r#"{"type":"string","enum":["fast","safe"],"default":"fast"}"#, true, Some(r#""fast""#), None),
    (r#"{"type":"string","oneOf":[{"const":"flash","title":"Flash"},{"const":"pro","title":"Pro"}],"default":"pro"}"#, true, Some(r#""pro""#), None),
    (r#"{"type":"array","items":{"type":"string","enum":["a","b"]},"minItems":2,"maxItems":2,"default":["b"]}"#, true, Some(r#"["b"]"#), Some("Choose at least 2 options")),
    (r#"{"type":"array","items":{"anyOf":[{"const":"a","title":"Alpha"},{"const":"b","title":"Beta"}]},"maxItems":1,"default":["a","b"]}"#, true, Some(r#"["a","b"]"#), Some("Choose at most 1 options")),
    (r#"{"type":"string","minLength":2,"default":"x"}"#, true, Some(r#""x""#), Some("Enter at least 2 characters")),
    (r#"{"type":"string","maxLength":2,"default":"xyz"}"#, true, Some(r#""xyz""#), Some("Enter at most 2 characters")),
    (r#"{"type":"integer","minimum":18,"default":17}"#, true, Some("17"), Some("Value must be at least 18")),
    (r#"{"type":"number","maximum":2.0,"default":2.5}"#, true, Some("2.5"), Some("Value must be at most 2")),
    (r#"{"type":"boolean","default":false}"#, true, Some("false"), None),
    (r#"{"type":"string"}"#, true, None, Some("This field is required")),
    (r#"{"type":"string"}"#, false, None, None),
];

#[test]
fn projects_supported_wire_shapes_defaults_and_constraints() {
    let legacy = field(PROJECTION_CASES[0].0, /*required*/ true);
    let McpFormControl::Select { options, .. } = &legacy.control else {
        panic!("legacy enum should project to a select control");
    };
    assert_eq!(
        options
            .iter()
            .map(|option| option.label.as_str())
            .collect::<Vec<_>>(),
        vec!["Fast", "Safe"]
    );

    for &(schema, required, expected_value, expected_error) in PROJECTION_CASES {
        let field = field(schema, required);
        let expected_value = expected_value.map(|json| {
            serde_json::from_str::<Value>(json).expect("expected value should be valid JSON")
        });
        assert_eq!(field.value(), expected_value);
        assert_eq!(field.validate().err().as_deref(), expected_error);
    }
}

fn field(schema: &str, required: bool) -> McpFormField {
    // App-server has already typed the schema before the TUI sees it; construct
    // Number directly when exercising that projection branch.
    let schema = serde_json::from_str::<McpElicitationPrimitiveSchema>(schema)
        .or_else(|_| {
            serde_json::from_str::<McpElicitationNumberSchema>(schema)
                .map(McpElicitationPrimitiveSchema::Number)
        })
        .expect("primitive schema should deserialize");
    McpFormField::new("field", &schema, required)
}
