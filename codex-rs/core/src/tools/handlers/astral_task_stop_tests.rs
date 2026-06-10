use super::*;
use pretty_assertions::assert_eq;

#[test]
fn task_stop_target_prefers_task_id_over_shell_alias() -> anyhow::Result<()> {
    let target =
        task_stop_target_from_arguments(r#"{"task_id":"/root/worker","shell_id":"1001"}"#)?;

    assert_eq!(target.id, "/root/worker");
    assert_eq!(target.hint, TargetHint::TaskId);
    assert_eq!(target.shell_process_id()?, None);
    Ok(())
}

#[test]
fn task_stop_target_accepts_numeric_shell_alias() -> anyhow::Result<()> {
    let target = task_stop_target_from_arguments(r#"{"shell_id":"1001"}"#)?;

    assert_eq!(target.id, "1001");
    assert_eq!(target.hint, TargetHint::ShellId);
    assert_eq!(target.shell_process_id()?, Some(1001));
    Ok(())
}

#[test]
fn task_stop_target_rejects_non_numeric_shell_alias() {
    let target = task_stop_target_from_arguments(r#"{"shell_id":"agent-a"}"#)
        .expect("shell_id target should parse before shell process validation");

    assert_eq!(
        target.shell_process_id(),
        Err(FunctionCallError::RespondToModel(
            "TaskStop `shell_id` must be a numeric Bash session id".to_string()
        ))
    );
}
