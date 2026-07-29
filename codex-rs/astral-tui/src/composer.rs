//! Editable prompt state for the Astral surface.
//!
//! The cursor is a UTF-8 byte offset, matching the convention used by the
//! Grok Build prompt widget. All mutations keep it on a character boundary.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use crate::mention::MentionBinding;
use crate::mention::MentionTarget;
use crate::mention::PromptSubmission;

mod edit;

use edit::byte_at_column;
use edit::line_end;
use edit::line_start;
use edit::next_boundary;
use edit::previous_boundary;
use edit::small_word_end_right;
use edit::small_word_start_left;
use edit::whitespace_word_start_left;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ComposerState {
    text: String,
    cursor: usize,
    preferred_column: Option<usize>,
    kill_buffer: String,
    mention_bindings: Vec<MentionBinding>,
}

impl ComposerState {
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn set_cursor(&mut self, cursor: usize) -> bool {
        let mut cursor = cursor.min(self.text.len());
        while !self.text.is_char_boundary(cursor) {
            cursor = cursor.saturating_sub(1);
        }
        if cursor == self.cursor {
            return false;
        }
        self.cursor = cursor;
        self.preferred_column = None;
        true
    }

    pub(crate) fn replace(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.len();
        self.preferred_column = None;
        self.mention_bindings.clear();
    }

    pub(crate) fn take(&mut self) -> String {
        self.cursor = 0;
        self.preferred_column = None;
        self.mention_bindings.clear();
        std::mem::take(&mut self.text)
    }

    pub(crate) fn take_submission(&mut self) -> PromptSubmission {
        let mentions = self
            .mention_bindings
            .drain(..)
            .filter(|binding| binding_matches_text(&self.text, binding))
            .collect();
        self.cursor = 0;
        self.preferred_column = None;
        PromptSubmission {
            text: std::mem::take(&mut self.text),
            mentions,
        }
    }

    pub(crate) fn restore_submission(&mut self, submission: PromptSubmission) {
        self.text = submission.text;
        self.mention_bindings = submission.mentions;
        self.cursor = self.text.len();
        self.preferred_column = None;
    }

    pub(crate) fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.preferred_column = None;
        self.mention_bindings.clear();
    }

    pub(crate) fn insert_char(&mut self, character: char) {
        self.replace_range(self.cursor..self.cursor, &character.to_string());
    }

    pub(crate) fn insert_text(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        self.replace_range(self.cursor..self.cursor, &normalized);
    }

    pub(crate) fn insert_mention(
        &mut self,
        range: std::ops::Range<usize>,
        insert_text: String,
        target: MentionTarget,
    ) {
        let start = range.start;
        let inserted = format!("{insert_text} ");
        self.replace_range(range, &inserted);
        self.mention_bindings.push(MentionBinding {
            range: start..start + insert_text.len(),
            insert_text,
            target,
        });
    }

    pub(crate) fn edit_key(&mut self, key: KeyEvent) -> bool {
        let word_modifier = key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match (key.code, key.modifiers) {
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
        }
    }

    pub(crate) fn backspace(&mut self) -> bool {
        let Some(previous) = previous_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.replace_range(previous..self.cursor, "");
        true
    }

    pub(crate) fn delete(&mut self) -> bool {
        let Some(next) = next_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.replace_range(self.cursor..next, "");
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

    fn kill_range(&mut self, range: std::ops::Range<usize>) {
        self.kill_buffer = self.text[range.clone()].to_string();
        self.replace_range(range, "");
    }

    fn replace_range(&mut self, range: std::ops::Range<usize>, replacement: &str) {
        let removed_len = range.end.saturating_sub(range.start);
        let inserted_len = replacement.len();
        self.mention_bindings.retain_mut(|binding| {
            if range.is_empty() {
                return adjust_binding_for_insertion(binding, range.start, replacement);
            }
            if range.end <= binding.range.start {
                binding.range.start = shifted_index(binding.range.start, removed_len, inserted_len);
                binding.range.end = shifted_index(binding.range.end, removed_len, inserted_len);
                return true;
            }
            range.start >= binding.range.end
        });
        self.text.replace_range(range.clone(), replacement);
        self.cursor = range.start.saturating_add(inserted_len);
        self.preferred_column = None;
    }
}

fn adjust_binding_for_insertion(
    binding: &mut MentionBinding,
    position: usize,
    inserted: &str,
) -> bool {
    if position < binding.range.start {
        binding.range.start = binding.range.start.saturating_add(inserted.len());
        binding.range.end = binding.range.end.saturating_add(inserted.len());
        return true;
    }
    if position == binding.range.start {
        if !inserted
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
        {
            return false;
        }
        binding.range.start = binding.range.start.saturating_add(inserted.len());
        binding.range.end = binding.range.end.saturating_add(inserted.len());
        return true;
    }
    if position < binding.range.end {
        return false;
    }
    position != binding.range.end || inserted.chars().next().is_some_and(char::is_whitespace)
}

fn shifted_index(index: usize, removed_len: usize, inserted_len: usize) -> usize {
    index
        .saturating_sub(removed_len)
        .saturating_add(inserted_len)
}

fn binding_matches_text(text: &str, binding: &MentionBinding) -> bool {
    text.get(binding.range.clone()) == Some(binding.insert_text.as_str())
        && (binding.range.start == 0
            || text[..binding.range.start]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace))
        && (binding.range.end == text.len()
            || text[binding.range.end..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace))
}

#[cfg(test)]
#[path = "composer_tests.rs"]
mod tests;
