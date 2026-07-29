use std::ops::Range;

use codex_app_server_protocol::FuzzyFileSearchMatchType;
use codex_app_server_protocol::FuzzyFileSearchResult;

use crate::composer::ComposerElement;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileSearchRequest {
    pub(crate) generation: u64,
    pub(crate) query: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct FileSearchSnapshot {
    pub(crate) open: bool,
    pub(crate) query: String,
    pub(crate) matches: Vec<FuzzyFileSearchResult>,
    pub(crate) selected: usize,
    pub(crate) waiting: bool,
    pub(crate) error: Option<String>,
    token_range: Option<Range<usize>>,
    generation: u64,
}

impl FileSearchSnapshot {
    pub(crate) fn is_directory_mode(&self) -> bool {
        self.query.ends_with('/')
    }
}

#[derive(Debug, Default)]
pub(crate) struct FileSearchController {
    snapshot: FileSearchSnapshot,
    dismissed_token: Option<String>,
    pending_request: Option<FileSearchRequest>,
    next_generation: u64,
}

impl FileSearchController {
    pub(crate) fn snapshot(&self) -> &FileSearchSnapshot {
        &self.snapshot
    }

    pub(crate) fn refresh(&mut self, text: &str, cursor: usize, elements: &[ComposerElement]) {
        let Some(token) = active_token(text, cursor) else {
            self.reset();
            return;
        };
        if elements
            .iter()
            .any(|element| element.range == token.range && element.matches_text(text))
        {
            self.reset();
            return;
        }
        let token_text = text[token.range.clone()].to_string();
        if self.dismissed_token.as_deref() != Some(token_text.as_str()) {
            self.dismissed_token = None;
        }

        let query_changed =
            self.snapshot.token_range.is_none() || self.snapshot.query != token.query;
        if query_changed {
            self.next_generation = self.next_generation.wrapping_add(1);
            let generation = self.next_generation;
            let query = token.query.to_string();
            self.snapshot = FileSearchSnapshot {
                open: self.dismissed_token.is_none(),
                query: query.clone(),
                matches: Vec::new(),
                selected: 0,
                waiting: !query.is_empty(),
                error: None,
                token_range: Some(token.range),
                generation,
            };
            self.pending_request =
                (!query.is_empty()).then_some(FileSearchRequest { generation, query });
        } else {
            self.snapshot.open = self.dismissed_token.is_none();
            self.snapshot.token_range = Some(token.range);
        }
    }

    pub(crate) fn take_request(&mut self) -> Option<FileSearchRequest> {
        self.pending_request.take()
    }

    pub(crate) fn apply_results(
        &mut self,
        generation: u64,
        query: &str,
        mut matches: Vec<FuzzyFileSearchResult>,
    ) -> bool {
        if !self.accepts(generation, query) {
            return false;
        }
        if self.snapshot.is_directory_mode() {
            matches.retain(|result| result.match_type == FuzzyFileSearchMatchType::Directory);
        }
        self.snapshot.matches = matches;
        self.snapshot.selected = self
            .snapshot
            .selected
            .min(self.snapshot.matches.len().saturating_sub(1));
        self.snapshot.waiting = false;
        self.snapshot.error = None;
        true
    }

    pub(crate) fn apply_error(&mut self, generation: u64, query: &str, error: String) -> bool {
        if !self.accepts(generation, query) {
            return false;
        }
        self.snapshot.matches.clear();
        self.snapshot.selected = 0;
        self.snapshot.waiting = false;
        self.snapshot.error = Some(error);
        true
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let len = self.snapshot.matches.len();
        if len == 0 {
            return;
        }
        let last = len.saturating_sub(1) as isize;
        self.snapshot.selected = (self.snapshot.selected as isize + delta).clamp(0, last) as usize;
    }

    pub(crate) fn select(&mut self, index: usize) {
        if !self.snapshot.matches.is_empty() {
            self.snapshot.selected = index.min(self.snapshot.matches.len().saturating_sub(1));
        }
    }

    pub(crate) fn dismiss(&mut self, text: &str) {
        if let Some(range) = self.snapshot.token_range.clone() {
            self.dismissed_token = Some(text[range].to_string());
        }
        self.snapshot.open = false;
    }

    pub(crate) fn selection(&self) -> Option<(Range<usize>, FuzzyFileSearchResult)> {
        Some((
            self.snapshot.token_range.clone()?,
            self.snapshot.matches.get(self.snapshot.selected)?.clone(),
        ))
    }

    fn accepts(&self, generation: u64, query: &str) -> bool {
        self.snapshot.token_range.is_some()
            && self.snapshot.generation == generation
            && self.snapshot.query == query
    }

    fn reset(&mut self) {
        self.snapshot = FileSearchSnapshot::default();
        self.dismissed_token = None;
        self.pending_request = None;
    }
}

struct ActiveToken<'a> {
    range: Range<usize>,
    query: &'a str,
}

fn active_token(text: &str, cursor: usize) -> Option<ActiveToken<'_>> {
    if cursor > text.len() || !text.is_char_boundary(cursor) {
        return None;
    }
    let at = text[..cursor].rfind('@')?;
    if text[..at]
        .chars()
        .next_back()
        .is_some_and(|character| character.is_alphanumeric() || character == '_')
    {
        return None;
    }
    let end = text[at + 1..]
        .char_indices()
        .find_map(|(offset, character)| {
            (character.is_whitespace() || matches!(character, ',' | ';')).then_some(at + 1 + offset)
        })
        .unwrap_or(text.len());
    if cursor > end {
        return None;
    }
    Some(ActiveToken {
        range: at..end,
        query: &text[at + 1..cursor],
    })
}
