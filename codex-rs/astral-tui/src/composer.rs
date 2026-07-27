//! Editable prompt state for the Astral surface.
//!
//! The cursor is a UTF-8 byte offset, matching the convention used by the
//! Grok Build prompt widget. All mutations keep it on a character boundary.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ComposerState {
    text: String,
    cursor: usize,
    preferred_column: Option<usize>,
}

impl ComposerState {
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn replace(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.len();
        self.preferred_column = None;
    }

    pub(crate) fn take(&mut self) -> String {
        self.cursor = 0;
        self.preferred_column = None;
        std::mem::take(&mut self.text)
    }

    pub(crate) fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.preferred_column = None;
    }

    pub(crate) fn insert_char(&mut self, character: char) {
        self.text.insert(self.cursor, character);
        self.cursor += character.len_utf8();
        self.preferred_column = None;
    }

    pub(crate) fn insert_text(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        self.text.insert_str(self.cursor, &normalized);
        self.cursor += normalized.len();
        self.preferred_column = None;
    }

    pub(crate) fn edit_key(&mut self, key: KeyEvent) -> bool {
        let word_modifier = key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match key.code {
            KeyCode::Backspace if word_modifier => self.delete_word_left(),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Left if word_modifier => self.move_word_left(),
            KeyCode::Right if word_modifier => self.move_word_right(),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Up => self.move_up(),
            KeyCode::Down => self.move_down(),
            KeyCode::Home => self.move_home(),
            KeyCode::End => self.move_end(),
            KeyCode::Delete => self.delete(),
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) =>
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
        self.text.drain(previous..self.cursor);
        self.cursor = previous;
        self.preferred_column = None;
        true
    }

    pub(crate) fn delete(&mut self) -> bool {
        let Some(next) = next_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.text.drain(self.cursor..next);
        self.preferred_column = None;
        true
    }

    pub(crate) fn delete_word_left(&mut self) -> bool {
        let mut start = self.cursor;
        while let Some(previous) = previous_boundary(&self.text, start) {
            if !self.text[previous..start].chars().all(char::is_whitespace) {
                break;
            }
            start = previous;
        }
        while let Some(previous) = previous_boundary(&self.text, start) {
            if self.text[previous..start].chars().all(char::is_whitespace) {
                break;
            }
            start = previous;
        }
        if start == self.cursor {
            return false;
        }
        self.text.drain(start..self.cursor);
        self.cursor = start;
        self.preferred_column = None;
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
        let target = word_start_left(&self.text, self.cursor);
        if target == self.cursor {
            return false;
        }
        self.cursor = target;
        self.preferred_column = None;
        true
    }

    pub(crate) fn move_word_right(&mut self) -> bool {
        let mut target = self.cursor;
        while let Some(next) = next_boundary(&self.text, target) {
            if !self.text[target..next].chars().all(char::is_whitespace) {
                break;
            }
            target = next;
        }
        while let Some(next) = next_boundary(&self.text, target) {
            if self.text[target..next].chars().all(char::is_whitespace) {
                break;
            }
            target = next;
        }
        if target == self.cursor {
            return false;
        }
        self.cursor = target;
        self.preferred_column = None;
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

fn previous_boundary(text: &str, cursor: usize) -> Option<usize> {
    text[..cursor]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
}

fn word_start_left(text: &str, cursor: usize) -> usize {
    let mut start = cursor;
    while let Some(previous) = previous_boundary(text, start) {
        if !text[previous..start].chars().all(char::is_whitespace) {
            break;
        }
        start = previous;
    }
    while let Some(previous) = previous_boundary(text, start) {
        if text[previous..start].chars().all(char::is_whitespace) {
            break;
        }
        start = previous;
    }
    start
}

fn next_boundary(text: &str, cursor: usize) -> Option<usize> {
    text[cursor..]
        .chars()
        .next()
        .map(|character| cursor + character.len_utf8())
}

fn line_start(text: &str, cursor: usize) -> usize {
    text[..cursor].rfind('\n').map_or(0, |newline| newline + 1)
}

fn line_end(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .find('\n')
        .map_or(text.len(), |newline| cursor + newline)
}

fn byte_at_column(text: &str, start: usize, end: usize, column: usize) -> usize {
    text[start..end]
        .char_indices()
        .nth(column)
        .map_or(end, |(offset, _)| start + offset)
}

#[cfg(test)]
#[path = "composer_tests.rs"]
mod tests;
