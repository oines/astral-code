use std::ops::Range;

use crate::mention::MentionTarget;
use crate::slash::fuzzy_match;

const MAX_MATCHES: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MentionKind {
    Plugin,
    Skill,
}

impl MentionKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Plugin => "Plugin",
            Self::Skill => "Skill",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MentionCandidate {
    pub(crate) kind: MentionKind,
    pub(crate) display: String,
    pub(crate) description: String,
    pub(crate) insert_text: String,
    pub(crate) search_terms: Vec<String>,
    pub(crate) target: MentionTarget,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MentionCatalog {
    pub(crate) candidates: Vec<MentionCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MentionSuggestion {
    pub(crate) kind: MentionKind,
    pub(crate) display: String,
    pub(crate) description: String,
    pub(crate) insert_text: String,
    pub(crate) indices: Vec<usize>,
    pub(crate) target: MentionTarget,
}

impl MentionSuggestion {
    fn key(&self) -> &str {
        self.target.key()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MentionSnapshot {
    pub(crate) active: bool,
    pub(crate) open: bool,
    pub(crate) query: String,
    pub(crate) matches: Vec<MentionSuggestion>,
    pub(crate) selected: usize,
    token_range: Option<Range<usize>>,
}

impl MentionSnapshot {
    pub(crate) fn selection(&self) -> Option<&MentionSuggestion> {
        self.matches
            .get(self.selected.min(self.matches.len().saturating_sub(1)))
    }
}

#[derive(Debug, Default)]
pub(crate) struct MentionController {
    catalog: MentionCatalog,
    snapshot: MentionSnapshot,
    dismissed_token: Option<String>,
}

impl MentionController {
    pub(crate) fn snapshot(&self) -> &MentionSnapshot {
        &self.snapshot
    }

    pub(crate) fn set_catalog(&mut self, catalog: MentionCatalog) {
        self.catalog = catalog;
    }

    pub(crate) fn refresh(&mut self, text: &str, cursor: usize) {
        let previous = self
            .snapshot
            .selection()
            .map(|selection| selection.key().to_string());
        let Some(token) = active_token(text, cursor) else {
            self.snapshot = MentionSnapshot::default();
            self.dismissed_token = None;
            return;
        };
        let token_text = text[token.range.clone()].to_string();
        if self.dismissed_token.as_deref() != Some(token_text.as_str()) {
            self.dismissed_token = None;
        }
        let mut matches = self
            .catalog
            .candidates
            .iter()
            .filter_map(|candidate| suggestion(candidate, token.query))
            .collect::<Vec<_>>();
        matches.sort_by(|a, b| {
            a.0.kind
                .cmp(&b.0.kind)
                .then_with(|| b.1.cmp(&a.1))
                .then_with(|| a.0.display.cmp(&b.0.display))
        });
        let matches = matches
            .into_iter()
            .take(MAX_MATCHES)
            .map(|(suggestion, _)| suggestion)
            .collect::<Vec<_>>();
        let selected = previous
            .and_then(|key| matches.iter().position(|row| row.key() == key))
            .unwrap_or_default();
        self.snapshot = MentionSnapshot {
            active: true,
            open: !matches.is_empty() && self.dismissed_token.is_none(),
            query: token.query.to_string(),
            matches,
            selected,
            token_range: Some(token.range),
        };
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let len = self.snapshot.matches.len();
        if len > 0 {
            self.snapshot.selected =
                (self.snapshot.selected as isize + delta).rem_euclid(len as isize) as usize;
        }
    }

    pub(crate) fn dismiss(&mut self, text: &str) {
        if let Some(range) = self.snapshot.token_range.clone() {
            self.dismissed_token = Some(text[range].to_string());
        }
        self.snapshot.open = false;
    }

    pub(crate) fn selection(&self) -> Option<(Range<usize>, MentionSuggestion)> {
        Some((
            self.snapshot.token_range.clone()?,
            self.snapshot.selection()?.clone(),
        ))
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
    let before_cursor = &text[..cursor];
    let start = before_cursor
        .char_indices()
        .rfind(|(_, character)| character.is_whitespace())
        .map_or(0, |(index, character)| index + character.len_utf8());
    let token = &text[start..];
    let end = token.find(char::is_whitespace).unwrap_or(token.len()) + start;
    let query = text[start..cursor].strip_prefix('$')?;
    Some(ActiveToken {
        range: start..end,
        query,
    })
}

fn suggestion(candidate: &MentionCandidate, query: &str) -> Option<(MentionSuggestion, u32)> {
    let display_match = fuzzy_match(&candidate.display, query);
    let fallback_score = candidate
        .search_terms
        .iter()
        .filter_map(|term| fuzzy_match(term, query).map(|(score, _)| score))
        .max();
    let (score, indices) =
        display_match.or_else(|| fallback_score.map(|score| (score, Vec::new())))?;
    Some((
        MentionSuggestion {
            kind: candidate.kind,
            display: candidate.display.clone(),
            description: candidate.description.clone(),
            insert_text: candidate.insert_text.clone(),
            indices,
            target: candidate.target.clone(),
        },
        score,
    ))
}

#[cfg(test)]
#[path = "../mention_tests.rs"]
mod tests;
