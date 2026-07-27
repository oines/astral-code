use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::ThemePickerInput;
use super::ThemePickerState;
use super::handle_key;
use super::render_picker;
use crate::view::AstralTheme;
use crate::view::AstralThemeId;

#[test]
fn theme_picker_previews_and_selects_explicit_variants() {
    let mut state = ThemePickerState::new(AstralThemeId::Night);
    assert_eq!(
        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        ThemePickerInput::Preview(AstralThemeId::Day)
    );
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        ),
        ThemePickerInput::Select(AstralThemeId::Day)
    );
}

#[test]
fn theme_picker_snapshot() {
    let state = ThemePickerState::new(AstralThemeId::Night);
    let area = Rect::new(0, 0, 80, 18);
    let mut buffer = Buffer::empty(area);
    render_picker(
        &state,
        area,
        &mut buffer,
        AstralTheme::for_id(AstralThemeId::Night),
    );

    insta::assert_snapshot!(buffer_text(&buffer));
}

fn buffer_text(buffer: &Buffer) -> String {
    let area = buffer.area;
    (area.y..area.y + area.height)
        .map(|y| {
            (area.x..area.x + area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}
