//! Atomic editing and paste-chip behavior for structured prompt elements.
//!
//! The visible buffer keeps a compact placeholder while the element owns the
//! full paste payload. Navigation, selection, and deletion treat that range as
//! one unit; submission expands the payload before it reaches app-server.
//! Thresholds and interactions follow Grok Build's prompt widget at commit
//! 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).

use std::ops::Range;

use super::ComposerElement;
use super::ComposerState;
use super::history::MutationKind;
use crate::mention::PromptSubmission;

const PASTE_CHIP_DISPLAY_BYTES: usize = 10_000;
const PASTE_CHIP_MIN_LINES: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileReferenceAtCursor {
    pub(crate) range: Range<usize>,
    pub(crate) path: String,
    pub(crate) line_range: Option<Range<usize>>,
}

impl ComposerState {
    pub(crate) fn elements(&self) -> &[ComposerElement] {
        &self.elements
    }

    pub(crate) fn submission(&self) -> PromptSubmission {
        PromptSubmission {
            text: self.text.clone(),
            elements: self
                .elements
                .iter()
                .filter(|element| element.matches_text(&self.text))
                .cloned()
                .collect(),
        }
    }

    pub(crate) fn insert_paste(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        if normalized.is_empty() {
            return;
        }
        if self.selection_range().is_none()
            && let Some(index) = self.paste_element_near_cursor(&normalized)
        {
            self.expand_element(index);
            return;
        }

        let line_count = normalized.lines().count();
        let by_bytes = normalized.len() > PASTE_CHIP_DISPLAY_BYTES;
        if line_count < PASTE_CHIP_MIN_LINES && !by_bytes {
            self.insert_text(&normalized);
            return;
        }

        let placeholder = if by_bytes {
            paste_chip_bytes(normalized.len())
        } else {
            paste_chip_lines(line_count)
        };
        let range = self.selection_range().unwrap_or(self.cursor..self.cursor);
        let range = self.expand_range_to_element_boundaries(range);
        let start = range.start;
        self.replace_range(range, &placeholder, MutationKind::Replace);
        self.elements.push(ComposerElement::paste(
            start..start + placeholder.len(),
            placeholder,
            normalized,
        ));
        self.elements.sort_by_key(|element| element.range.start);
    }

    pub(crate) fn insert_file_reference(&mut self, range: Range<usize>, path: String) {
        let start = range.start;
        let insert_text = format!("@{path}");
        let replacement = format!("{insert_text} ");
        self.replace_range(range, &replacement, MutationKind::Replace);
        self.elements.push(ComposerElement::file_reference(
            start..start + insert_text.len(),
            insert_text,
        ));
        self.elements.sort_by_key(|element| element.range.start);
    }

    pub(crate) fn replace_file_reference_path(&mut self, token_range: Range<usize>, path: &str) {
        let path_start = token_range.start.saturating_add(1).min(token_range.end);
        self.replace_range(path_start..token_range.end, path, MutationKind::Replace);
    }

    pub(crate) fn replace_file_reference(&mut self, range: Range<usize>, path: &str) {
        let start = range.start;
        let end = if self.text.as_bytes().get(range.end) == Some(&b' ') {
            range.end.saturating_add(1)
        } else {
            range.end
        };
        let insert_text = format!("@{path}");
        self.replace_range(
            range.start..end,
            &format!("{insert_text} "),
            MutationKind::Replace,
        );
        self.elements.push(ComposerElement::file_reference(
            start..start + insert_text.len(),
            insert_text,
        ));
        self.elements.sort_by_key(|element| element.range.start);
    }

    pub(crate) fn file_reference_at_cursor(&self) -> Option<FileReferenceAtCursor> {
        self.file_reference_near_cursor(false)
    }

    pub(crate) fn file_reference_at_boundary(&self) -> Option<FileReferenceAtCursor> {
        self.file_reference_near_cursor(true)
    }

    pub(crate) fn expand_paste_at_cursor(&mut self) -> bool {
        let Some(index) = self.element_index_at(self.cursor) else {
            return false;
        };
        if !self.elements[index].is_paste() {
            return false;
        }
        self.expand_element(index);
        true
    }

    pub(super) fn expand_paste_at_position(&mut self, position: usize) -> bool {
        let Some(index) = self.element_index_at(position) else {
            return false;
        };
        if !self.elements[index].is_paste() {
            return false;
        }
        self.expand_element(index);
        true
    }

    fn file_reference_near_cursor(&self, boundary_only: bool) -> Option<FileReferenceAtCursor> {
        let element = self.elements.iter().find(|element| {
            element.is_file_reference()
                && if boundary_only {
                    self.cursor == element.range.start || self.cursor == element.range.end
                } else {
                    self.cursor >= element.range.start
                        && self.cursor <= element.range.end.saturating_add(1)
                }
        })?;
        let text = self.text.get(element.range.clone())?;
        let (path, line_range) = parse_file_reference(text);
        Some(FileReferenceAtCursor {
            range: element.range.clone(),
            path,
            line_range,
        })
    }

    pub(super) fn element_start_at(&self, position: usize) -> Option<usize> {
        self.element_index_at(position)
            .map(|index| self.elements[index].range.start)
    }

    pub(super) fn atomic_left_target(&self) -> Option<usize> {
        if let Some(element) = self
            .elements
            .iter()
            .find(|element| self.cursor > element.range.start && self.cursor <= element.range.end)
        {
            return Some(element.range.start);
        }
        super::previous_boundary(&self.text, self.cursor)
    }

    pub(super) fn atomic_right_target(&self) -> Option<usize> {
        if let Some(element) = self
            .elements
            .iter()
            .find(|element| self.cursor >= element.range.start && self.cursor < element.range.end)
        {
            return Some(element.range.end);
        }
        super::next_boundary(&self.text, self.cursor)
    }

    pub(super) fn snap_position_to_element_boundary(&self, position: usize) -> usize {
        let position = position.min(self.text.len());
        let Some(element) = self
            .elements
            .iter()
            .find(|element| position > element.range.start && position < element.range.end)
        else {
            return position;
        };
        let from_start = position.saturating_sub(element.range.start);
        let from_end = element.range.end.saturating_sub(position);
        if from_start <= from_end {
            element.range.start
        } else {
            element.range.end
        }
    }

    pub(super) fn expand_range_to_element_boundaries(
        &self,
        mut range: Range<usize>,
    ) -> Range<usize> {
        loop {
            let mut changed = false;
            for element in &self.elements {
                if element.range.start < range.end && element.range.end > range.start {
                    let start = range.start.min(element.range.start);
                    let end = range.end.max(element.range.end);
                    if start != range.start || end != range.end {
                        range = start..end;
                        changed = true;
                    }
                }
            }
            if !changed {
                return range;
            }
        }
    }

    pub(super) fn expanded_text_for_range(&self, range: Range<usize>) -> String {
        let range = self.expand_range_to_element_boundaries(range);
        let mut output = String::new();
        let mut cursor = range.start;
        for element in self.elements.iter().filter(|element| {
            element.range.start >= range.start
                && element.range.end <= range.end
                && element.matches_text(&self.text)
        }) {
            output.push_str(&self.text[cursor..element.range.start]);
            output.push_str(element.submission_text());
            cursor = element.range.end;
        }
        output.push_str(&self.text[cursor..range.end]);
        output
    }

    fn element_index_at(&self, position: usize) -> Option<usize> {
        self.elements.iter().position(|element| {
            position >= element.range.start
                && position < element.range.end
                && element.matches_text(&self.text)
        })
    }

    fn paste_element_near_cursor(&self, content: &str) -> Option<usize> {
        if let Some(index) = self.element_index_at(self.cursor) {
            return self.elements[index]
                .paste_content()
                .is_some_and(|paste| paste == content)
                .then_some(index);
        }
        self.elements.iter().position(|element| {
            element.range.end == self.cursor
                && element.matches_text(&self.text)
                && element
                    .paste_content()
                    .is_some_and(|paste| paste == content)
        })
    }

    fn expand_element(&mut self, index: usize) {
        let element = self.elements[index].clone();
        let replacement = element.submission_text().to_string();
        self.replace_range(element.range, &replacement, MutationKind::Replace);
    }
}

fn parse_file_reference(text: &str) -> (String, Option<Range<usize>>) {
    let text = text.strip_prefix('@').unwrap_or(text);
    let Some((path, suffix)) = text.rsplit_once(':') else {
        return (text.to_string(), None);
    };
    let parsed = if let Some((start, end)) = suffix.split_once('-') {
        let start = start.parse::<usize>().ok();
        let end = end.parse::<usize>().ok();
        start.zip(end).and_then(|(start, end)| {
            (start > 0 && end >= start).then(|| start..end.saturating_add(1))
        })
    } else {
        suffix
            .parse::<usize>()
            .ok()
            .filter(|line| *line > 0)
            .map(|line| line..line.saturating_add(1))
    };
    parsed.map_or_else(
        || (text.to_string(), None),
        |range| (path.to_string(), Some(range)),
    )
}

fn paste_chip_lines(line_count: usize) -> String {
    let suffix = if line_count == 1 { "" } else { "s" };
    format!("[Pasted: {line_count} line{suffix}]")
}

fn paste_chip_bytes(byte_len: usize) -> String {
    let size = if byte_len >= 1_000_000 {
        format!("{:.1} MB", byte_len as f64 / 1_000_000.0)
    } else if byte_len >= 1_000 {
        format!("{} KB", byte_len / 1_000)
    } else {
        format!("{byte_len} bytes")
    };
    format!("[Pasted: {size}]")
}
