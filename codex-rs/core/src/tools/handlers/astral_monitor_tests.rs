use crate::function_tool::FunctionCallError;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use super::rewrite_monitor_arguments;

fn rewritten(arguments: Value) -> anyhow::Result<Value> {
    Ok(serde_json::from_str(&rewrite_monitor_arguments(
        &arguments.to_string(),
    )?)?)
}

#[test]
fn monitor_accepts_numeric_session_id_aliases() -> anyhow::Result<()> {
    let arguments = rewritten(json!({
        "task_id": "42",
        "chars": "y\n",
        "yield_time_ms": 30000
    }))?;

    assert_eq!(
        arguments,
        json!({
            "session_id": 42,
            "chars": "y\n",
            "yield_time_ms": 30000
        })
    );
    Ok(())
}

#[test]
fn monitor_requires_a_session_identifier() {
    assert_eq!(
        rewrite_monitor_arguments(r#"{"chars":"y\n"}"#),
        Err(FunctionCallError::RespondToModel(
            "Monitor requires `session_id`, `task_id`, or `shell_id` from a previous background Bash result".to_string(),
        ))
    );
}

#[test]
fn monitor_rejects_non_numeric_shell_ids() {
    assert_eq!(
        rewrite_monitor_arguments(r#"{"shell_id":"agent-a"}"#),
        Err(FunctionCallError::RespondToModel(
            "Monitor session id must be a numeric Bash session id".to_string()
        ))
    );
}
