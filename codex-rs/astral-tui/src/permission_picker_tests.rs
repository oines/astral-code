use codex_protocol::models::BUILT_IN_PERMISSION_PROFILE_WORKSPACE;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::PermissionPickerInput;
use super::PermissionPickerState;
use super::PermissionSelection;
use super::handle_key;
use super::render_picker;
use crate::view::AstralTheme;

#[test]
fn permission_picker_snapshot() {
    let mut state =
        PermissionPickerState::new(Some(BUILT_IN_PERMISSION_PROFILE_WORKSPACE.to_string()));
    let area = Rect::new(0, 0, 80, 18);
    let mut buffer = Buffer::empty(area);
    render_picker(&mut state, area, &mut buffer, AstralTheme::default());

    insta::assert_snapshot!(buffer_text(&buffer));
}

#[test]
fn permission_picker_selected_row_uses_shared_modal_style() {
    let mut state =
        PermissionPickerState::new(Some(BUILT_IN_PERMISSION_PROFILE_WORKSPACE.to_string()));
    let area = Rect::new(0, 0, 80, 18);
    let mut buffer = Buffer::empty(area);
    let theme = AstralTheme::default();
    render_picker(&mut state, area, &mut buffer, theme);

    let selected = text_position(&buffer, "Workspace").expect("selected permission");
    let selected_cell = &buffer[selected];
    let row_fill = &buffer[(selected.0 + 24, selected.1)];
    let description = text_position(&buffer, "Edit workspace").expect("description");
    let unselected = text_position(&buffer, "Read only").expect("unselected permission");

    assert_eq!(selected_cell.fg, theme.text_primary);
    assert_eq!(selected_cell.bg, theme.panel_selected);
    assert!(selected_cell.modifier.contains(Modifier::BOLD));
    assert_eq!(row_fill.bg, theme.panel_selected);
    assert_eq!(buffer[description].bg, theme.bg_base);
    assert_eq!(buffer[unselected].bg, theme.bg_base);
    assert!(!buffer[unselected].modifier.contains(Modifier::BOLD));

    insta::assert_snapshot!(
        "permission_picker_selected_style",
        style_summary(&buffer, selected, row_fill, description, unselected)
    );
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

fn style_summary(
    buffer: &Buffer,
    selected: (u16, u16),
    row_fill: &ratatui::buffer::Cell,
    description: (u16, u16),
    unselected: (u16, u16),
) -> String {
    let selected = &buffer[selected];
    let description = &buffer[description];
    let unselected = &buffer[unselected];
    format!(
        "selected: fg={:?} bg={:?} modifiers={:?}\nrow-fill: bg={:?}\ndescription: bg={:?}\nunselected: bg={:?} modifiers={:?}",
        selected.fg,
        selected.bg,
        selected.modifier,
        row_fill.bg,
        description.bg,
        unselected.bg,
        unselected.modifier,
    )
}
