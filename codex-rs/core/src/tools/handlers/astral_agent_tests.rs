use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn agent_result_exposes_task_id_for_task_stop() -> anyhow::Result<()> {
    let result = AstralAgentResult::from_spawn_result(json!({
        "task_name": "/root/worker",
        "nickname": "worker"
    }))?;

    assert_eq!(
        result,
        AstralAgentResult {
            task_id: "/root/worker".to_string(),
            task_name: "/root/worker".to_string(),
            nickname: Some("worker".to_string()),
        }
    );
    Ok(())
}

#[test]
fn agent_result_accepts_existing_task_id_field() -> anyhow::Result<()> {
    let result = AstralAgentResult::from_spawn_result(json!({
        "task_id": "agent-x7q"
    }))?;

    assert_eq!(
        result,
        AstralAgentResult {
            task_id: "agent-x7q".to_string(),
            task_name: "agent-x7q".to_string(),
            nickname: None,
        }
    );
    Ok(())
}

#[test]
fn agent_result_rejects_missing_task_identifier() {
    assert_eq!(
        AstralAgentResult::from_spawn_result(json!({ "nickname": "worker" })),
        Err(FunctionCallError::RespondToModel(
            "Agent result is missing task_name/task_id".to_string()
        ))
    );
}
