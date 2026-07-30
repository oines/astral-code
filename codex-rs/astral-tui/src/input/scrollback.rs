use crossterm::event::KeyEvent;

use crate::InputAction;
use crate::SurfaceState;
use crate::actions;
use crate::actions::ActionId;
use crate::actions::When;

pub(super) fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    if state.handle_scrollback_search_key(key).is_some() {
        return InputAction::Redraw;
    }
    match actions::lookup(&key, When::ScrollbackFocused) {
        Some(ActionId::OpenTranscriptSearch) => {
            if state.open_scrollback_search() {
                InputAction::Redraw
            } else {
                InputAction::None
            }
        }
        Some(ActionId::FocusPrompt) => {
            state.focus_prompt();
            InputAction::Redraw
        }
        Some(ActionId::ModelPicker) => {
            if state.open_model_picker() {
                InputAction::Redraw
            } else {
                InputAction::Notice(
                    "Model selection is unavailable while Astral is working".to_string(),
                )
            }
        }
        Some(ActionId::PreviousTurn) => {
            state.previous_turn();
            InputAction::Redraw
        }
        Some(ActionId::NextTurn) => {
            state.next_turn();
            InputAction::Redraw
        }
        Some(ActionId::NextResponse) => {
            state.next_response();
            InputAction::Redraw
        }
        Some(ActionId::PreviousResponse) => {
            state.previous_response();
            InputAction::Redraw
        }
        Some(ActionId::GoToTop) => {
            state.goto_scrollback_top();
            InputAction::Redraw
        }
        Some(ActionId::GoToBottom) => {
            state.goto_scrollback_bottom();
            InputAction::Redraw
        }
        Some(ActionId::ScrollLineUp) => {
            state.scroll_up(/* lines */ 1);
            InputAction::Redraw
        }
        Some(ActionId::ScrollLineDown) => {
            state.scroll_down(/* lines */ 1);
            InputAction::Redraw
        }
        Some(ActionId::HalfPageUp) => {
            state.half_page_up();
            InputAction::Redraw
        }
        Some(ActionId::HalfPageDown) => {
            state.half_page_down();
            InputAction::Redraw
        }
        Some(ActionId::SelectNext) => {
            state.move_entry_selection(1);
            InputAction::Redraw
        }
        Some(ActionId::SelectPrevious) => {
            state.move_entry_selection(-1);
            InputAction::Redraw
        }
        Some(ActionId::CollapseEntry) => {
            state.collapse_selected_entry();
            InputAction::Redraw
        }
        Some(ActionId::ExpandEntry) => {
            state.expand_selected_entry();
            InputAction::Redraw
        }
        Some(ActionId::ToggleEntry) => {
            state.toggle_selected_entry();
            InputAction::Redraw
        }
        Some(ActionId::ToggleAllEntries) => {
            state.toggle_all_entries();
            InputAction::Redraw
        }
        Some(ActionId::ToggleAllReasoning) => {
            state.toggle_all_thinking();
            InputAction::Redraw
        }
        Some(ActionId::ToggleRawMarkdown) => {
            if state.toggle_selected_raw() {
                InputAction::Redraw
            } else {
                InputAction::None
            }
        }
        Some(ActionId::CopyBlockContent) => {
            state
                .selected_copy_text()
                .map_or(InputAction::None, |text| InputAction::CopyText {
                    text,
                    notice: "Copied block content".to_string(),
                })
        }
        Some(ActionId::CopyBlockMetadata) => {
            state
                .selected_copy_meta()
                .map_or(InputAction::None, |text| InputAction::CopyText {
                    text,
                    notice: "Copied block metadata".to_string(),
                })
        }
        Some(ActionId::NextLink) => {
            if state.cycle_scrollback_link(true) {
                InputAction::Redraw
            } else {
                InputAction::None
            }
        }
        Some(ActionId::PreviousLink) => {
            if state.cycle_scrollback_link(false) {
                InputAction::Redraw
            } else {
                InputAction::None
            }
        }
        Some(ActionId::OpenEntry) => {
            if let Some(target) = state.highlighted_scrollback_link() {
                InputAction::OpenLink(target)
            } else if let Some(thread_id) = state.selected_subagent_thread_id() {
                InputAction::OpenSubagent { thread_id }
            } else if state.open_selected_entry() {
                InputAction::Redraw
            } else {
                InputAction::None
            }
        }
        Some(ActionId::PageUp) => InputAction::ScrollUp,
        Some(ActionId::PageDown) => InputAction::ScrollDown,
        Some(ActionId::CycleMode) => InputAction::CycleMode,
        Some(ActionId::ShortcutsHelp) => InputAction::OpenShortcuts,
        Some(ActionId::ScrollbackCancel) => {
            if state.activity().is_running() {
                InputAction::Interrupt
            } else {
                InputAction::Exit
            }
        }
        Some(
            ActionId::CommandPalette
            | ActionId::ToggleMultiline
            | ActionId::OpenSessions
            | ActionId::NewSession
            | ActionId::ShellMode
            | ActionId::ToggleQueue
            | ActionId::FocusScrollback
            | ActionId::SendPrompt
            | ActionId::InterjectPrompt
            | ActionId::OpenExternalEditor
            | ActionId::PromptCancel
            | ActionId::ExitEmptyPrompt
            | ActionId::CopyLastResponse,
        )
        | None => InputAction::None,
    }
}
