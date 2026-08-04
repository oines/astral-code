use std::time::Duration;
use std::time::Instant;

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
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::FullscreenHost;
use super::FullscreenOutcome;
use super::ScrollbackKeyMode;
use crate::ConversationState;
use crate::SurfaceNodeId;

#[test]
fn grok_key_and_double_click_actions_share_one_retained_surface() {
    let mut conversation = ConversationState::from_thread(&thread(vec![turn(vec![
        user("Inspect the interaction host"),
        reasoning(
            "reasoning",
            "Follow the canonical transcript and presentation state.",
        ),
        web_search("search-1", "grok fullscreen input routing"),
        web_search("search-2", "ratatui stable selection"),
        assistant(
            "answer",
            "The host routes interaction without reprojecting transcript data.",
        ),
    ])]));
    let area = Rect::new(0, 0, 64, 14);
    let mut host = FullscreenHost::new(&conversation, area, ScrollbackKeyMode::Vim);
    let entries = conversation.transcript().turns()[0].entries();
    let reasoning_id = entries[1].id();
    let answer_id = entries[4].id();

    assert_eq!(
        press(&mut host, &mut conversation, KeyCode::Char('j')),
        changed()
    );
    assert_eq!(
        press(&mut host, &mut conversation, KeyCode::Char('j')),
        changed()
    );
    assert_eq!(
        host.viewport().selected(),
        Some(SurfaceNodeId::VerbGroup(reasoning_id))
    );
    assert_eq!(
        press(&mut host, &mut conversation, KeyCode::Right),
        changed()
    );
    assert_eq!(
        press(&mut host, &mut conversation, KeyCode::Enter),
        changed()
    );
    assert_eq!(
        press(&mut host, &mut conversation, KeyCode::Char('j')),
        changed()
    );
    assert_eq!(
        press(&mut host, &mut conversation, KeyCode::Enter),
        FullscreenOutcome::OpenViewer(SurfaceNodeId::Entry(answer_id))
    );

    assert_eq!(
        press(&mut host, &mut conversation, KeyCode::Char('k')),
        changed()
    );
    assert_eq!(
        press(&mut host, &mut conversation, KeyCode::Right),
        changed()
    );
    let reasoning_row = host
        .surface()
        .node(SurfaceNodeId::Entry(reasoning_id))
        .expect("expanded reasoning entry")
        .rows()
        .start;
    let screen_row = area.y + reasoning_row.saturating_sub(host.viewport().top()) as u16;
    let now = Instant::now();
    click(&mut host, &mut conversation, 8, screen_row, now);
    click(
        &mut host,
        &mut conversation,
        8,
        screen_row,
        now + Duration::from_millis(100),
    );
    assert_eq!(
        host.viewport().selected(),
        Some(SurfaceNodeId::Entry(reasoning_id))
    );

    let mut buffer = Buffer::empty(area);
    host.render(&mut buffer);
    insta::assert_snapshot!(buffer_text(&buffer, area));

    assert_eq!(
        press(&mut host, &mut conversation, KeyCode::Char('/')),
        FullscreenOutcome::OpenSearch
    );
    host.set_key_mode(ScrollbackKeyMode::Simple);
    let key = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE);
    assert_eq!(
        host.handle_key_event(key, &mut conversation),
        FullscreenOutcome::ForwardToComposer(key)
    );

    let mut empty = ConversationState::from_thread(&thread(Vec::new()));
    let mut empty_host = FullscreenHost::new(&empty, area, ScrollbackKeyMode::Vim);
    let slash = KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE);
    assert_eq!(
        empty_host.handle_key_event(slash, &mut empty),
        FullscreenOutcome::ForwardToComposer(slash)
    );

    let short_area = Rect::new(0, 0, 30, 3);
    host.refresh_surface(&conversation, short_area);
    let down = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: short_area.right() - 1,
        row: short_area.bottom() - 1,
        modifiers: KeyModifiers::NONE,
    };
    assert_eq!(
        host.handle_mouse_event_at(down, Instant::now(), &mut conversation),
        changed()
    );
    assert_eq!(
        host.viewport().top(),
        host.surface().row_count() - usize::from(short_area.height)
    );
    assert!(!host.viewport().is_following_bottom());
}

fn press(
    host: &mut FullscreenHost,
    conversation: &mut ConversationState,
    code: KeyCode,
) -> FullscreenOutcome {
    host.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE), conversation)
}

fn click(
    host: &mut FullscreenHost,
    conversation: &mut ConversationState,
    column: u16,
    row: u16,
    now: Instant,
) {
    let down = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let up = MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        ..down
    };
    assert_eq!(
        host.handle_mouse_event_at(down, now, conversation),
        FullscreenOutcome::Unchanged
    );
    host.refresh_surface(conversation, host.area);
    assert_eq!(host.handle_mouse_event_at(up, now, conversation), changed());
}

fn changed() -> FullscreenOutcome {
    FullscreenOutcome::Changed
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

fn turn(items: Vec<ThreadItem>) -> Turn {
    Turn {
        id: "turn-1".to_string(),
        items,
        items_view: TurnItemsView::Full,
        status: TurnStatus::Completed,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
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
