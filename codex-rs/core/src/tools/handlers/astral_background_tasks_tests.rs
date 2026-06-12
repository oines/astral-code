use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use super::TaskIoMode;
use super::rewrite_task_io_arguments;

fn rewritten(arguments: Value, mode: TaskIoMode) -> anyhow::Result<Value> {
    Ok(serde_json::from_str(&rewrite_task_io_arguments(
        &arguments.to_string(),
        mode,
    )?)?)
}

#[test]
fn read_task_output_maps_task_id_to_empty_stdin_poll() -> anyhow::Result<()> {
    let arguments = rewritten(
        json!({
            "task_id": "42",
            "yield_time_ms": 5000,
            "max_output_tokens": 2000,
        }),
        TaskIoMode::ReadOutput,
    )?;

    assert_eq!(
        arguments,
        json!({
            "session_id": 42,
            "chars": "",
            "yield_time_ms": 5000,
            "max_output_tokens": 2000,
        })
    );
    Ok(())
}

#[test]
fn send_task_input_maps_input_to_stdin_chars() -> anyhow::Result<()> {
    let arguments = rewritten(
        json!({
            "task_id": 42,
            "input": "y\n",
            "yield_time_ms": 250,
        }),
        TaskIoMode::SendInput,
    )?;

    assert_eq!(
        arguments,
        json!({
            "session_id": 42,
            "chars": "y\n",
            "yield_time_ms": 250,
        })
    );
    Ok(())
}

#[test]
fn task_id_aliases_are_accepted_but_normalized() -> anyhow::Result<()> {
    let arguments = rewritten(
        json!({
            "shell_id": "7",
        }),
        TaskIoMode::ReadOutput,
    )?;

    assert_eq!(
        arguments,
        json!({
            "session_id": 7,
            "chars": "",
        })
    );
    Ok(())
}

#[test]
fn send_task_input_rejects_empty_input() {
    let err = rewrite_task_io_arguments(
        &json!({
            "task_id": 42,
            "input": "",
        })
        .to_string(),
        TaskIoMode::SendInput,
    )
    .expect_err("empty input should fail");

    assert_eq!(
        err.to_string(),
        "SendTaskInput `input` must not be empty; use ReadTaskOutput to poll output"
    );
}
