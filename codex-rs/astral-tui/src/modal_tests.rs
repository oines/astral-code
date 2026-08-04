use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::ModalOutcome;
use super::ModalPresentation;
use super::ModalShortcut;
use super::ModalSizing;
use super::ModalWindow;
use super::ModalWindowConfig;

#[test]
fn responsive_chrome_shares_layout_and_routes_only_its_own_input() {
    let area = Rect::new(0, 0, 64, 16);
    let tabs = [
        "General",
        "Models & Providers",
        "Tools & Search",
        "Permissions & Safety",
    ];
    let shortcuts = [
        ModalShortcut::action(7, "Enter select"),
        ModalShortcut::hint("↑/↓ navigate"),
        ModalShortcut::action(9, "Esc close"),
    ];
    let config = ModalWindowConfig::new("Settings")
        .with_tabs(&tabs)
        .with_shortcuts(&shortcuts)
        .with_sizing(ModalSizing::medium());
    let mut window = ModalWindow::default();
    let mut popup = Buffer::empty(area);
    for y in area.y..area.bottom() {
        popup.set_string(
            area.x,
            y,
            ".".repeat(usize::from(area.width)),
            Style::default(),
        );
    }
    let layout = window
        .render(&mut popup, area, &config)
        .expect("popup should fit");
    popup.set_string(
        layout.content.x,
        layout.content.y,
        "Presenter content stays independent",
        Style::default(),
    );

    let models = find_text(&popup, area, "Models & Providers");
    let left = MouseEventKind::Down(MouseButton::Left);
    assert_mouse(&mut window, left, models, ModalOutcome::TabChanged(1));
    assert_eq!(window.active_tab(), 1);

    let select = find_text(&popup, area, "Enter select");
    assert_mouse(
        &mut window,
        MouseEventKind::Moved,
        select,
        ModalOutcome::Handled,
    );
    assert_mouse(
        &mut window,
        left,
        select,
        ModalOutcome::ShortcutActivated(7),
    );
    assert_eq!(
        window.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        ModalOutcome::CloseRequested
    );

    let close = find_text(&popup, area, "[×]");
    assert_mouse(&mut window, left, close, ModalOutcome::CloseRequested);
    assert_mouse(
        &mut window,
        left,
        (area.x, area.y),
        ModalOutcome::CloseRequested,
    );

    let popup_snapshot = buffer_text(&popup, area);
    let narrow = Rect::new(0, 0, 19, 5);
    assert_eq!(window.render(&mut popup, narrow, &config), None);
    assert_mouse(&mut window, left, close, ModalOutcome::Unhandled);

    let embedded_area = Rect::new(0, 0, 42, 8);
    let embedded_tabs = ["Question 1", "Question 2"];
    let embedded_config = ModalWindowConfig::new("Ask Astral")
        .with_tabs(&embedded_tabs)
        .with_shortcuts(&shortcuts)
        .with_sizing(ModalSizing::medium().compact())
        .with_presentation(ModalPresentation::Embedded);
    let mut embedded = Buffer::empty(embedded_area);
    let embedded_layout = window
        .render(&mut embedded, embedded_area, &embedded_config)
        .expect("embedded chrome should fit");
    embedded.set_string(
        embedded_layout.content.x,
        embedded_layout.content.y,
        "Prompt-area presenter content",
        Style::default(),
    );
    insta::assert_snapshot!(format!(
        "POPUP\n{popup_snapshot}\n\nEMBEDDED\n{}",
        buffer_text(&embedded, embedded_area)
    ));
}

fn assert_mouse(
    window: &mut ModalWindow,
    kind: MouseEventKind,
    position: (u16, u16),
    expected: ModalOutcome,
) {
    assert_eq!(window.handle_mouse_event(mouse(kind, position)), expected);
}

fn mouse(kind: MouseEventKind, position: (u16, u16)) -> MouseEvent {
    MouseEvent {
        kind,
        column: position.0,
        row: position.1,
        modifiers: KeyModifiers::NONE,
    }
}

fn find_text(buffer: &Buffer, area: Rect, needle: &str) -> (u16, u16) {
    for y in area.y..area.bottom() {
        let row = (area.x..area.right())
            .map(|x| buffer.cell((x, y)).expect("cell in area").symbol())
            .collect::<String>();
        if let Some(column) = row.find(needle) {
            return (area.x + column as u16, y);
        }
    }
    panic!("text not rendered: {needle}");
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
