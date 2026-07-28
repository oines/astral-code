// Derived from Grok Build's TextMatcher and ListPane search/filter lifecycle at
// commit 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// This viewer-local version matches Astral's already-rendered lines and does
// not introduce another transcript projection or touch provider semantics.

use std::ops::Range;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use regex::Regex;
use regex::RegexBuilder;

use crate::composer::ComposerState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ViewerMatchMode {
    Search,
    Filter,
}

impl ViewerMatchMode {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Filter => "filter",
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct ViewerMatcher {
    editor: ComposerState,
    mode: Option<ViewerMatchMode>,
    input_active: bool,
    matcher: Option<Regex>,
    matcher_error: bool,
    match_lines: Vec<usize>,
    show_highlights: bool,
}

impl ViewerMatcher {
    pub(super) fn open(&mut self, mode: ViewerMatchMode) {
        let reopen_same = self.matcher.is_some() && self.mode == Some(mode);
        if !reopen_same {
            self.clear();
        }
        self.mode = Some(mode);
        self.input_active = true;
        self.show_highlights = true;
    }

    pub(super) fn input_active(&self) -> bool {
        self.input_active
    }

    pub(super) fn is_visible(&self) -> bool {
        self.input_active || self.matcher.is_some()
    }

    pub(super) fn mode(&self) -> ViewerMatchMode {
        self.mode.unwrap_or(ViewerMatchMode::Search)
    }

    pub(super) fn filter_active(&self) -> bool {
        self.mode == Some(ViewerMatchMode::Filter) && self.matcher.is_some()
    }

    pub(super) fn query(&self) -> &str {
        self.editor.text()
    }

    pub(super) fn cursor(&self) -> usize {
        self.editor.cursor()
    }

    pub(super) fn is_error(&self) -> bool {
        self.matcher_error
    }

    pub(super) fn match_count(&self) -> usize {
        self.match_lines.len()
    }

    pub(super) fn match_lines(&self) -> &[usize] {
        &self.match_lines
    }

    pub(super) fn rebuild(&mut self, lines: &[String]) {
        self.match_lines.clear();
        let Some(matcher) = self.matcher.as_ref() else {
            return;
        };
        self.match_lines.extend(
            lines
                .iter()
                .enumerate()
                .filter_map(|(index, line)| matcher.is_match(line).then_some(index)),
        );
    }

    pub(super) fn handle_key(
        &mut self,
        key: KeyEvent,
        lines: &[String],
        selected: Option<usize>,
    ) -> Option<usize> {
        if !self.input_active {
            return None;
        }
        if key.code == KeyCode::Enter {
            if self.editor.text().is_empty() {
                self.clear();
            } else {
                self.input_active = false;
                self.show_highlights = self.mode != Some(ViewerMatchMode::Filter);
            }
            return None;
        }
        if key.code == KeyCode::Esc {
            self.clear();
            return None;
        }
        if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
            if self.editor.text().is_empty() {
                self.clear();
            } else {
                self.editor.clear();
                self.compile_and_rebuild(lines);
            }
            return None;
        }
        if self.editor.text().is_empty()
            && (key.code == KeyCode::Backspace
                || key.code == KeyCode::Char('w') && key.modifiers == KeyModifiers::CONTROL)
        {
            self.clear();
            return None;
        }
        if !self.editor.edit_key(key) {
            return None;
        }
        self.compile_and_rebuild(lines);
        if self.mode == Some(ViewerMatchMode::Search) {
            self.nearest_match(selected.unwrap_or(0))
        } else {
            None
        }
    }

    pub(super) fn paste(
        &mut self,
        text: &str,
        lines: &[String],
        selected: Option<usize>,
    ) -> Option<usize> {
        if !self.input_active {
            return None;
        }
        let single_line = text
            .chars()
            .filter(|character| *character != '\n' && *character != '\r')
            .collect::<String>();
        self.editor.insert_text(&single_line);
        self.compile_and_rebuild(lines);
        if self.mode == Some(ViewerMatchMode::Search) {
            self.nearest_match(selected.unwrap_or(0))
        } else {
            None
        }
    }

    pub(super) fn next_match(&self, current: usize) -> Option<usize> {
        if self.match_lines.is_empty() {
            return None;
        }
        let position = self.match_lines.partition_point(|&line| line <= current);
        self.match_lines
            .get(position)
            .or_else(|| self.match_lines.first())
            .copied()
    }

    pub(super) fn previous_match(&self, current: usize) -> Option<usize> {
        if self.match_lines.is_empty() {
            return None;
        }
        let position = self.match_lines.partition_point(|&line| line < current);
        position
            .checked_sub(1)
            .and_then(|index| self.match_lines.get(index))
            .or_else(|| self.match_lines.last())
            .copied()
    }

    pub(super) fn match_ranges(&self, text: &str) -> Vec<Range<usize>> {
        if !self.show_highlights {
            return Vec::new();
        }
        self.matcher
            .as_ref()
            .map(|matcher| {
                matcher
                    .find_iter(text)
                    .filter_map(|matched| {
                        (matched.start() != matched.end()).then_some(matched.start()..matched.end())
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn clear(&mut self) {
        self.editor.clear();
        self.mode = None;
        self.input_active = false;
        self.matcher = None;
        self.matcher_error = false;
        self.match_lines.clear();
        self.show_highlights = true;
    }

    fn compile_and_rebuild(&mut self, lines: &[String]) {
        self.show_highlights = true;
        let query = self.editor.text();
        if query.is_empty() {
            self.matcher = None;
            self.matcher_error = false;
            self.match_lines.clear();
            return;
        }
        let smart_case = !query.chars().any(char::is_uppercase);
        match RegexBuilder::new(query)
            .case_insensitive(smart_case)
            .build()
        {
            Ok(matcher) => {
                self.matcher = Some(matcher);
                self.matcher_error = false;
            }
            Err(_) => {
                self.matcher = Regex::new(r"\z.").ok();
                self.matcher_error = true;
            }
        }
        self.rebuild(lines);
    }

    fn nearest_match(&self, current: usize) -> Option<usize> {
        if self.match_lines.binary_search(&current).is_ok() {
            return None;
        }
        let position = self.match_lines.partition_point(|&line| line < current);
        self.match_lines
            .get(position)
            .or_else(|| self.match_lines.first())
            .copied()
    }
}
