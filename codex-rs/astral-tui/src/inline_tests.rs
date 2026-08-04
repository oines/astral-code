use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_app_server_protocol::WebSearchAction;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use ratatui::TerminalOptions;
use ratatui::Viewport;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::InlineHost;
use crate::ConversationState;
use crate::SurfaceNodeId;

#[test]
fn inline_frontier_waits_for_group_boundary_and_prints_each_node_once() {
    let mut conversation = ConversationState::from_thread(&thread(vec![Turn {
        id: "turn-1".to_string(),
        items: vec![
            user("Inspect the repository"),
            reasoning("reasoning", "I will inspect the relevant files."),
        ],
        items_view: TurnItemsView::Full,
        status: TurnStatus::InProgress,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }]));
    let terminal_width = 60;
    let mut host = InlineHost::new(&conversation, terminal_width);
    let mut terminal = astral_terminal_inline::Terminal::with_options(
        TestBackend::new(terminal_width, 12),
        TerminalOptions {
            viewport: Viewport::Inline(4),
        },
    )
    .expect("inline terminal");

    let reasoning = host.surface().nodes()[1].id();
    assert!(matches!(reasoning, SurfaceNodeId::Entry(_)));
    assert!(!super::is_commit_ready(&host.surface().nodes()[1]));
    assert_eq!(
        host.live_tail_height(u16::MAX),
        (host.surface().row_count() - host.surface().nodes()[1].rows().start) as u16
    );
    let first = host
        .commit_to_terminal(&mut terminal)
        .expect("commit user prompt");
    assert_eq!(first.committed_nodes, 1);
    assert_eq!(first.tail_start, host.surface().nodes()[1].rows().start);

    assert_eq!(
        conversation.apply(&item_started(web_search(
            "search",
            "ratatui inline viewport"
        ))),
        astral_tui_scrollback::ApplyOutcome::Applied
    );
    host.refresh_surface(&conversation, terminal_width);
    let group = host.surface().nodes()[1].id();
    assert!(matches!(group, SurfaceNodeId::VerbGroup(_)));
    assert!(!super::is_commit_ready(&host.surface().nodes()[1]));
    assert_eq!(
        host.commit_to_terminal(&mut terminal)
            .expect("open running group stays live")
            .committed_nodes,
        0
    );

    assert_eq!(
        conversation.apply(&item_completed(web_search(
            "search",
            "ratatui inline viewport"
        ))),
        astral_tui_scrollback::ApplyOutcome::Applied
    );
    host.refresh_surface(&conversation, terminal_width);
    assert!(!super::is_commit_ready(&host.surface().nodes()[1]));

    assert_eq!(
        conversation.apply(&item_started(assistant("answer", "Streaming answer…"))),
        astral_tui_scrollback::ApplyOutcome::Applied
    );
    host.refresh_surface(&conversation, terminal_width);
    assert!(super::is_commit_ready(
        host.surface().node(group).expect("same group")
    ));
    let assistant_start = host.surface().nodes()[2].rows().start;
    assert_eq!(
        host.live_tail_height(u16::MAX),
        (host.surface().row_count() - assistant_start) as u16
    );
    let second = host
        .commit_to_terminal(&mut terminal)
        .expect("commit closed group");
    assert_eq!(second.committed_nodes, 1);

    let area = Rect::new(0, 0, terminal_width, 4);
    let mut live = Buffer::empty(area);
    host.render_live_tail(area, &mut live);
    let terminal_area = Rect::new(0, 0, terminal_width, 12);
    let committed = buffer_text(terminal.backend_mut().buffer(), terminal_area);
    insta::assert_snapshot!(format!(
        "COMMITTED TERMINAL\n{committed}\n\nPINNED LIVE TAIL\n{}",
        buffer_text(&live, area)
    ));

    assert_eq!(
        conversation.apply(&item_completed(assistant("answer", "Done."))),
        astral_tui_scrollback::ApplyOutcome::Applied
    );
    host.refresh_surface(&conversation, terminal_width);
    let final_pass = host
        .commit_to_terminal(&mut terminal)
        .expect("commit completed answer");
    assert_eq!(final_pass.committed_nodes, 1);
    assert_eq!(final_pass.tail_start, host.surface().row_count());
    assert_eq!(
        host.commit_to_terminal(&mut terminal)
            .expect("idempotent commit")
            .committed_nodes,
        0
    );
}

fn buffer_text(buffer: &Buffer, area: Rect) -> String {
    (area.y..area.bottom())
        .map(|y| {
            let mut row = String::new();
            for x in area.x..area.right() {
                row.push_str(buffer.cell((x, y)).expect("cell in area").symbol());
            }
            row.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn thread(turns: Vec<Turn>) -> Thread {
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
        status: ThreadStatus::Idle,
        path: None,
        cwd: AbsolutePathBuf::current_dir().expect("current directory should be absolute"),
        cli_version: "0.0.0".to_string(),
        source: SessionSource::Exec,
        thread_source: None,
        agent_nickname: None,
        agent_role: None,
        git_info: None,
        name: None,
        turns,
    }
}

fn item_started(item: ThreadItem) -> ServerNotification {
    ServerNotification::ItemStarted(ItemStartedNotification {
        item,
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
        started_at_ms: 1_000,
    })
}

fn item_completed(item: ThreadItem) -> ServerNotification {
    ServerNotification::ItemCompleted(ItemCompletedNotification {
        item,
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
        completed_at_ms: 2_000,
    })
}

fn user(text: &str) -> ThreadItem {
    ThreadItem::UserMessage {
        id: "user".to_string(),
        client_id: None,
        content: vec![UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }],
    }
}

fn reasoning(id: &str, summary: &str) -> ThreadItem {
    ThreadItem::Reasoning {
        id: id.to_string(),
        summary: vec![summary.to_string()],
        content: Vec::new(),
    }
}

fn web_search(id: &str, query: &str) -> ThreadItem {
    ThreadItem::WebSearch {
        id: id.to_string(),
        query: query.to_string(),
        action: Some(WebSearchAction::Search {
            query: Some(query.to_string()),
            queries: None,
        }),
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
