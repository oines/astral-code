use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use super::rewrite_bash_arguments;

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
