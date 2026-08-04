//! Grok-style visible presenter for typed MCP elicitation forms.

use std::time::Instant;

use codex_app_server_protocol::McpServerElicitationAction;
use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::McpServerElicitationRequestResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;

use super::field::McpFormControl;
use super::model::McpFormModel;
use super::model::McpFormProgress;
use crate::ModalOutcome;
use crate::ModalWindow;
use crate::prompt_interaction::PromptInteractionOutcome;
use crate::prompt_interaction::PromptInteractionSubmission;

mod render;

const DECLINE_ACTION: usize = 0;

pub(in crate::prompt_interaction) struct McpFormPrompt {
    request_id: RequestId,
    server_name: String,
    message: String,
    model: McpFormModel,
    window: ModalWindow,
}

impl McpFormPrompt {
    pub(in crate::prompt_interaction) fn from_request(request: &ServerRequest) -> Option<Self> {
        let ServerRequest::McpServerElicitationRequest { request_id, params } = request else {
            return None;
        };
        let McpServerElicitationRequest::Form {
            message,
            requested_schema,
            ..
        } = &params.request
        else {
            return None;
        };
        if requested_schema.properties.is_empty() {
            return None;
        }
        Some(Self {
            request_id: request_id.clone(),
            server_name: params.server_name.clone(),
            message: message.clone(),
            model: McpFormModel::new(requested_schema),
            window: ModalWindow::default(),
        })
    }

    pub(in crate::prompt_interaction) fn selected_index(&self) -> usize {
        self.model.active_index()
    }

    pub(in crate::prompt_interaction) fn set_selected_index(&mut self, selected: usize) {
        self.model.set_active_index(selected);
        self.window.set_active_tab(self.model.active_index());
    }

    pub(in crate::prompt_interaction) fn handle_key_event(
        &mut self,
        key: KeyEvent,
    ) -> PromptInteractionOutcome {
        if key.kind == KeyEventKind::Release {
            return PromptInteractionOutcome::Unchanged;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => return self.cancel(),
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => return self.decline(),
            (KeyCode::Char('p'), KeyModifiers::CONTROL)
            | (KeyCode::PageUp, KeyModifiers::NONE)
            | (KeyCode::BackTab, _) => return self.move_field(/*delta*/ -1),
            (KeyCode::Char('n'), KeyModifiers::CONTROL)
            | (KeyCode::PageDown | KeyCode::Tab, KeyModifiers::NONE) => {
                return self.move_field(/*delta*/ 1);
            }
            _ => {}
        }
        match self.model.active_field().map(|field| &field.control) {
            Some(McpFormControl::Text { .. }) => self.handle_text_key(key),
            Some(McpFormControl::Select { .. }) => self.handle_select_key(key),
            None if key.code == KeyCode::Enter => self.advance_or_submit(),
            None => PromptInteractionOutcome::Unchanged,
        }
    }

    pub(in crate::prompt_interaction) fn handle_mouse_event_at(
        &mut self,
        mouse: MouseEvent,
        _now: Instant,
    ) -> PromptInteractionOutcome {
        match self.window.handle_mouse_event(mouse) {
            ModalOutcome::CloseRequested => return self.cancel(),
            ModalOutcome::TabChanged(tab) => {
                self.model.set_active_index(tab);
                return PromptInteractionOutcome::Changed;
            }
            ModalOutcome::ShortcutActivated(DECLINE_ACTION) => return self.decline(),
            ModalOutcome::Handled | ModalOutcome::ShortcutActivated(_) => {
                return PromptInteractionOutcome::Changed;
            }
            ModalOutcome::Unhandled => {}
        }
        PromptInteractionOutcome::Unchanged
    }

    pub(in crate::prompt_interaction) fn handle_paste(
        &mut self,
        text: &str,
    ) -> PromptInteractionOutcome {
        if !text.is_empty() && self.model.insert_text(text) {
            PromptInteractionOutcome::Changed
        } else {
            PromptInteractionOutcome::Unchanged
        }
    }

    fn handle_text_key(&mut self, key: KeyEvent) -> PromptInteractionOutcome {
        let changed = match (key.code, key.modifiers) {
            (KeyCode::Enter, KeyModifiers::NONE) => return self.advance_or_submit(),
            (KeyCode::Enter, KeyModifiers::SHIFT) => self.model.insert_text("\n"),
            (KeyCode::Left, KeyModifiers::NONE) => self.model.move_text_cursor(/*delta*/ -1),
            (KeyCode::Right, KeyModifiers::NONE) => self.model.move_text_cursor(/*delta*/ 1),
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.model.move_text_cursor_to_edge(/*end*/ false)
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.model.move_text_cursor_to_edge(/*end*/ true)
            }
            (KeyCode::Backspace, KeyModifiers::NONE) => self.model.backspace(),
            (KeyCode::Delete, KeyModifiers::NONE) => self.model.delete(),
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                self.model.clear_active();
                true
            }
            (KeyCode::Char(character), modifiers)
                if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT =>
            {
                self.model.insert_text(&character.to_string())
            }
            _ => false,
        };
        if changed {
            PromptInteractionOutcome::Changed
        } else {
            PromptInteractionOutcome::Unchanged
        }
    }

    fn handle_select_key(&mut self, key: KeyEvent) -> PromptInteractionOutcome {
        match (key.code, key.modifiers) {
            (KeyCode::Up | KeyCode::Char('k'), KeyModifiers::NONE) => {
                self.model.move_choice(/*delta*/ -1);
            }
            (KeyCode::Down | KeyCode::Char('j'), KeyModifiers::NONE) => {
                self.model.move_choice(/*delta*/ 1);
            }
            (KeyCode::Left | KeyCode::Char('h'), KeyModifiers::NONE) => {
                return self.move_field(/*delta*/ -1);
            }
            (KeyCode::Right | KeyCode::Char('l'), KeyModifiers::NONE) => {
                return self.move_field(/*delta*/ 1);
            }
            (KeyCode::Char(' '), KeyModifiers::NONE) => self.model.activate_choice(),
            (KeyCode::Backspace | KeyCode::Delete, KeyModifiers::NONE) => {
                self.model.clear_active();
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                if self.single_select_needs_choice() {
                    self.model.activate_choice();
                }
                return self.advance_or_submit();
            }
            (KeyCode::Char(character @ '1'..='9'), KeyModifiers::NONE) => {
                let index = usize::from(character as u8 - b'1');
                if index >= self.model.choice_count() {
                    return PromptInteractionOutcome::Unchanged;
                }
                self.model.set_choice_cursor(index);
                self.model.activate_choice();
                if self.active_select_is_multiple() {
                    return PromptInteractionOutcome::Changed;
                }
                return self.advance_or_submit();
            }
            _ => return PromptInteractionOutcome::Unchanged,
        }
        PromptInteractionOutcome::Changed
    }

    fn move_field(&mut self, delta: i32) -> PromptInteractionOutcome {
        if self.model.field_count() < 2 {
            return PromptInteractionOutcome::Unchanged;
        }
        self.model.move_field(delta);
        self.window.set_active_tab(self.model.active_index());
        PromptInteractionOutcome::Changed
    }

    fn advance_or_submit(&mut self) -> PromptInteractionOutcome {
        match self.model.advance_or_complete() {
            McpFormProgress::Advanced | McpFormProgress::Invalid => {
                self.window.set_active_tab(self.model.active_index());
                PromptInteractionOutcome::Changed
            }
            McpFormProgress::Complete(content) => {
                self.submit(McpServerElicitationAction::Accept, Some(content))
            }
        }
    }

    fn cancel(&self) -> PromptInteractionOutcome {
        self.submit(McpServerElicitationAction::Cancel, None)
    }

    fn decline(&self) -> PromptInteractionOutcome {
        self.submit(McpServerElicitationAction::Decline, None)
    }

    fn submit(
        &self,
        action: McpServerElicitationAction,
        content: Option<serde_json::Value>,
    ) -> PromptInteractionOutcome {
        match serde_json::to_value(McpServerElicitationRequestResponse {
            action,
            content,
            meta: None,
        }) {
            Ok(result) => PromptInteractionOutcome::Submit(PromptInteractionSubmission {
                request_id: self.request_id.clone(),
                result,
            }),
            Err(error) => PromptInteractionOutcome::Failed(format!(
                "failed to serialize MCP form elicitation response: {error}"
            )),
        }
    }

    fn single_select_needs_choice(&self) -> bool {
        self.model.active_field().is_some_and(|field| {
            field.required
                && matches!(
                    &field.control,
                    McpFormControl::Select { selected, multiple: false, .. }
                        if selected.is_empty()
                )
        })
    }

    fn active_select_is_multiple(&self) -> bool {
        matches!(
            self.model.active_field().map(|field| &field.control),
            Some(McpFormControl::Select { multiple: true, .. })
        )
    }
}
