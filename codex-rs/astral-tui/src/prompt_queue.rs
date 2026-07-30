use std::collections::VecDeque;

use crate::PromptSubmission;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueuedPrompt {
    id: u64,
    submission: PromptSubmission,
    expanded_text: String,
}

impl QueuedPrompt {
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn text(&self) -> &str {
        &self.expanded_text
    }

    pub(crate) fn submission(&self) -> &PromptSubmission {
        &self.submission
    }
}

#[derive(Debug, Default)]
pub(crate) struct PromptQueue {
    entries: VecDeque<QueuedPrompt>,
    next_id: u64,
    selected_id: Option<u64>,
    focused: bool,
}

impl PromptQueue {
    pub(crate) fn push_back(&mut self, submission: PromptSubmission) {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let expanded_text = submission.expanded_text();
        self.entries.push_back(QueuedPrompt {
            id,
            submission,
            expanded_text,
        });
        self.selected_id.get_or_insert(id);
    }

    pub(crate) fn push_front(&mut self, prompt: QueuedPrompt) {
        self.selected_id = Some(prompt.id);
        self.entries.push_front(prompt);
    }

    pub(crate) fn pop_front(&mut self) -> Option<QueuedPrompt> {
        let prompt = self.entries.pop_front()?;
        if self.selected_id == Some(prompt.id) {
            self.selected_id = self.entries.front().map(|entry| entry.id);
        }
        if self.entries.is_empty() {
            self.focused = false;
        }
        Some(prompt)
    }

    pub(crate) fn entries(&self) -> &VecDeque<QueuedPrompt> {
        &self.entries
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn focused(&self) -> bool {
        self.focused
    }

    pub(crate) fn focus(&mut self) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        self.focused = true;
        if self.selected_id.is_none() {
            self.selected_id = self.entries.front().map(|entry| entry.id);
        }
        true
    }

    pub(crate) fn toggle_focus(&mut self) -> bool {
        if self.focused {
            self.blur();
            true
        } else {
            self.focus()
        }
    }

    pub(crate) fn blur(&mut self) {
        self.focused = false;
    }

    pub(crate) fn selected_id(&self) -> Option<u64> {
        self.selected_id
    }

    pub(crate) fn selected(&self) -> Option<&QueuedPrompt> {
        let id = self.selected_id?;
        self.get(id)
    }

    pub(crate) fn front_id(&self) -> Option<u64> {
        self.entries.front().map(|entry| entry.id)
    }

    pub(crate) fn get(&self, id: u64) -> Option<&QueuedPrompt> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub(crate) fn select(&mut self, id: u64) -> bool {
        if self.get(id).is_none() {
            return false;
        }
        self.selected_id = Some(id);
        self.focused = true;
        true
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let Some(selected) = self
            .selected_id
            .and_then(|id| self.entries.iter().position(|entry| entry.id == id))
        else {
            self.selected_id = self.entries.front().map(|entry| entry.id);
            return;
        };
        let last = self.entries.len().saturating_sub(1);
        let next = selected.saturating_add_signed(delta).min(last);
        self.selected_id = self.entries.get(next).map(|entry| entry.id);
    }

    pub(crate) fn remove_selected(&mut self) -> Option<QueuedPrompt> {
        let selected = self.selected_id?;
        self.remove(selected)
    }

    pub(crate) fn remove(&mut self, id: u64) -> Option<QueuedPrompt> {
        let index = self.entries.iter().position(|entry| entry.id == id)?;
        let removed = self.entries.remove(index)?;
        let next = index.min(self.entries.len().saturating_sub(1));
        self.selected_id = self.entries.get(next).map(|entry| entry.id);
        if self.entries.is_empty() {
            self.focused = false;
        }
        Some(removed)
    }

    pub(crate) fn swap_selected(&mut self, delta: isize) {
        let Some(selected) = self
            .selected_id
            .and_then(|id| self.entries.iter().position(|entry| entry.id == id))
        else {
            return;
        };
        let last = self.entries.len().saturating_sub(1);
        let target = selected.saturating_add_signed(delta).min(last);
        if selected != target {
            self.entries.swap(selected, target);
        }
    }

    pub(crate) fn replace(&mut self, id: u64, submission: PromptSubmission) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        entry.expanded_text = submission.expanded_text();
        entry.submission = submission;
        true
    }
}

#[derive(Debug)]
pub(crate) struct PromptQueueEdit {
    pub(crate) id: u64,
    pub(crate) stashed_submission: PromptSubmission,
}
