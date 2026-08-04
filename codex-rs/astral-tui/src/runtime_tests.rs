use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::AgentMessageDeltaNotification;
use codex_app_server_protocol::CommandExecutionOutputDeltaNotification;
use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::DynamicToolCallParams;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ServerRequestResolvedNotification;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadStatus;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

use super::RuntimeEvent;
use super::TranscriptUpdate;
use super::apply_event;
use crate::ConversationState;
use crate::PendingInteractionUpdate;
use crate::PendingInteractions;

#[test]
fn notification_projection_recovers_dropped_starts_and_ignores_unusable_deltas() {
    let mut conversation = ConversationState::from_thread(&thread());
    let mut pending = PendingInteractions::new("thread-1");
    let started = ServerNotification::ItemStarted(ItemStartedNotification {
        item: assistant("assistant", ""),
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
        started_at_ms: 10,
    });

    let action = apply_event(
        &mut conversation,
        &mut pending,
        AppServerEvent::ServerNotification(started),
    );
    assert!(matches!(
        action,
        Some(RuntimeEvent::ServerNotification {
            transcript_update: TranscriptUpdate::Applied,
            ..
        })
    ));
    assert_eq!(
        conversation.transcript().turns()[0].entries()[0]
            .item()
            .id(),
        "assistant"
    );

    let inferred = ServerNotification::AgentMessageDelta(AgentMessageDeltaNotification {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-without-start".to_string(),
        item_id: "missing".to_string(),
        delta: "late".to_string(),
    });
    assert!(matches!(
        apply_event(
            &mut conversation,
            &mut pending,
            AppServerEvent::ServerNotification(inferred)
        ),
        Some(RuntimeEvent::ServerNotification {
            transcript_update: TranscriptUpdate::Applied,
            ..
        })
    ));
    assert_eq!(
        conversation.transcript().turns()[1].entries()[0]
            .item()
            .id(),
        "missing"
    );

    assert!(matches!(
        apply_event(
            &mut conversation,
            &mut pending,
            AppServerEvent::ServerNotification(ServerNotification::ItemCompleted(
                ItemCompletedNotification {
                    item: assistant("assistant", "done"),
                    thread_id: "thread-1".to_string(),
                    turn_id: "turn-1".to_string(),
                    completed_at_ms: 20,
                },
            )),
        ),
        Some(RuntimeEvent::ServerNotification {
            transcript_update: TranscriptUpdate::Applied,
            ..
        })
    ));
    let late = ServerNotification::AgentMessageDelta(AgentMessageDeltaNotification {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
        item_id: "assistant".to_string(),
        delta: "too late".to_string(),
    });
    assert!(matches!(
        apply_event(
            &mut conversation,
            &mut pending,
            AppServerEvent::ServerNotification(late)
        ),
        Some(RuntimeEvent::ServerNotification {
            transcript_update: TranscriptUpdate::Unchanged,
            ..
        })
    ));

    let best_effort_output =
        ServerNotification::CommandExecutionOutputDelta(CommandExecutionOutputDeltaNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "missing-command-turn".to_string(),
            item_id: "missing-command".to_string(),
            delta: "cosmetic output".to_string(),
        });
    assert!(matches!(
        apply_event(
            &mut conversation,
            &mut pending,
            AppServerEvent::ServerNotification(best_effort_output)
        ),
        Some(RuntimeEvent::ServerNotification {
            transcript_update: TranscriptUpdate::Unchanged,
            ..
        })
    ));
}

#[test]
fn event_boundary_owns_active_interactions_and_preserves_other_server_requests() {
    let mut conversation = ConversationState::from_thread(&thread());
    let mut pending = PendingInteractions::new("thread-1");
    assert!(
        apply_event(
            &mut conversation,
            &mut pending,
            AppServerEvent::Lagged { skipped: 42 }
        )
        .is_none()
    );

    let request = ServerRequest::DynamicToolCall {
        request_id: RequestId::Integer(7),
        params: DynamicToolCallParams {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            call_id: "call-1".to_string(),
            namespace: None,
            tool: "client_tool".to_string(),
            arguments: Default::default(),
        },
    };
    let action = apply_event(
        &mut conversation,
        &mut pending,
        AppServerEvent::ServerRequest(request.clone()),
    );
    let Some(RuntimeEvent::ServerRequest(forwarded)) = action else {
        panic!("server request should be forwarded to the assembly layer");
    };
    assert_eq!(forwarded, request);

    let approval = ServerRequest::CommandExecutionRequestApproval {
        request_id: RequestId::Integer(8),
        params: CommandExecutionRequestApprovalParams {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "command-1".to_string(),
            started_at_ms: 1,
            approval_id: None,
            environment_id: None,
            reason: None,
            network_approval_context: None,
            command: Some("ls".to_string()),
            cwd: None,
            command_actions: None,
            additional_permissions: None,
            proposed_execpolicy_amendment: None,
            proposed_network_policy_amendments: None,
            available_decisions: None,
        },
    };
    assert!(matches!(
        apply_event(
            &mut conversation,
            &mut pending,
            AppServerEvent::ServerRequest(approval)
        ),
        Some(RuntimeEvent::PendingInteraction(
            PendingInteractionUpdate::Added {
                request_id: RequestId::Integer(8),
            }
        ))
    ));
    assert_eq!(pending.len(), 1);

    assert!(matches!(
        apply_event(
            &mut conversation,
            &mut pending,
            AppServerEvent::ServerNotification(ServerNotification::ServerRequestResolved(
                ServerRequestResolvedNotification {
                    thread_id: "thread-1".to_string(),
                    request_id: RequestId::Integer(8),
                }
            ))
        ),
        Some(RuntimeEvent::PendingInteraction(
            PendingInteractionUpdate::Resolved {
                request_id: RequestId::Integer(8),
            }
        ))
    ));
    assert!(pending.is_empty());

    let action = apply_event(
        &mut conversation,
        &mut pending,
        AppServerEvent::Disconnected {
            message: "closed".to_string(),
        },
    );
    assert!(matches!(
        action,
        Some(RuntimeEvent::Disconnected { message }) if message == "closed"
    ));
}

fn thread() -> Thread {
    Thread {
        id: "thread-1".to_string(),
        session_id: "session-1".to_string(),
        forked_from_id: None,
        parent_thread_id: None,
        preview: String::new(),
        ephemeral: false,
        model_provider: "astral".to_string(),
        created_at: 1,
        updated_at: 1,
        status: ThreadStatus::Active {
            active_flags: Vec::new(),
        },
        path: None,
        cwd: AbsolutePathBuf::current_dir().expect("current directory should be absolute"),
        cli_version: "0.0.0".to_string(),
        source: SessionSource::Exec,
        thread_source: None,
        agent_nickname: None,
        agent_role: None,
        git_info: None,
        name: None,
        turns: Vec::new(),
    }
}

fn assistant(id: &str, text: &str) -> ThreadItem {
    ThreadItem::AgentMessage {
        id: id.to_string(),
        text: text.to_string(),
        phase: None,
        memory_citation: None,
    }
}
