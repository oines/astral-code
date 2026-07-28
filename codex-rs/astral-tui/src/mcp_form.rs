//! Typed interaction state for MCP form elicitation requests.
//!
//! Form fields own their editor state so an elicitation never overwrites the
//! primary prompt composer.

mod field;
mod pointer;

use codex_app_server_protocol::McpElicitationSchema;
use codex_app_server_protocol::McpServerElicitationAction;
use codex_app_server_protocol::McpServerElicitationRequestResponse;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use serde_json::Map;
use serde_json::Value;

use crate::composer::ComposerState;

use self::pointer::McpFormPointerState;
pub(crate) use field::McpFormControl;
pub(crate) use field::McpFormField;

pub(crate) fn compile_fields(schema: &McpElicitationSchema) -> Vec<McpFormField> {
    crate::mcp_form_schema::project_fields(schema)
        .into_iter()
        .filter_map(|field| {
            schema
                .properties
                .get(&field.name)
                .map(|property| McpFormField::from_schema(field, property))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum McpFormEvent {
    None,
    Redraw,
    Submit(McpServerElicitationRequestResponse),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum McpFormHit {
    Choice(usize),
    Editor,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct McpFormState {
    schema: Option<McpElicitationSchema>,
    fields: Vec<McpFormField>,
    current: usize,
    editor: ComposerState,
    error: Option<String>,
    pointer: McpFormPointerState,
}

impl McpFormState {
    pub(crate) fn sync(&mut self, schema: &McpElicitationSchema) {
        if self.schema.as_ref() == Some(schema) {
            return;
        }
        self.schema = Some(schema.clone());
        self.fields = compile_fields(schema);
        self.current = 0;
        self.error = None;
        self.pointer.reset();
        self.load_editor();
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn current_field(&self) -> Option<&McpFormField> {
        self.fields.get(self.current)
    }

    pub(crate) fn current_index(&self) -> usize {
        self.current
    }

    pub(crate) fn field_count(&self) -> usize {
        self.fields.len()
    }

    pub(crate) fn editor(&self) -> &str {
        self.editor.text()
    }

    pub(crate) fn editor_cursor(&self) -> usize {
        self.editor.cursor()
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn handle_paste(&mut self, schema: &McpElicitationSchema, text: &str) -> bool {
        self.sync(schema);
        if text.is_empty() || !self.current_is_text() {
            return false;
        }
        self.editor.insert_text(text);
        self.error = None;
        true
    }

    pub(crate) fn handle_key(
        &mut self,
        schema: &McpElicitationSchema,
        key: KeyEvent,
    ) -> McpFormEvent {
        self.sync(schema);
        if key.code == KeyCode::Esc {
            return McpFormEvent::Submit(response(McpServerElicitationAction::Cancel, None));
        }
        if key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return McpFormEvent::Submit(response(McpServerElicitationAction::Decline, None));
        }
        if self.fields.is_empty() {
            return if key.code == KeyCode::Enter {
                McpFormEvent::Submit(response(
                    McpServerElicitationAction::Accept,
                    Some(Value::Object(Map::new())),
                ))
            } else {
                McpFormEvent::None
            };
        }
        if self.handle_field_navigation(key) {
            return McpFormEvent::Redraw;
        }

        match key.code {
            KeyCode::Enter => {
                self.commit_single_choice();
                self.advance_or_submit()
            }
            KeyCode::Up | KeyCode::Char('k') if !self.current_is_text() => {
                self.move_choice(/*next*/ false);
                McpFormEvent::Redraw
            }
            KeyCode::Down | KeyCode::Char('j') if !self.current_is_text() => {
                self.move_choice(/*next*/ true);
                McpFormEvent::Redraw
            }
            KeyCode::Char(' ') if !self.current_is_text() => {
                self.toggle_choice();
                McpFormEvent::Redraw
            }
            KeyCode::Backspace | KeyCode::Delete if !self.current_is_text() => {
                self.clear_choice();
                McpFormEvent::Redraw
            }
            KeyCode::Char(character) if !self.current_is_text() => {
                let Some(index) = self.choice_index_for_digit(character) else {
                    return McpFormEvent::None;
                };
                self.select_choice_at(index);
                self.advance_or_submit()
            }
            _ if self.current_is_text() && self.editor.edit_key(key) => {
                self.error = None;
                McpFormEvent::Redraw
            }
            _ => McpFormEvent::None,
        }
    }

    fn handle_field_navigation(&mut self, key: KeyEvent) -> bool {
        let previous = matches!(
            (key.code, key.modifiers),
            (KeyCode::PageUp, KeyModifiers::NONE) | (KeyCode::Char('p'), KeyModifiers::CONTROL)
        ) || self.fields.len() > 1
            && !self.current_is_text()
            && matches!(
                (key.code, key.modifiers),
                (KeyCode::Left | KeyCode::Char('h'), KeyModifiers::NONE)
            );
        let next = matches!(
            (key.code, key.modifiers),
            (KeyCode::PageDown, KeyModifiers::NONE) | (KeyCode::Char('n'), KeyModifiers::CONTROL)
        ) || self.fields.len() > 1
            && !self.current_is_text()
            && matches!(
                (key.code, key.modifiers),
                (KeyCode::Right | KeyCode::Char('l'), KeyModifiers::NONE)
            );
        if (!previous && !next) || self.fields.len() < 2 {
            return false;
        }
        self.save_editor();
        self.current = if previous {
            (self.current + self.fields.len() - 1) % self.fields.len()
        } else {
            (self.current + 1) % self.fields.len()
        };
        self.error = None;
        self.load_editor();
        true
    }

    fn move_choice(&mut self, next: bool) {
        let Some(McpFormControl::Select {
            choices, cursor, ..
        }) = self
            .fields
            .get_mut(self.current)
            .map(|field| &mut field.control)
        else {
            return;
        };
        if choices.is_empty() {
            return;
        }
        *cursor = if next {
            (*cursor + 1) % choices.len()
        } else {
            (*cursor + choices.len() - 1) % choices.len()
        };
        self.error = None;
    }

    fn toggle_choice(&mut self) {
        let Some(index) = self.choice_cursor() else {
            return;
        };
        self.toggle_choice_at(index);
    }

    fn toggle_choice_at(&mut self, index: usize) {
        let Some(McpFormControl::Select {
            choices,
            cursor,
            selected,
            multiple,
        }) = self
            .fields
            .get_mut(self.current)
            .map(|field| &mut field.control)
        else {
            return;
        };
        if index >= choices.len() {
            return;
        }
        *cursor = index;
        if !*multiple {
            selected.clear();
        }
        if !selected.remove(&index) {
            selected.insert(index);
        }
        self.error = None;
    }

    fn commit_single_choice(&mut self) {
        let required = self
            .current_field()
            .is_some_and(|field| field.schema.required);
        let Some(McpFormControl::Select {
            choices,
            cursor,
            selected,
            multiple: false,
        }) = self
            .fields
            .get_mut(self.current)
            .map(|field| &mut field.control)
        else {
            return;
        };
        if !choices.is_empty() && (required || !selected.is_empty()) {
            selected.clear();
            selected.insert(*cursor);
        }
    }

    fn select_choice_at(&mut self, index: usize) {
        let Some(McpFormControl::Select {
            choices,
            cursor,
            selected,
            multiple,
        }) = self
            .fields
            .get_mut(self.current)
            .map(|field| &mut field.control)
        else {
            return;
        };
        if index >= choices.len() {
            return;
        }
        *cursor = index;
        if !*multiple {
            selected.clear();
        }
        selected.insert(index);
        self.error = None;
    }

    fn choice_cursor(&self) -> Option<usize> {
        match self.current_field().map(|field| &field.control) {
            Some(McpFormControl::Select { cursor, .. }) => Some(*cursor),
            Some(McpFormControl::Text { .. }) | None => None,
        }
    }

    fn choice_index_for_digit(&self, character: char) -> Option<usize> {
        let index = usize::try_from(character.to_digit(10)?)
            .ok()?
            .checked_sub(1)?;
        match self.current_field().map(|field| &field.control) {
            Some(McpFormControl::Select { choices, .. }) if index < choices.len() => Some(index),
            Some(McpFormControl::Select { .. }) | Some(McpFormControl::Text { .. }) | None => None,
        }
    }

    fn clear_choice(&mut self) {
        if let Some(McpFormControl::Select { selected, .. }) = self
            .fields
            .get_mut(self.current)
            .map(|field| &mut field.control)
        {
            selected.clear();
            self.error = None;
        }
    }

    fn advance_or_submit(&mut self) -> McpFormEvent {
        self.save_editor();
        if let Err(error) = self.fields[self.current].validate() {
            self.error = Some(error);
            return McpFormEvent::Redraw;
        }
        if self.current + 1 < self.fields.len() {
            self.current += 1;
            self.error = None;
            self.load_editor();
            return McpFormEvent::Redraw;
        }
        for index in 0..self.fields.len() {
            if let Err(error) = self.fields[index].validate() {
                self.current = index;
                self.error = Some(error);
                self.load_editor();
                return McpFormEvent::Redraw;
            }
        }
        let content = self
            .fields
            .iter()
            .filter_map(|field| {
                field
                    .value()
                    .map(|value| (field.schema.name.clone(), value))
            })
            .collect();
        McpFormEvent::Submit(response(
            McpServerElicitationAction::Accept,
            Some(Value::Object(content)),
        ))
    }

    fn current_is_text(&self) -> bool {
        self.current_field()
            .is_some_and(|field| matches!(field.control, McpFormControl::Text { .. }))
    }

    fn save_editor(&mut self) {
        let value = self.editor.text().to_string();
        if let Some(McpFormControl::Text { value: field_value }) = self
            .fields
            .get_mut(self.current)
            .map(|field| &mut field.control)
        {
            *field_value = value;
        }
    }

    fn load_editor(&mut self) {
        let value = self
            .current_field()
            .and_then(|field| match &field.control {
                McpFormControl::Text { value } => Some(value.clone()),
                McpFormControl::Select { .. } => None,
            })
            .unwrap_or_default();
        self.editor.replace(value);
    }
}

fn response(
    action: McpServerElicitationAction,
    content: Option<Value>,
) -> McpServerElicitationRequestResponse {
    McpServerElicitationRequestResponse {
        action,
        content,
        meta: None,
    }
}

#[cfg(test)]
#[path = "mcp_form_tests.rs"]
mod tests;
