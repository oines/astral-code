use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::DynamicToolCallParams;
use codex_app_server_protocol::FileChangeRequestApprovalParams;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ServerRequestResolvedNotification;
use codex_app_server_protocol::ThreadClosedNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ToolRequestUserInputParams;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use pretty_assertions::assert_eq;

use super::PendingInteractionError;
use super::PendingInteractionKind;
use super::PendingInteractionStatus;
use super::PendingInteractionUpdate;
use super::PendingInteractions;
use super::RequestObservation;
use super::ResponseOwnership;

#[test]
fn request_id_is_the_only_pending_identity() {
    let mut pending = PendingInteractions::new("thread-1");

    assert_eq!(
        pending.observe_request(command_approval(
            1,
            "thread-1",
            "turn-1",
            "approval-1",
            "ls",
        )),
        RequestObservation::Updated(PendingInteractionUpdate::Added {
            request_id: RequestId::Integer(1),
        })
    );
    assert_eq!(
        pending.observe_request(command_approval(
            1,
            "thread-1",
            "turn-1",
            "approval-1",
            "ls -la",
        )),
        RequestObservation::Updated(PendingInteractionUpdate::Refreshed {
            request_id: RequestId::Integer(1),
        })
    );
    assert_eq!(
        pending.observe_request(command_approval(
            2,
            "thread-1",
            "turn-1",
            "approval-1",
            "pwd",
        )),
        RequestObservation::Updated(PendingInteractionUpdate::Added {
            request_id: RequestId::Integer(2),
        })
    );
    assert_eq!(
        pending
            .iter()
            .map(|interaction| interaction.request_id().clone())
            .collect::<Vec<_>>(),
        vec![RequestId::Integer(1), RequestId::Integer(2)]
    );

    let first = pending
        .active()
        .expect("first interaction should remain queued");
    let ServerRequest::CommandExecutionRequestApproval { params, .. } = first.request() else {
        panic!("expected command approval");
    };
    assert_eq!(params.command.as_deref(), Some("ls -la"));

    assert_eq!(
        pending.begin_response(&RequestId::Integer(1)),
        Ok(ResponseOwnership::Tracked)
    );
    assert_eq!(
        pending.observe_request(command_approval(
            1,
            "thread-1",
            "turn-1",
            "approval-1",
            "ls -lah",
        )),
        RequestObservation::Updated(PendingInteractionUpdate::Refreshed {
            request_id: RequestId::Integer(1),
        })
    );
    assert_eq!(
        pending.active().map(super::PendingInteraction::status),
        Some(PendingInteractionStatus::Responding)
    );
    assert_eq!(
        pending.begin_response(&RequestId::Integer(1)),
        Err(PendingInteractionError::AlreadyResponding(
            RequestId::Integer(1)
        ))
    );
    pending.response_failed(&RequestId::Integer(1));
    assert_eq!(
        pending.active().map(super::PendingInteraction::status),
        Some(PendingInteractionStatus::Waiting)
    );
    pending.response_succeeded(&RequestId::Integer(1));

    assert_eq!(
        pending.observe_notification(&ServerNotification::ServerRequestResolved(
            ServerRequestResolvedNotification {
                thread_id: "thread-1".to_string(),
                request_id: RequestId::Integer(2),
            }
        )),
        Some(PendingInteractionUpdate::Resolved {
            request_id: RequestId::Integer(2),
        })
    );
    assert!(pending.is_empty());
}

#[test]
fn lifecycle_fallbacks_clear_only_the_active_thread_requests_they_settle() {
    let mut pending = PendingInteractions::new("thread-1");
    let foreign = command_approval(4, "thread-2", "turn-1", "approval-2", "pwd");
    assert_eq!(
        pending.observe_request(foreign.clone()),
        RequestObservation::PassThrough(Box::new(foreign))
    );
    let dynamic = dynamic_tool(5);
    assert_eq!(
        pending.observe_request(dynamic.clone()),
        RequestObservation::PassThrough(Box::new(dynamic))
    );
    assert_eq!(
        pending.begin_response(&RequestId::Integer(5)),
        Ok(ResponseOwnership::Untracked)
    );

    pending.observe_request(file_approval(6, "turn-1", "edit-1"));
    pending.observe_request(user_input(7, "turn-1", "question-1"));
    pending.observe_request(command_approval(
        8,
        "thread-1",
        "turn-2",
        "approval-3",
        "date",
    ));
    assert_eq!(pending.len(), 3);

    pending.observe_notification(&ServerNotification::ItemStarted(ItemStartedNotification {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
        started_at_ms: 1,
        item: ThreadItem::FileChange {
            id: "edit-1".to_string(),
            changes: Vec::new(),
            status: PatchApplyStatus::InProgress,
        },
    }));
    assert_eq!(pending.len(), 2);

    pending.observe_notification(&turn_completed("turn-1"));
    assert_eq!(
        pending.active().map(|interaction| (
            interaction.request_id().clone(),
            interaction.kind(),
            interaction.turn_id().map(str::to_string),
        )),
        Some((
            RequestId::Integer(8),
            PendingInteractionKind::CommandExecutionApproval,
            Some("turn-2".to_string()),
        ))
    );

    pending.observe_notification(&ServerNotification::ThreadClosed(
        ThreadClosedNotification {
            thread_id: "thread-1".to_string(),
        },
    ));
    assert!(pending.is_empty());
}

fn command_approval(
    request_id: i64,
    thread_id: &str,
    turn_id: &str,
    approval_id: &str,
    command: &str,
) -> ServerRequest {
    ServerRequest::CommandExecutionRequestApproval {
        request_id: RequestId::Integer(request_id),
        params: CommandExecutionRequestApprovalParams {
            thread_id: thread_id.to_string(),
            turn_id: turn_id.to_string(),
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

fn file_approval(request_id: i64, turn_id: &str, item_id: &str) -> ServerRequest {
    ServerRequest::FileChangeRequestApproval {
        request_id: RequestId::Integer(request_id),
        params: FileChangeRequestApprovalParams {
            thread_id: "thread-1".to_string(),
            turn_id: turn_id.to_string(),
            item_id: item_id.to_string(),
            started_at_ms: 1,
            reason: None,
            grant_root: None,
        },
    }
}

fn user_input(request_id: i64, turn_id: &str, item_id: &str) -> ServerRequest {
    ServerRequest::ToolRequestUserInput {
        request_id: RequestId::Integer(request_id),
        params: ToolRequestUserInputParams {
            thread_id: "thread-1".to_string(),
            turn_id: turn_id.to_string(),
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

fn turn_completed(turn_id: &str) -> ServerNotification {
    ServerNotification::TurnCompleted(TurnCompletedNotification {
        thread_id: "thread-1".to_string(),
        turn: Turn {
            id: turn_id.to_string(),
            items: Vec::new(),
            items_view: TurnItemsView::Full,
            status: TurnStatus::Completed,
            error: None,
            started_at: None,
            completed_at: Some(1),
            duration_ms: Some(1),
        },
    })
}
