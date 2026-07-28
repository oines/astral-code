//! Current Astral TUI keyboard bindings shown by Ctrl+.

use crate::modal::ModalRow;
use crate::modal::ModalState;

pub(crate) fn shortcuts_modal() -> ModalState {
    ModalState::info(
        "Keyboard shortcuts",
        vec![
            ModalRow::new("Enter", "Send / submit focused input"),
            ModalRow::new("Shift+Tab", "Cycle collaboration mode"),
            ModalRow::new("Ctrl+.", "Toggle shortcuts window"),
            ModalRow::new("Ctrl+C", "Interrupt / clear / idle exit"),
            ModalRow::new("Ctrl+D", "Exit when the composer is empty"),
            ModalRow::new("Ctrl+O", "Copy the last agent response"),
            ModalRow::new("PageUp", "Scroll history up"),
            ModalRow::new("PageDown", "Scroll history down"),
            ModalRow::new("/", "Open slash-command discovery"),
            ModalRow::new("↑/↓", "Navigate menus / scroll lists"),
            ModalRow::new("Tab", "Complete item / switch focus"),
            ModalRow::new("j/k", "Select foldable entry"),
            ModalRow::new("e / Enter", "Toggle selected entry"),
            ModalRow::new("h/l", "Collapse / expand entry"),
            ModalRow::new("E", "Expand all / collapse all entries"),
            ModalRow::new("Ctrl+E", "Expand / collapse all thinking"),
            ModalRow::new("Esc", "Close the focused menu or modal"),
            ModalRow::new("Y", "Approve a pending request"),
            ModalRow::new("A", "Approve for the session when available"),
            ModalRow::new("N", "Decline a pending request"),
        ],
    )
}
