use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use super::rewrite_bash_arguments;
use super::rewrite_bash_arguments_for_shell_command;

fn rewritten(arguments: Value) -> anyhow::Result<Value> {
    Ok(serde_json::from_str(&rewrite_bash_arguments(
        &arguments.to_string(),
    )?)?)
}

#[test]
fn run_in_background_sets_short_initial_yield() -> anyhow::Result<()> {
    let arguments = rewritten(json!({
        "command": "npm run dev",
        "cwd": "/workspace/app",
        "run_in_background": true,
    }))?;

    assert_eq!(
        arguments,
        json!({
            "cmd": "npm run dev",
            "workdir": "/workspace/app",
            "yield_time_ms": 250,
        })
    );
    Ok(())
}

#[test]
fn run_in_background_preserves_explicit_yield_time() -> anyhow::Result<()> {
    let arguments = rewritten(json!({
        "command": "npm run dev",
        "run_in_background": true,
        "yield_time_ms": 1500,
    }))?;

    assert_eq!(
        arguments,
        json!({
            "cmd": "npm run dev",
            "yield_time_ms": 1500,
        })
    );
    Ok(())
}

#[test]
fn shell_command_backend_rejects_run_in_background() {
    let err = rewrite_bash_arguments_for_shell_command(
        &json!({
            "command": "npm run dev",
            "run_in_background": true,
        })
        .to_string(),
    )
    .expect_err("shell command backend should reject background mode");

    assert!(
        err.to_string()
            .contains("run_in_background is only supported by the unified exec backend")
    );
}
