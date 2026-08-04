//! Retained navigation and editing state for a typed MCP form.

use codex_app_server_protocol::McpElicitationSchema;
use serde_json::Map;
use serde_json::Value;

use super::field::McpFormControl;
use super::field::McpFormField;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum McpFormProgress {
    Advanced,
    Complete(Value),
    Invalid,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct McpFormModel {
    fields: Vec<McpFormField>,
    active: usize,
    error: Option<String>,
}

impl McpFormModel {
    pub(super) fn new(schema: &McpElicitationSchema) -> Self {
        let required = schema.required.as_deref().unwrap_or_default();
        let fields = schema
            .properties
            .iter()
            .map(|(name, schema)| {
                McpFormField::new(name, schema, required.iter().any(|field| field == name))
            })
            .collect();
        Self {
            fields,
            active: 0,
            error: None,
        }
    }

    pub(super) fn field_count(&self) -> usize {
        self.fields.len()
    }

    pub(super) fn active_index(&self) -> usize {
        self.active
    }

    pub(super) fn fields(&self) -> &[McpFormField] {
        &self.fields
    }

    pub(super) fn active_field(&self) -> Option<&McpFormField> {
        self.fields.get(self.active)
    }

    #[cfg(test)]
    pub(super) fn active_field_name(&self) -> Option<&str> {
        self.active_field().map(|field| field.name.as_str())
    }

    pub(super) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(super) fn move_field(&mut self, delta: i32) {
        if self.fields.len() < 2 {
            return;
        }
        self.active = (self.active as i32 + delta).rem_euclid(self.fields.len() as i32) as usize;
        self.error = None;
    }

    pub(super) fn set_active_index(&mut self, active: usize) {
        self.active = active.min(self.fields.len().saturating_sub(1));
        self.error = None;
    }

    pub(super) fn choice_count(&self) -> usize {
        match self.active_field().map(|field| &field.control) {
            Some(McpFormControl::Select { options, .. }) => options.len(),
            _ => 0,
        }
    }

    pub(super) fn choice_cursor(&self) -> usize {
        match self.active_field().map(|field| &field.control) {
            Some(McpFormControl::Select { cursor, .. }) => *cursor,
            _ => 0,
        }
    }

    pub(super) fn set_choice_cursor(&mut self, cursor: usize) {
        let Some(McpFormControl::Select {
            options,
            cursor: current,
            ..
        }) = self.active_control_mut()
        else {
            return;
        };
        *current = cursor.min(options.len().saturating_sub(1));
        self.error = None;
    }

    pub(super) fn move_choice(&mut self, delta: i32) -> bool {
        let count = self.choice_count();
        if count == 0 {
            return false;
        }
        let cursor = (self.choice_cursor() as i32 + delta).rem_euclid(count as i32) as usize;
        self.set_choice_cursor(cursor);
        true
    }

    pub(super) fn activate_choice(&mut self) {
        let Some(McpFormControl::Select {
            options,
            cursor,
            selected,
            multiple,
            committed,
            ..
        }) = self.active_control_mut()
        else {
            return;
        };
        if options.is_empty() {
            return;
        }
        if !*multiple {
            selected.clear();
        }
        if !selected.remove(cursor) {
            selected.insert(*cursor);
        }
        *committed = true;
        self.error = None;
    }

    pub(super) fn clear_active(&mut self) {
        match self.active_control_mut() {
            Some(McpFormControl::Text {
                draft,
                cursor,
                committed,
                ..
            }) => {
                draft.clear();
                *cursor = 0;
                *committed = false;
            }
            Some(McpFormControl::Select {
                selected,
                committed,
                ..
            }) => {
                selected.clear();
                *committed = false;
            }
            None => return,
        }
        self.error = None;
    }

    pub(super) fn insert_text(&mut self, text: &str) -> bool {
        let Some(McpFormControl::Text {
            draft,
            cursor,
            committed,
            ..
        }) = self.active_control_mut()
        else {
            return false;
        };
        draft.insert_str(*cursor, text);
        *cursor += text.len();
        *committed = true;
        self.error = None;
        true
    }

    pub(super) fn backspace(&mut self) -> bool {
        let Some(McpFormControl::Text {
            draft,
            cursor,
            committed,
            ..
        }) = self.active_control_mut()
        else {
            return false;
        };
        let previous = previous_boundary(draft, *cursor);
        if previous == *cursor {
            return false;
        }
        draft.drain(previous..*cursor);
        *cursor = previous;
        *committed = true;
        self.error = None;
        true
    }

    pub(super) fn delete(&mut self) -> bool {
        let Some(McpFormControl::Text {
            draft,
            cursor,
            committed,
            ..
        }) = self.active_control_mut()
        else {
            return false;
        };
        let next = next_boundary(draft, *cursor);
        if next == *cursor {
            return false;
        }
        draft.drain(*cursor..next);
        *committed = true;
        self.error = None;
        true
    }

    pub(super) fn move_text_cursor(&mut self, delta: i32) -> bool {
        let Some(McpFormControl::Text { draft, cursor, .. }) = self.active_control_mut() else {
            return false;
        };
        *cursor = if delta < 0 {
            previous_boundary(draft, *cursor)
        } else {
            next_boundary(draft, *cursor)
        };
        true
    }

    pub(super) fn move_text_cursor_to_edge(&mut self, end: bool) -> bool {
        let Some(McpFormControl::Text { draft, cursor, .. }) = self.active_control_mut() else {
            return false;
        };
        *cursor = if end { draft.len() } else { 0 };
        true
    }

    pub(super) fn advance_or_complete(&mut self) -> McpFormProgress {
        if self.fields.is_empty() {
            return McpFormProgress::Complete(Value::Object(Map::new()));
        }
        if let Err(error) = self.fields[self.active].validate() {
            self.error = Some(error);
            return McpFormProgress::Invalid;
        }
        if self.active + 1 < self.fields.len() {
            self.active += 1;
            self.error = None;
            return McpFormProgress::Advanced;
        }
        for (index, field) in self.fields.iter().enumerate() {
            if let Err(error) = field.validate() {
                self.active = index;
                self.error = Some(error);
                return McpFormProgress::Invalid;
            }
        }
        let content = self
            .fields
            .iter()
            .filter_map(|field| field.value().map(|value| (field.name.clone(), value)))
            .collect();
        McpFormProgress::Complete(Value::Object(content))
    }

    fn active_control_mut(&mut self) -> Option<&mut McpFormControl> {
        self.fields
            .get_mut(self.active)
            .map(|field| &mut field.control)
    }
}

fn previous_boundary(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_boundary(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .char_indices()
        .nth(1)
        .map_or(text.len(), |(index, _)| cursor + index)
}
