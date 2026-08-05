use super::*;
use pretty_assertions::assert_eq;

fn start_compaction(chat: &mut ChatWidget, item_id: &str, started_at_ms: i64) {
    chat.handle_server_notification(
        ServerNotification::ItemStarted(ItemStartedNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            started_at_ms,
            item: AppServerThreadItem::ContextCompaction {
                id: item_id.to_string(),
            },
        }),
        /*replay_kind*/ None,
    );
}

fn rendered_history(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> String {
    drain_insert_history(rx)
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn compaction_lifecycle_renders_authoritative_outcomes_snapshot() {
    let (mut completed_chat, mut completed_rx, _op_rx) =
        make_chatwidget_manual(/*model_override*/ None).await;
    completed_chat.on_context_compaction_requested();
    assert_eq!(
        completed_chat
            .bottom_pane
            .status_widget()
            .expect("the requested compaction should show status")
            .header(),
        "Compacting…"
    );
    handle_turn_started(&mut completed_chat, "turn-1");
    assert_eq!(
        completed_chat
            .bottom_pane
            .status_widget()
            .expect("turn start should preserve compaction status")
            .header(),
        "Compacting…"
    );
    start_compaction(
        &mut completed_chat,
        "compact-1",
        /*started_at_ms*/ 1_000,
    );

    assert!(completed_chat.bottom_pane.is_task_running());
    assert_eq!(
        completed_chat
            .bottom_pane
            .status_widget()
            .expect("compaction should keep the status visible")
            .header(),
        "Compacting…"
    );
    assert!(drain_insert_history(&mut completed_rx).is_empty());

    completed_chat.handle_server_notification(
        ServerNotification::ItemCompleted(ItemCompletedNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 3_450,
            item: AppServerThreadItem::ContextCompaction {
                id: "compact-1".to_string(),
            },
        }),
        /*replay_kind*/ None,
    );
    let completed = rendered_history(&mut completed_rx);
    assert_eq!(
        completed_chat
            .bottom_pane
            .status_widget()
            .expect("the enclosing turn is still running")
            .header(),
        "Working"
    );

    let (mut failed_chat, mut failed_rx, _op_rx) =
        make_chatwidget_manual(/*model_override*/ None).await;
    handle_turn_started(&mut failed_chat, "turn-1");
    start_compaction(&mut failed_chat, "compact-2", /*started_at_ms*/ 1_000);
    handle_error(
        &mut failed_chat,
        "backend unavailable",
        /*codex_error_info*/ None,
    );
    let failed = rendered_history(&mut failed_rx);
    failed_chat.handle_server_notification(
        ServerNotification::TurnCompleted(TurnCompletedNotification {
            thread_id: "thread-1".to_string(),
            turn: app_server_turn(
                "turn-1",
                AppServerTurnStatus::Failed,
                /*duration_ms*/ None,
                Some(AppServerTurnError {
                    message: "backend unavailable".to_string(),
                    codex_error_info: None,
                    additional_details: None,
                }),
            ),
        }),
        /*replay_kind*/ None,
    );
    assert!(drain_insert_history(&mut failed_rx).is_empty());

    let (mut directly_failed_chat, mut directly_failed_rx, _op_rx) =
        make_chatwidget_manual(/*model_override*/ None).await;
    directly_failed_chat.on_context_compaction_requested();
    handle_turn_started(&mut directly_failed_chat, "turn-1");
    directly_failed_chat.handle_server_notification(
        ServerNotification::TurnCompleted(TurnCompletedNotification {
            thread_id: "thread-1".to_string(),
            turn: app_server_turn(
                "turn-1",
                AppServerTurnStatus::Failed,
                /*duration_ms*/ None,
                Some(AppServerTurnError {
                    message: "backend unavailable".to_string(),
                    codex_error_info: None,
                    additional_details: None,
                }),
            ),
        }),
        /*replay_kind*/ None,
    );
    let directly_failed = rendered_history(&mut directly_failed_rx);

    let (mut directly_failed_without_error_chat, mut directly_failed_without_error_rx, _op_rx) =
        make_chatwidget_manual(/*model_override*/ None).await;
    directly_failed_without_error_chat.on_context_compaction_requested();
    handle_turn_started(&mut directly_failed_without_error_chat, "turn-1");
    directly_failed_without_error_chat.handle_server_notification(
        ServerNotification::TurnCompleted(TurnCompletedNotification {
            thread_id: "thread-1".to_string(),
            turn: app_server_turn(
                "turn-1",
                AppServerTurnStatus::Failed,
                /*duration_ms*/ None,
                /*error*/ None,
            ),
        }),
        /*replay_kind*/ None,
    );
    let directly_failed_without_error = rendered_history(&mut directly_failed_without_error_rx);

    let (mut cancelled_chat, mut cancelled_rx, _op_rx) =
        make_chatwidget_manual(/*model_override*/ None).await;
    handle_turn_started(&mut cancelled_chat, "turn-1");
    start_compaction(
        &mut cancelled_chat,
        "compact-3",
        /*started_at_ms*/ 1_000,
    );
    handle_turn_interrupted(&mut cancelled_chat, "turn-1");
    let cancelled = rendered_history(&mut cancelled_rx);

    let (mut replay_chat, mut replay_rx, _op_rx) =
        make_chatwidget_manual(/*model_override*/ None).await;
    replay_chat.replay_thread_item(
        AppServerThreadItem::ContextCompaction {
            id: "compact-replay".to_string(),
        },
        "turn-replay".to_string(),
        ReplayKind::ResumeInitialMessages,
    );
    let replayed = rendered_history(&mut replay_rx);

    assert_chatwidget_snapshot!(
        "compaction_lifecycle_outcomes",
        format!(
            "completed\n{completed}\n\nfailed\n{failed}\n\ndirect failure\n{directly_failed}\n\ndirect failure without error\n{directly_failed_without_error}\n\ncancelled\n{cancelled}\n\nreplayed\n{replayed}"
        )
    );
}
