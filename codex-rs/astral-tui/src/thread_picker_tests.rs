use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadListResponse;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use serde_json::json;

use super::PickerInput;
use super::PickerState;
use super::ThreadPickerAction;
use super::handle_key;
use super::render_picker;
use crate::view::AstralTheme;
use crate::view::AstralThemeId;

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
    let theme = AstralTheme::default();
    render_picker(&state, area, &mut buffer, theme);

    insta::assert_snapshot!(buffer_text(&buffer));
}

#[test]
fn picker_owns_row_colors_and_omits_raw_timestamps() {
    let mut state = state();
    state.set_notice("load failed");
    let area = Rect::new(0, 0, 72, 13);
    let mut buffer = Buffer::empty(area);
    let theme = AstralTheme::for_id(AstralThemeId::Day);
    render_picker(&state, area, &mut buffer, theme);

    let rendered = buffer_text(&buffer);
    assert!(!rendered.contains("updated"));
    assert!(!rendered.contains("200"));
    assert!(!rendered.contains("100"));

    let diamonds = symbol_positions(&buffer, "◆");
    assert_eq!(diamonds.len(), 2);
    assert_eq!(buffer[diamonds[0]].fg, theme.gray_dim);
    assert_eq!(buffer[diamonds[0]].bg, theme.panel_selected);
    assert_eq!(
        buffer[(diamonds[0].0 + 35, diamonds[0].1)].bg,
        theme.panel_selected
    );
    assert_eq!(buffer[diamonds[1]].bg, theme.bg_base);

    let selected_title = text_position(&buffer, "Astral TUI integration").expect("selected title");
    assert_eq!(buffer[selected_title].fg, theme.text_primary);
    assert_eq!(buffer[selected_title].bg, theme.panel_selected);
    assert!(buffer[selected_title].modifier.contains(Modifier::BOLD));

    let cwd = text_position(&buffer, "/workspace/astral-code").expect("cwd metadata");
    assert_eq!(buffer[cwd].fg, theme.gray);
    assert_eq!(buffer[cwd].bg, theme.bg_base);

    let unselected_title =
        text_position(&buffer, "Fix daemon reconnect").expect("unselected title");
    assert_eq!(buffer[unselected_title].fg, theme.text_primary);
    assert_eq!(buffer[unselected_title].bg, theme.bg_base);
    assert!(!buffer[unselected_title].modifier.contains(Modifier::BOLD));

    let search = text_position(&buffer, "Search:").expect("search label");
    assert_eq!(buffer[search].fg, theme.gray);
    let count = text_position(&buffer, "1/2").expect("visible count");
    assert_eq!(buffer[count].fg, theme.gray);
    let more = text_position(&buffer, "more available").expect("pagination hint");
    assert_eq!(buffer[more].fg, theme.accent_running);
    let notice = text_position(&buffer, "load failed").expect("error notice");
    assert_eq!(buffer[notice].fg, theme.accent_error);
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

fn symbol_positions(buffer: &Buffer, symbol: &str) -> Vec<(u16, u16)> {
    let area = buffer.area;
    let mut positions = Vec::new();
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if buffer[(x, y)].symbol() == symbol {
                positions.push((x, y));
            }
        }
    }
    positions
}

fn text_position(buffer: &Buffer, text: &str) -> Option<(u16, u16)> {
    let symbols = text
        .chars()
        .map(|character| character.to_string())
        .collect::<Vec<_>>();
    let width = u16::try_from(symbols.len()).ok()?;
    let area = buffer.area;
    for y in area.y..area.bottom() {
        for x in area.x..area.right().saturating_sub(width).saturating_add(1) {
            if (x..x.saturating_add(width))
                .zip(&symbols)
                .all(|(column, symbol)| buffer[(column, y)].symbol() == symbol)
            {
                return Some((x, y));
            }
        }
    }
    None
}
