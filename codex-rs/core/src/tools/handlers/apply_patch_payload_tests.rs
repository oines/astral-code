use pretty_assertions::assert_eq;
use serde_json::json;

use super::apply_patch_input_from_payload;
use crate::tools::context::ToolPayload;

const PATCH: &str = "*** Begin Patch\n*** End Patch";

#[test]
fn normalizes_custom_and_function_payloads_to_the_same_patch() {
    let payloads = [
        ToolPayload::Custom {
            input: PATCH.to_string(),
        },
        ToolPayload::Function {
            arguments: json!(PATCH).to_string(),
        },
        ToolPayload::Function {
            arguments: json!({ "input": PATCH }).to_string(),
        },
    ];

    for payload in payloads {
        assert_eq!(
            apply_patch_input_from_payload(&payload).expect("valid apply_patch payload"),
            PATCH
        );
    }
}

#[test]
fn rejects_function_payload_without_patch_input() {
    let error = apply_patch_input_from_payload(&ToolPayload::Function {
        arguments: json!({ "patch": PATCH }).to_string(),
    })
    .expect_err("payload without input must fail");

    assert_eq!(
        error.to_string(),
        "apply_patch handler received invalid function arguments"
    );
}
