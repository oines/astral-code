use codex_protocol::models::BUILT_IN_PERMISSION_PROFILE_WORKSPACE;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::PermissionPickerInput;
use super::PermissionPickerState;
use super::PermissionSelection;
use super::handle_key;
use super::render_picker;
use crate::view::AstralTheme;

#[test]
fn permission_picker_snapshot() {
    let state = PermissionPickerState::new(Some(BUILT_IN_PERMISSION_PROFILE_WORKSPACE.to_string()));
    let area = Rect::new(0, 0, 80, 18);
    let mut buffer = Buffer::empty(area);
    render_picker(&state, area, &mut buffer, AstralTheme::default());

    insta::assert_snapshot!(buffer_text(&buffer));
}

#[test]
fn full_access_requires_confirmation() {
    let mut state = PermissionPickerState::new(None);
    assert_eq!(
        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        PermissionPickerInput::Redraw
    );
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        ),
        PermissionPickerInput::Redraw
    );
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)
        ),
        PermissionPickerInput::Select(PermissionSelection::FullAccess)
    );
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
