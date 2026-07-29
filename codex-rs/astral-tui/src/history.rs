//! Prompt history shared by arrow-key browsing and `/history` search.
//!
//! The interaction follows Grok Build's history panel while keeping Astral's
//! app-server transcript authoritative. Resumed user messages seed the local
//! history and successful submissions extend it; no separate persistence or
//! protocol surface is introduced here.

use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::UserInput;

use crate::PromptSubmission;
use crate::slash::fuzzy_match;

const MAX_HISTORY_ENTRIES: usize = 1_000;
const MAX_MATCHES: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryMatch {
    pub(crate) submission: PromptSubmission,
    pub(crate) display: String,
    pub(crate) indices: Vec<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HistorySnapshot {
    pub(crate) open: bool,
    pub(crate) browse: bool,
    pub(crate) saved_submission: PromptSubmission,
    pub(crate) query: String,
    pub(crate) matches: Vec<HistoryMatch>,
    pub(crate) selected: usize,
}

impl HistorySnapshot {
    pub(crate) fn selection(&self) -> Option<&HistoryMatch> {
        self.matches.get(self.selected)
    }
}

#[derive(Debug, Default)]
pub(crate) struct PromptHistory {
    /// Most recent first. Empty-query results reverse this so the newest
    /// prompt sits at the bottom of the panel, nearest the composer.
    entries: Vec<PromptSubmission>,
    snapshot: HistorySnapshot,
}

impl PromptHistory {
    pub(crate) fn from_turns(turns: &[Turn]) -> Self {
        let mut history = Self::default();
        for turn in turns {
            for item in &turn.items {
                let ThreadItem::UserMessage { content, .. } = item else {
                    continue;
                };
                let text = content
                    .iter()
                    .filter_map(|input| match input {
                        UserInput::Text { text, .. } => Some(text.as_str()),
                        UserInput::Image { .. }
                        | UserInput::LocalImage { .. }
                        | UserInput::Skill { .. }
                        | UserInput::Mention { .. } => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                history.record(&PromptSubmission::text_only(text));
            }
        }
        history
    }

    pub(crate) fn snapshot(&self) -> &HistorySnapshot {
        &self.snapshot
    }

    pub(crate) fn record(&mut self, submission: &PromptSubmission) {
        if submission.text().trim().is_empty()
            || self
                .entries
                .first()
                .is_some_and(|entry| entry == submission)
        {
            return;
        }
        self.entries.insert(0, submission.clone());
        self.entries.truncate(MAX_HISTORY_ENTRIES);
        self.deactivate();
    }

    pub(crate) fn activate_browse(
        &mut self,
        saved_submission: PromptSubmission,
    ) -> Option<PromptSubmission> {
        if self.entries.is_empty() {
            return None;
        }
        self.activate(saved_submission, /*browse*/ true);
        self.selected_submission().cloned()
    }

    pub(crate) fn activate_search(&mut self, saved_submission: PromptSubmission) {
        self.activate(saved_submission, /*browse*/ false);
    }

    fn activate(&mut self, saved_submission: PromptSubmission, browse: bool) {
        self.snapshot.open = true;
        self.snapshot.browse = browse;
        self.snapshot.saved_submission = saved_submission;
        self.snapshot.query.clear();
        self.refresh_matches();
    }

    pub(crate) fn update_query(&mut self, query: &str) {
        if !self.snapshot.open || self.snapshot.browse || self.snapshot.query == query {
            return;
        }
        self.snapshot.query = query.to_string();
        self.refresh_matches();
    }

    fn refresh_matches(&mut self) {
        let query = self.snapshot.query.trim();
        let mut matches: Vec<HistoryMatch> = if query.is_empty() {
            self.entries
                .iter()
                .take(MAX_MATCHES)
                .rev()
                .map(|submission| HistoryMatch {
                    submission: submission.clone(),
                    display: single_line(submission.text()),
                    indices: Vec::new(),
                })
                .collect()
        } else {
            let mut ranked = self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(recency, submission)| {
                    let display = single_line(submission.text());
                    fuzzy_match(&display, query)
                        .map(|(score, indices)| (recency, score, submission, display, indices))
                })
                .collect::<Vec<_>>();
            // Weakest/oldest first, strongest/newest last. The best result is
            // therefore selected at the panel edge nearest the composer.
            ranked.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(&left.0)));
            ranked
                .into_iter()
                .rev()
                .take(MAX_MATCHES)
                .rev()
                .map(|(_, _, submission, display, indices)| HistoryMatch {
                    submission: submission.clone(),
                    display,
                    indices,
                })
                .collect()
        };
        if matches.len() > MAX_MATCHES {
            matches.drain(..matches.len() - MAX_MATCHES);
        }
        self.snapshot.matches = matches;
        self.snapshot.selected = self.snapshot.matches.len().saturating_sub(1);
    }

    pub(crate) fn move_selection(&mut self, delta: isize) -> bool {
        let len = self.snapshot.matches.len();
        if len == 0 {
            return false;
        }
        let selected = (self.snapshot.selected as isize + delta).clamp(0, len as isize - 1);
        let selected = selected as usize;
        if selected == self.snapshot.selected {
            return false;
        }
        self.snapshot.selected = selected;
        true
    }

    pub(crate) fn page_selection(&mut self, delta: isize, visible_rows: usize) -> bool {
        let page = (visible_rows / 2).max(1) as isize;
        self.move_selection(delta.saturating_mul(page))
    }

    pub(crate) fn select(&mut self, index: usize) {
        if self.snapshot.matches.is_empty() {
            return;
        }
        self.snapshot.selected = index.min(self.snapshot.matches.len() - 1);
    }

    pub(crate) fn selected_submission(&self) -> Option<&PromptSubmission> {
        self.snapshot.selection().map(|entry| &entry.submission)
    }

    pub(crate) fn accept(&mut self) -> PromptSubmission {
        let submission = self
            .selected_submission()
            .cloned()
            .unwrap_or_else(|| self.snapshot.saved_submission.clone());
        self.deactivate();
        submission
    }

    pub(crate) fn cancel(&mut self) -> PromptSubmission {
        let saved = self.snapshot.saved_submission.clone();
        self.deactivate();
        saved
    }

    pub(crate) fn detach(&mut self) {
        self.deactivate();
    }

    fn deactivate(&mut self) {
        self.snapshot = HistorySnapshot::default();
    }
}

fn single_line(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_whitespace() {
                ' '
            } else {
                character
            }
        })
        .collect()
}
