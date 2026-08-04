use astral_tui_scrollback::ApplyOutcome;
use codex_app_server_protocol::AgentMessageDeltaNotification;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use codex_utils_absolute_path::AbsolutePathBuf;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::BlockViewerHost;
use super::BlockViewerOutcome;
use crate::ConversationState;
use crate::SurfaceNodeId;

#[test]
fn thought_viewer_uses_visible_reasoning_and_refuses_opaque_content() {
    let mut summary_conversation = conversation(vec![reasoning(
        "thought",
        &[
            "I inspected the canonical transcript.\n\n- Keep source order\n- Do not clone stale text",
        ],
        &[],
    )]);
    let entry_id = summary_conversation.transcript().turns()[0].entries()[0].id();
    let mut viewer = BlockViewerHost::open(&summary_conversation, SurfaceNodeId::Entry(entry_id))
        .expect("summary reasoning should open");
    assert!(!viewer.supports_raw(&summary_conversation));
    let area = Rect::new(0, 0, 88, 28);
    let mut buffer = Buffer::empty(area);

    assert!(viewer.render(&mut buffer, area, &summary_conversation));
    insta::assert_snapshot!(buffer_text(&buffer, area));

    summary_conversation.reset_from_thread(&thread(vec![reasoning(
        "thought",
        &["Updated summary."],
        &["Canonical raw reasoning."],
    )]));
    assert!(viewer.supports_raw(&summary_conversation));
    let mut updated_buffer = Buffer::empty(area);
    assert!(viewer.render(&mut updated_buffer, area, &summary_conversation));
    assert!(buffer_text(&updated_buffer, area).contains("r raw"));
    assert_eq!(
        viewer.handle_key_event(key(KeyCode::Char('r')), &summary_conversation),
        BlockViewerOutcome::Changed
    );
    let BlockViewerOutcome::Copy(raw) =
        viewer.handle_key_event(key(KeyCode::Char('y')), &summary_conversation)
    else {
        panic!("copy should follow the updated canonical raw body");
    };
    assert!(raw.contains("Canonical raw reasoning."));
    summary_conversation.reset_from_thread(&thread(Vec::new()));
    assert!(!viewer.is_available(&summary_conversation));

    let raw_only = conversation(vec![reasoning(
        "raw-thought",
        &[],
        &["Raw reasoning remains visible while the live item is open."],
    )]);
    let raw_id = raw_only.transcript().turns()[0].entries()[0].id();
    let mut raw_viewer = BlockViewerHost::open(&raw_only, SurfaceNodeId::Entry(raw_id))
        .expect("raw-only reasoning should open");
    assert!(raw_viewer.raw());
    assert!(!raw_viewer.supports_raw(&raw_only));
    let mut raw_buffer = Buffer::empty(area);
    assert!(raw_viewer.render(&mut raw_buffer, area, &raw_only));
    assert!(buffer_text(&raw_buffer, area).contains("Raw reasoning remains visible"));

    let opaque = conversation(vec![reasoning("opaque", &[], &[])]);
    let opaque_id = opaque.transcript().turns()[0].entries()[0].id();
    assert!(BlockViewerHost::open(&opaque, SurfaceNodeId::Entry(opaque_id)).is_none());
}

#[test]
fn viewer_scroll_copy_and_close_follow_one_canonical_document() {
    let markdown = (0..40)
        .map(|index| format!("- canonical line {index:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let conversation = conversation(vec![assistant("answer", &markdown)]);
    let entry_id = conversation.transcript().turns()[0].entries()[0].id();
    let mut viewer = BlockViewerHost::open(&conversation, SurfaceNodeId::Entry(entry_id))
        .expect("assistant markdown should open");
    let area = Rect::new(0, 0, 72, 28);
    let mut buffer = Buffer::empty(area);
    assert!(viewer.render(&mut buffer, area, &conversation));
    let first_page = buffer_text(&buffer, area);
    insta::assert_snapshot!("scrollable_assistant_viewer", first_page);

    let content = viewer.content_area.expect("viewer content geometry");
    assert_eq!(
        viewer.handle_mouse_event(
            mouse(MouseEventKind::ScrollDown, content.x, content.y),
            &conversation,
        ),
        BlockViewerOutcome::Changed
    );
    assert_eq!(viewer.scroll_offset, 3);

    let scrollbar = viewer.scrollbar_area.expect("scrollbar geometry");
    assert_eq!(
        viewer.handle_mouse_event(
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                scrollbar.x,
                scrollbar.bottom().saturating_sub(1),
            ),
            &conversation,
        ),
        BlockViewerOutcome::Changed
    );
    assert!(viewer.scrollbar_dragging);
    assert_eq!(viewer.scroll_offset, viewer.maximum_scroll());
    assert_eq!(
        viewer.handle_mouse_event(
            mouse(
                MouseEventKind::Drag(MouseButton::Left),
                scrollbar.x,
                scrollbar.y,
            ),
            &conversation,
        ),
        BlockViewerOutcome::Changed
    );
    assert_eq!(viewer.scroll_offset, 0);
    let _ = viewer.handle_mouse_event(
        mouse(
            MouseEventKind::Up(MouseButton::Left),
            scrollbar.x,
            scrollbar.y,
        ),
        &conversation,
    );
    assert!(!viewer.scrollbar_dragging);

    assert_eq!(
        viewer.handle_key_event(key(KeyCode::PageDown), &conversation),
        BlockViewerOutcome::Changed
    );
    assert!(viewer.scroll_offset > 0);
    let mut scrolled_buffer = Buffer::empty(area);
    assert!(viewer.render(&mut scrolled_buffer, area, &conversation));
    let second_page = buffer_text(&scrolled_buffer, area);
    assert_ne!(first_page, second_page);
    assert!(!second_page.contains("canonical line 00"));
    let BlockViewerOutcome::Copy(copied) =
        viewer.handle_key_event(key(KeyCode::Char('y')), &conversation)
    else {
        panic!("copy shortcut should return the canonical document");
    };
    assert!(copied.contains("canonical line 00"));
    assert!(copied.contains("canonical line 39"));
    assert_eq!(
        viewer.handle_key_event(key(KeyCode::Esc), &conversation),
        BlockViewerOutcome::Close
    );
}

#[test]
fn running_viewer_follows_new_content_until_the_user_moves_the_viewport() {
    let mut snapshot = thread(Vec::new());
    snapshot.turns.clear();
    let mut conversation = ConversationState::from_thread(&snapshot);
    let initial = lines("initial", 24);
    assert_eq!(
        conversation.apply(&agent_delta("assistant", &initial)),
        ApplyOutcome::Applied
    );
    let entry_id = conversation.transcript().turns()[0].entries()[0].id();
    let mut viewer = BlockViewerHost::open(&conversation, SurfaceNodeId::Entry(entry_id))
        .expect("running assistant entry should open");
    let area = Rect::new(0, 0, 64, 20);
    let mut buffer = Buffer::empty(area);
    assert!(viewer.render(&mut buffer, area, &conversation));
    let first_bottom = viewer.scroll_offset;
    assert!(viewer.follow_bottom);
    assert!(first_bottom > 0);

    assert_eq!(
        conversation.apply(&agent_delta("assistant", &lines("followed", 8))),
        ApplyOutcome::Applied
    );
    let mut followed_buffer = Buffer::empty(area);
    assert!(viewer.render(&mut followed_buffer, area, &conversation));
    assert!(viewer.scroll_offset > first_bottom);

    assert_eq!(
        viewer.handle_key_event(key(KeyCode::Up), &conversation),
        BlockViewerOutcome::Changed
    );
    let manual_offset = viewer.scroll_offset;
    assert!(!viewer.follow_bottom);
    assert_eq!(
        conversation.apply(&agent_delta("assistant", &lines("anchored", 8))),
        ApplyOutcome::Applied
    );
    let mut anchored_buffer = Buffer::empty(area);
    assert!(viewer.render(&mut anchored_buffer, area, &conversation));
    assert_eq!(viewer.scroll_offset, manual_offset);

    assert_eq!(
        viewer.handle_key_event(key(KeyCode::End), &conversation),
        BlockViewerOutcome::Changed
    );
    let resumed_bottom = viewer.scroll_offset;
    assert!(viewer.follow_bottom);
    assert_eq!(
        conversation.apply(&agent_delta("assistant", &lines("resumed", 8))),
        ApplyOutcome::Applied
    );
    let mut resumed_buffer = Buffer::empty(area);
    assert!(viewer.render(&mut resumed_buffer, area, &conversation));
    assert!(viewer.scroll_offset > resumed_bottom);
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
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

fn conversation(items: Vec<ThreadItem>) -> ConversationState {
    ConversationState::from_thread(&thread(items))
}

fn thread(items: Vec<ThreadItem>) -> Thread {
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
        turns: vec![Turn {
            id: "turn-1".to_string(),
            items,
            items_view: TurnItemsView::Full,
            status: TurnStatus::Completed,
            error: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
        }],
    }
}

fn reasoning(id: &str, summary: &[&str], content: &[&str]) -> ThreadItem {
    ThreadItem::Reasoning {
        id: id.to_string(),
        summary: summary.iter().map(|part| (*part).to_string()).collect(),
        content: content.iter().map(|part| (*part).to_string()).collect(),
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

fn agent_delta(item_id: &str, delta: &str) -> ServerNotification {
    ServerNotification::AgentMessageDelta(AgentMessageDeltaNotification {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-stream".to_string(),
        item_id: item_id.to_string(),
        delta: delta.to_string(),
    })
}

fn lines(prefix: &str, count: usize) -> String {
    (0..count)
        .map(|index| format!("{prefix} line {index:02}\n"))
        .collect()
}
