use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use super::rewrite_todo_write_arguments;

fn rewritten(arguments: Value) -> anyhow::Result<Value> {
    Ok(serde_json::from_str(&rewrite_todo_write_arguments(
        &arguments.to_string(),
    )?)?)
}

#[test]
fn rewrites_claudeish_todos_to_plan_update() -> anyhow::Result<()> {
    let arguments = rewritten(json!({
        "todos": [
            {
                "content": "Map Bash to unified exec",
                "status": "completed",
                "activeForm": "Mapping Bash"
            },
            {
                "content": "Harden TodoWrite",
                "status": "in_progress",
                "activeForm": "Hardening TodoWrite"
            }
        ]
    }))?;

    assert_eq!(
        arguments,
        json!({
            "explanation": null,
            "plan": [
                {
                    "step": "Map Bash to unified exec",
                    "status": "completed",
                    "activeForm": "Mapping Bash"
                },
                {
                    "step": "Harden TodoWrite",
                    "status": "in_progress",
                    "activeForm": "Hardening TodoWrite"
                }
            ]
        })
    );
    Ok(())
}

#[test]
fn preserves_optional_explanation_for_internal_plan_updates() -> anyhow::Result<()> {
    let arguments = rewritten(json!({
        "explanation": "Switching implementation focus",
        "todos": [
            {
                "content": "Run focused checks",
                "status": "pending",
                "activeForm": "Running checks"
            }
        ]
    }))?;

    assert_eq!(
        arguments,
        json!({
            "explanation": "Switching implementation focus",
            "plan": [
                {
                    "step": "Run focused checks",
                    "status": "pending",
                    "activeForm": "Running checks"
                }
            ]
        })
    );
    Ok(())
}
