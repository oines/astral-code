use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::DynamicToolCallParams;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ToolRequestUserInputParams;
use pretty_assertions::assert_eq;

use super::PendingInteractionError;
use super::PendingInteractionKind;
use super::PendingInteractionStatus;
use super::PendingInteractionUpdate;
use super::PendingInteractions;
use super::RequestObservation;

#[test]
fn one_queue_handles_replay_replacement_response_failure_and_remote_resolution() {
    let mut pending = PendingInteractions::new("thread-1");
    let first = command_approval(1, "thread-1", "approval-1", "ls");

    assert_eq!(
        pending.observe_request(first.clone()),
        RequestObservation::Updated(PendingInteractionUpdate::Added {
            request_id: RequestId::Integer(1),
        })
    );
    assert_eq!(
        pending.observe_request(first),
        RequestObservation::Updated(PendingInteractionUpdate::Replayed {
            request_id: RequestId::Integer(1),
        })
    );

    assert_eq!(
        pending.observe_request(command_approval(2, "thread-1", "approval-1", "ls -la",)),
        RequestObservation::Updated(PendingInteractionUpdate::Replaced {
            previous_request_id: RequestId::Integer(1),
            request_id: RequestId::Integer(2),
        })
    );
    assert_eq!(
        interaction_summary(&pending),
        vec![(
            RequestId::Integer(2),
            PendingInteractionKind::CommandExecutionApproval,
            PendingInteractionStatus::Waiting,
            Some("turn-1".to_string()),
        )]
    );

    assert_eq!(pending.begin_response(&RequestId::Integer(2)), Ok(true));
    assert_eq!(
        pending.begin_response(&RequestId::Integer(2)),
        Err(PendingInteractionError::AlreadyResponding(
            RequestId::Integer(2)
        ))
    );
    assert_eq!(
        pending.active().map(super::PendingInteraction::status),
        Some(PendingInteractionStatus::Responding)
    );
    assert!(pending.response_failed(&RequestId::Integer(2)));
    assert_eq!(
        pending.active().map(super::PendingInteraction::status),
        Some(PendingInteractionStatus::Waiting)
    );

    assert_eq!(
        pending.observe_request(user_input(3, "thread-1", "question-1")),
        RequestObservation::Updated(PendingInteractionUpdate::Added {
            request_id: RequestId::Integer(3),
        })
    );
    assert_eq!(pending.len(), 2);
    assert_eq!(
        pending.resolve_notification("thread-2", &RequestId::Integer(2)),
        None
    );
    assert_eq!(
        pending.resolve_notification("thread-1", &RequestId::Integer(2)),
        Some(PendingInteractionUpdate::Resolved {
            request_id: RequestId::Integer(2),
        })
    );
    assert_eq!(
        pending
            .active()
            .map(|interaction| interaction.request_id().clone()),
        Some(RequestId::Integer(3))
    );

    let foreign = command_approval(4, "thread-2", "approval-2", "pwd");
    assert_eq!(
        pending.observe_request(foreign.clone()),
        RequestObservation::PassThrough(Box::new(foreign))
    );
    let dynamic = dynamic_tool(5);
    assert_eq!(
        pending.observe_request(dynamic.clone()),
        RequestObservation::PassThrough(Box::new(dynamic))
    );
    assert_eq!(pending.len(), 1);

    assert_eq!(
        pending.response_succeeded(&RequestId::Integer(3)),
        Some(PendingInteractionUpdate::Resolved {
            request_id: RequestId::Integer(3),
        })
    );
    assert!(pending.is_empty());
}

fn interaction_summary(
    pending: &PendingInteractions,
) -> Vec<(
    RequestId,
    PendingInteractionKind,
    PendingInteractionStatus,
    Option<String>,
)> {
    pending
        .iter()
        .map(|interaction| {
            (
                interaction.request_id().clone(),
                interaction.kind(),
                interaction.status(),
                interaction.turn_id().map(str::to_string),
            )
        })
        .collect()
}

fn command_approval(
    request_id: i64,
    thread_id: &str,
    approval_id: &str,
    command: &str,
) -> ServerRequest {
    ServerRequest::CommandExecutionRequestApproval {
        request_id: RequestId::Integer(request_id),
        params: CommandExecutionRequestApprovalParams {
            thread_id: thread_id.to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "command-1".to_string(),
            started_at_ms: 1,
            approval_id: Some(approval_id.to_string()),
            environment_id: None,
            reason: None,
            network_approval_context: None,
            command: Some(command.to_string()),
            cwd: None,
            command_actions: None,
            additional_permissions: None,
            proposed_execpolicy_amendment: None,
            proposed_network_policy_amendments: None,
            available_decisions: None,
        },
    }
}

fn user_input(request_id: i64, thread_id: &str, item_id: &str) -> ServerRequest {
    ServerRequest::ToolRequestUserInput {
        request_id: RequestId::Integer(request_id),
        params: ToolRequestUserInputParams {
            thread_id: thread_id.to_string(),
            turn_id: "turn-1".to_string(),
            item_id: item_id.to_string(),
            questions: Vec::new(),
        },
    }
}

fn dynamic_tool(request_id: i64) -> ServerRequest {
    ServerRequest::DynamicToolCall {
        request_id: RequestId::Integer(request_id),
        params: DynamicToolCallParams {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            call_id: "dynamic-1".to_string(),
            namespace: None,
            tool: "client-tool".to_string(),
            arguments: Default::default(),
        },
    }
}
