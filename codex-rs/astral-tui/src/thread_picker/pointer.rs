use crossterm::event::MouseEvent;

use crate::modal::ModalPointerAction;

use super::PickerInput;
use super::PickerState;

pub(crate) fn handle_mouse(state: &mut PickerState, mouse: MouseEvent) -> PickerInput {
    match state.pointer.handle_mouse(mouse) {
        ModalPointerAction::Ignored => PickerInput::None,
        ModalPointerAction::Redraw | ModalPointerAction::Hover(None) => PickerInput::Redraw,
        ModalPointerAction::Close => PickerInput::Cancel,
        ModalPointerAction::Hover(Some(index)) => {
            state.selected = index.min(state.filtered_indices().len().saturating_sub(1));
            PickerInput::Redraw
        }
        ModalPointerAction::Activate(index) => {
            state.selected = index.min(state.filtered_indices().len().saturating_sub(1));
            state
                .selected_thread()
                .cloned()
                .map(Box::new)
                .map(PickerInput::Select)
                .unwrap_or(PickerInput::None)
        }
        ModalPointerAction::Scroll(delta) if delta < 0 => {
            state.page_up(delta.unsigned_abs());
            PickerInput::Redraw
        }
        ModalPointerAction::Scroll(delta) => {
            state.page_down(delta.unsigned_abs());
            PickerInput::Redraw
        }
    }
}
