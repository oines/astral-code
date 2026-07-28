use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

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

#[test]
fn theme_picker_selected_row_uses_shared_modal_style() {
    let state = ThemePickerState::new(AstralThemeId::Night);
    let area = Rect::new(0, 0, 80, 18);
    let mut buffer = Buffer::empty(area);
    let theme = AstralTheme::for_id(AstralThemeId::Day);
    render_picker(&state, area, &mut buffer, theme);

    let selected = text_position(&buffer, "Night").expect("selected theme");
    let selected_cell = &buffer[selected];
    let row_fill = &buffer[(selected.0 + 24, selected.1)];
    let description =
        text_position(&buffer, "Dark palette with violet accents").expect("description");
    let unselected = text_position(&buffer, "Day").expect("unselected theme");

    assert_eq!(selected_cell.fg, theme.text_primary);
    assert_eq!(selected_cell.bg, theme.panel_selected);
    assert!(selected_cell.modifier.contains(Modifier::BOLD));
    assert_eq!(row_fill.bg, theme.panel_selected);
    assert_eq!(buffer[description].bg, theme.bg_base);
    assert_eq!(buffer[unselected].bg, theme.bg_base);
    assert!(!buffer[unselected].modifier.contains(Modifier::BOLD));

    insta::assert_snapshot!(
        "theme_picker_selected_style",
        style_summary(&buffer, selected, row_fill, description, unselected)
    );
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
