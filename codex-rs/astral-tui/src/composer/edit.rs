//! Cursor and word-boundary helpers for prompt editing.
//!
//! The word classes and readline boundary behavior follow Grok Build's
//! `xai-ratatui-textarea` at commit 47348d13ec4508dcfe440e34c6d511bb02998fb2
//! (Apache-2.0), adapted to Astral's lightweight composer state.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordClass {
    Whitespace,
    Word,
    Punctuation,
}

pub(super) fn previous_boundary(text: &str, cursor: usize) -> Option<usize> {
    text[..cursor]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
}

pub(super) fn next_boundary(text: &str, cursor: usize) -> Option<usize> {
    text[cursor..]
        .chars()
        .next()
        .map(|character| cursor + character.len_utf8())
}

pub(super) fn whitespace_word_start_left(text: &str, cursor: usize) -> usize {
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

pub(super) fn small_word_start_left(text: &str, cursor: usize) -> usize {
    let mut start = cursor;
    while let Some(previous) = previous_boundary(text, start) {
        if word_class(&text[previous..start]) != Some(WordClass::Whitespace) {
            break;
        }
        start = previous;
    }
    let Some(previous) = previous_boundary(text, start) else {
        return start;
    };
    let target_class = word_class(&text[previous..start]);
    while let Some(previous) = previous_boundary(text, start) {
        if word_class(&text[previous..start]) != target_class {
            break;
        }
        start = previous;
    }
    start
}

pub(super) fn small_word_end_right(text: &str, cursor: usize) -> usize {
    let mut end = cursor;
    while let Some(next) = next_boundary(text, end) {
        if word_class(&text[end..next]) != Some(WordClass::Whitespace) {
            break;
        }
        end = next;
    }
    let Some(next) = next_boundary(text, end) else {
        return end;
    };
    let target_class = word_class(&text[end..next]);
    while let Some(next) = next_boundary(text, end) {
        if word_class(&text[end..next]) != target_class {
            break;
        }
        end = next;
    }
    end
}

pub(super) fn line_start(text: &str, cursor: usize) -> usize {
    text[..cursor].rfind('\n').map_or(0, |newline| newline + 1)
}

pub(super) fn line_end(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .find('\n')
        .map_or(text.len(), |newline| cursor + newline)
}

pub(super) fn byte_at_column(text: &str, start: usize, end: usize, column: usize) -> usize {
    text[start..end]
        .char_indices()
        .nth(column)
        .map_or(end, |(offset, _)| start + offset)
}

fn word_class(text: &str) -> Option<WordClass> {
    let character = text.chars().next()?;
    if character.is_whitespace() {
        Some(WordClass::Whitespace)
    } else if character.is_alphanumeric() || character == '_' {
        Some(WordClass::Word)
    } else {
        Some(WordClass::Punctuation)
    }
}
