use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadListResponse;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use serde_json::json;

use super::PickerInput;
use super::PickerState;
use super::ThreadPickerAction;
use super::handle_key;
use super::render_picker;
use crate::view::AstralTheme;

fn thread(id: &str, name: Option<&str>, preview: &str, cwd: &str, updated_at: i64) -> Thread {
    serde_json::from_value(json!({
        "id": id,
        "sessionId": id,
        "forkedFromId": null,
        "parentThreadId": null,
        "preview": preview,
        "ephemeral": false,
        "modelProvider": "astral",
        "createdAt": 1,
        "updatedAt": updated_at,
        "status": {"type": "idle"},
        "path": null,
        "cwd": cwd,
        "cliVersion": "0.0.0",
        "source": "cli",
        "threadSource": "user",
        "agentNickname": null,
        "agentRole": null,
        "gitInfo": null,
        "name": name,
        "turns": []
    }))
    .expect("valid thread")
}

fn state() -> PickerState {
    PickerState::new(
        ThreadPickerAction::Resume,
        ThreadListResponse {
            data: vec![
                thread(
                    "thread-1",
                    Some("Astral TUI integration"),
                    "wire the new terminal",
                    "/workspace/astral-code",
                    200,
                ),
                thread(
                    "thread-2",
                    None,
                    "Fix daemon reconnect",
                    "/workspace/astral-code",
                    100,
                ),
            ],
            next_cursor: Some("next".to_string()),
            backwards_cursor: None,
        },
    )
}

#[test]
fn picker_snapshot() {
    let state = state();
    let area = Rect::new(0, 0, 72, 13);
    let mut buffer = Buffer::empty(area);
    render_picker(&state, area, &mut buffer, AstralTheme::default());

    insta::assert_snapshot!(buffer_text(&buffer));
}

#[test]
fn search_and_selection_use_visible_thread() {
    let mut state = state();

    assert!(matches!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('F'), KeyModifiers::NONE),
            13,
        ),
        PickerInput::Redraw
    ));
    assert_eq!(
        state.selected_thread().map(|thread| thread.id.as_str()),
        Some("thread-2")
    );
    assert!(matches!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            13,
        ),
        PickerInput::Select(thread) if thread.id == "thread-2"
    ));
}

#[test]
fn down_at_loaded_end_requests_next_page() {
    let mut state = state();
    state.selected = 1;

    assert!(matches!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            13,
        ),
        PickerInput::LoadNext
    ));
}

#[test]
fn down_with_unmatched_search_requests_next_page() {
    let mut state = state();
    state.query = "older session".to_string();

    assert!(matches!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            13,
        ),
        PickerInput::LoadNext
    ));
}

fn buffer_text(buffer: &Buffer) -> String {
    let area = buffer.area;
    (area.y..area.y + area.height)
        .map(|y| {
            let mut line = String::new();
            for x in area.x..area.x + area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            line.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}
