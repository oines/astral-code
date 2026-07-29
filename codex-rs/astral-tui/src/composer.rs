//! Editable prompt state for the Astral surface.
//!
//! The cursor is a UTF-8 byte offset, matching the convention used by the
//! Grok Build prompt widget. All mutations keep it on a character boundary.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use crate::mention::MentionTarget;
use crate::mention::PromptSubmission;

mod edit;
mod element;
mod history;
mod selection;

use edit::byte_at_column;
use edit::line_end;
use edit::line_start;
use edit::next_boundary;
use edit::previous_boundary;
use edit::small_word_end_right;
use edit::small_word_start_left;
use edit::whitespace_word_start_left;
pub(crate) use element::ComposerElement;
use history::EditHistory;
use history::MutationKind;
use selection::ClickTracker;
pub(crate) use selection::ComposerMouseAction;
use selection::Selection;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ComposerState {
    text: String,
    cursor: usize,
    preferred_column: Option<usize>,
    kill_buffer: String,
    history: EditHistory,
    elements: Vec<ComposerElement>,
    selection: Option<Selection>,
    drag_anchor: Option<usize>,
    drag_active: bool,
    click_tracker: ClickTracker,
}

impl ComposerState {
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn has_structured_mentions(&self) -> bool {
        !self.elements.is_empty()
    }

    pub(crate) fn replace(&mut self, text: impl Into<String>) {
        let text = text.into();
        if self.text == text && self.cursor == text.len() && self.elements.is_empty() {
            self.clear_selection_state();
            return;
        }
        self.begin_mutation(MutationKind::Replace);
        self.text = text;
        self.cursor = self.text.len();
        self.preferred_column = None;
        self.elements.clear();
        self.finish_mutation();
        self.clear_selection_state();
    }

    pub(crate) fn take(&mut self) -> String {
        if self.text.is_empty() && self.cursor == 0 && self.elements.is_empty() {
            self.clear_selection_state();
            return String::new();
        }
        self.begin_mutation(MutationKind::Replace);
        self.cursor = 0;
        self.preferred_column = None;
        self.elements.clear();
        let text = std::mem::take(&mut self.text);
        self.finish_mutation();
        self.clear_selection_state();
        text
    }

    pub(crate) fn take_submission(&mut self) -> PromptSubmission {
        if self.text.is_empty() && self.cursor == 0 && self.elements.is_empty() {
            self.clear_selection_state();
            return PromptSubmission::text_only(String::new());
        }
        self.begin_mutation(MutationKind::Replace);
        let elements = self
            .elements
            .drain(..)
            .filter(|element| element.matches_text(&self.text))
            .collect();
        self.cursor = 0;
        self.preferred_column = None;
        let submission = PromptSubmission {
            text: std::mem::take(&mut self.text),
            elements,
        };
        self.finish_mutation();
        self.clear_selection_state();
        submission
    }

    pub(crate) fn restore_submission(&mut self, submission: PromptSubmission) {
        if self.text == submission.text
            && self.cursor == submission.text.len()
            && self.elements == submission.elements
        {
            self.clear_selection_state();
            return;
        }
        self.begin_mutation(MutationKind::Replace);
        self.text = submission.text;
        self.elements = submission.elements;
        self.cursor = self.text.len();
        self.preferred_column = None;
        self.finish_mutation();
        self.clear_selection_state();
    }

    pub(crate) fn clear(&mut self) {
        if self.text.is_empty() && self.cursor == 0 && self.elements.is_empty() {
            self.clear_selection_state();
            return;
        }
        self.begin_mutation(MutationKind::Replace);
        self.text.clear();
        self.cursor = 0;
        self.preferred_column = None;
        self.elements.clear();
        self.finish_mutation();
        self.clear_selection_state();
    }

    pub(crate) fn insert_char(&mut self, character: char) {
        self.insert_text(&character.to_string());
    }

    pub(crate) fn insert_text(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        if normalized.is_empty() {
            return;
        }
        if let Some(range) = self.selection_range() {
            self.replace_range(range, &normalized, MutationKind::Replace);
            return;
        }
        self.clear_selection_state();
        self.history.break_insert_batch_at(&normalized);
        self.replace_range(self.cursor..self.cursor, &normalized, MutationKind::Insert);
        self.history.record_inserted_text(&normalized);
    }

    pub(crate) fn insert_mention(
        &mut self,
        range: std::ops::Range<usize>,
        insert_text: String,
        target: MentionTarget,
    ) {
        let start = range.start;
        let inserted = format!("{insert_text} ");
        self.replace_range(range, &inserted, MutationKind::Replace);
        self.elements.push(ComposerElement::mention(
            start..start + insert_text.len(),
            insert_text,
            target,
        ));
    }

    pub(crate) fn edit_key(&mut self, key: KeyEvent) -> bool {
        if self.selection_range().is_some() {
            match (key.code, key.modifiers) {
                (KeyCode::Char('j' | 'm'), KeyModifiers::CONTROL) => {
                    self.insert_char('\n');
                    return true;
                }
                (
                    KeyCode::Backspace | KeyCode::Delete | KeyCode::Char('\u{0008}' | '\u{007f}'),
                    _,
                )
                | (KeyCode::Char('h' | 'd'), KeyModifiers::CONTROL) => {
                    return self.delete_selection();
                }
                (KeyCode::Char(character), modifiers)
                    if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) =>
                {
                    self.insert_char(character);
                    return true;
                }
                _ => {}
            }
        }
        let selection_cleared = self.clear_selection_state();
        let word_modifier = key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let changed = match (key.code, key.modifiers) {
            (KeyCode::Char('Z'), modifiers)
                if modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) =>
            {
                self.redo()
            }
            (KeyCode::Char('z'), modifiers)
                if modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) =>
            {
                self.undo()
            }
            (KeyCode::Char('r'), KeyModifiers::CONTROL) => self.redo(),
            (KeyCode::Char('a'), KeyModifiers::CONTROL) => self.move_readline_line_start(),
            (KeyCode::Char('e'), KeyModifiers::CONTROL) => self.move_readline_line_end(),
            (KeyCode::Char('b'), KeyModifiers::CONTROL)
            | (KeyCode::Char('\u{0002}'), KeyModifiers::NONE) => self.move_left(),
            (KeyCode::Char('f'), KeyModifiers::CONTROL)
            | (KeyCode::Char('\u{0006}'), KeyModifiers::NONE) => self.move_right(),
            (KeyCode::Char('p'), KeyModifiers::CONTROL) => self.move_up(),
            (KeyCode::Char('n'), KeyModifiers::CONTROL) => self.move_down(),
            (KeyCode::Char('b'), KeyModifiers::ALT) => self.move_word_left(),
            (KeyCode::Char('f'), KeyModifiers::ALT) => self.move_word_right(),
            (KeyCode::Char('w'), KeyModifiers::CONTROL) => self.delete_word_left(),
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => self.kill_to_line_start(),
            (KeyCode::Char('k'), KeyModifiers::CONTROL) => self.kill_to_line_end(),
            (KeyCode::Char('y'), KeyModifiers::CONTROL) => self.yank(),
            (KeyCode::Char('h'), modifiers)
                if modifiers == KeyModifiers::CONTROL | KeyModifiers::ALT =>
            {
                self.delete_small_word_left()
            }
            (KeyCode::Char('h'), KeyModifiers::CONTROL)
            | (KeyCode::Char('\u{0008}' | '\u{007f}'), _) => self.backspace(),
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => self.delete(),
            (KeyCode::Char('d'), modifiers)
                if modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SUPER) =>
            {
                self.delete_word_right()
            }
            (KeyCode::Char('j' | 'm'), KeyModifiers::CONTROL) => {
                self.insert_char('\n');
                true
            }
            (KeyCode::Backspace, KeyModifiers::SUPER) => self.kill_to_line_start(),
            (KeyCode::Backspace, _) if word_modifier => self.delete_small_word_left(),
            (KeyCode::Backspace, _) => self.backspace(),
            (KeyCode::Delete, modifiers)
                if modifiers.intersects(
                    KeyModifiers::ALT | KeyModifiers::CONTROL | KeyModifiers::SUPER,
                ) =>
            {
                self.delete_word_right()
            }
            (KeyCode::Delete, _) => self.delete(),
            (KeyCode::Left, KeyModifiers::SUPER) => self.move_home(),
            (KeyCode::Right, KeyModifiers::SUPER) => self.move_end(),
            (KeyCode::Left, _) if word_modifier => self.move_word_left(),
            (KeyCode::Right, _) if word_modifier => self.move_word_right(),
            (KeyCode::Left, _) => self.move_left(),
            (KeyCode::Right, _) => self.move_right(),
            (KeyCode::Up, _) => self.move_up(),
            (KeyCode::Down, _) => self.move_down(),
            (KeyCode::Home, _) => self.move_home(),
            (KeyCode::End, _) => self.move_end(),
            (KeyCode::Char(character), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) =>
            {
                self.insert_char(character);
                true
            }
            _ => false,
        };
        changed || selection_cleared
    }

    pub(crate) fn backspace(&mut self) -> bool {
        let Some(previous) = previous_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.replace_range(previous..self.cursor, "", MutationKind::Delete);
        true
    }

    pub(crate) fn delete(&mut self) -> bool {
        let Some(next) = next_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.replace_range(self.cursor..next, "", MutationKind::Delete);
        true
    }

    pub(crate) fn delete_word_left(&mut self) -> bool {
        let start = whitespace_word_start_left(&self.text, self.cursor);
        if start == self.cursor {
            return false;
        }
        self.kill_range(start..self.cursor);
        true
    }

    fn delete_small_word_left(&mut self) -> bool {
        let start = small_word_start_left(&self.text, self.cursor);
        if start == self.cursor {
            return false;
        }
        self.kill_range(start..self.cursor);
        true
    }

    fn delete_word_right(&mut self) -> bool {
        let end = small_word_end_right(&self.text, self.cursor);
        if end == self.cursor {
            return false;
        }
        self.kill_range(self.cursor..end);
        true
    }

    pub(crate) fn move_left(&mut self) -> bool {
        let Some(previous) = previous_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.cursor = previous;
        self.preferred_column = None;
        true
    }

    pub(crate) fn move_right(&mut self) -> bool {
        let Some(next) = next_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.cursor = next;
        self.preferred_column = None;
        true
    }

    pub(crate) fn move_word_left(&mut self) -> bool {
        let target = small_word_start_left(&self.text, self.cursor);
        if target == self.cursor {
            return false;
        }
        self.cursor = target;
        self.preferred_column = None;
        true
    }

    pub(crate) fn move_word_right(&mut self) -> bool {
        let target = small_word_end_right(&self.text, self.cursor);
        if target == self.cursor {
            return false;
        }
        self.cursor = target;
        self.preferred_column = None;
        true
    }

    fn move_readline_line_start(&mut self) -> bool {
        let start = line_start(&self.text, self.cursor);
        let target = if self.cursor == start && start > 0 {
            line_start(&self.text, start - 1)
        } else {
            start
        };
        if target == self.cursor {
            return false;
        }
        self.cursor = target;
        self.preferred_column = None;
        true
    }

    fn move_readline_line_end(&mut self) -> bool {
        let end = line_end(&self.text, self.cursor);
        let target = if self.cursor == end && end < self.text.len() {
            line_end(&self.text, end + 1)
        } else {
            end
        };
        if target == self.cursor {
            return false;
        }
        self.cursor = target;
        self.preferred_column = None;
        true
    }

    fn kill_to_line_start(&mut self) -> bool {
        let line_start = line_start(&self.text, self.cursor);
        let start = if self.cursor == line_start {
            previous_boundary(&self.text, line_start).unwrap_or(line_start)
        } else {
            line_start
        };
        if start == self.cursor {
            return false;
        }
        self.kill_range(start..self.cursor);
        true
    }

    fn kill_to_line_end(&mut self) -> bool {
        let line_end = line_end(&self.text, self.cursor);
        let end = if self.cursor == line_end {
            next_boundary(&self.text, line_end).unwrap_or(line_end)
        } else {
            line_end
        };
        if end == self.cursor {
            return false;
        }
        self.kill_range(self.cursor..end);
        true
    }

    fn yank(&mut self) -> bool {
        if self.kill_buffer.is_empty() {
            return false;
        }
        let killed = self.kill_buffer.clone();
        self.insert_text(&killed);
        true
    }

    pub(crate) fn move_home(&mut self) -> bool {
        let start = line_start(&self.text, self.cursor);
        if start == self.cursor {
            return false;
        }
        self.cursor = start;
        self.preferred_column = None;
        true
    }

    pub(crate) fn move_end(&mut self) -> bool {
        let end = line_end(&self.text, self.cursor);
        if end == self.cursor {
            return false;
        }
        self.cursor = end;
        self.preferred_column = None;
        true
    }

    pub(crate) fn move_up(&mut self) -> bool {
        let current_start = line_start(&self.text, self.cursor);
        if current_start == 0 {
            return false;
        }
        let column = self
            .preferred_column
            .unwrap_or_else(|| self.text[current_start..self.cursor].chars().count());
        let target_end = current_start - 1;
        let target_start = line_start(&self.text, target_end);
        self.cursor = byte_at_column(&self.text, target_start, target_end, column);
        self.preferred_column = Some(column);
        true
    }

    pub(crate) fn move_down(&mut self) -> bool {
        let current_start = line_start(&self.text, self.cursor);
        let current_end = line_end(&self.text, self.cursor);
        if current_end == self.text.len() {
            return false;
        }
        let column = self
            .preferred_column
            .unwrap_or_else(|| self.text[current_start..self.cursor].chars().count());
        let target_start = current_end + 1;
        let target_end = line_end(&self.text, target_start);
        self.cursor = byte_at_column(&self.text, target_start, target_end, column);
        self.preferred_column = Some(column);
        true
    }
}

#[cfg(test)]
#[path = "composer_tests.rs"]
mod tests;
