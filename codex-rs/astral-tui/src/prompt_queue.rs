use std::collections::VecDeque;

use crate::PromptSubmission;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueuedPrompt {
    id: u64,
    submission: PromptSubmission,
}

impl QueuedPrompt {
    pub(crate) fn text(&self) -> &str {
        self.submission.text()
    }

    pub(crate) fn submission(&self) -> &PromptSubmission {
        &self.submission
    }
}

#[derive(Debug, Default)]
pub(crate) struct PromptQueue {
    entries: VecDeque<QueuedPrompt>,
    next_id: u64,
}

impl PromptQueue {
    pub(crate) fn push_back(&mut self, submission: PromptSubmission) {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.entries.push_back(QueuedPrompt { id, submission });
    }

    pub(crate) fn push_front(&mut self, prompt: QueuedPrompt) {
        self.entries.push_front(prompt);
    }

    pub(crate) fn pop_front(&mut self) -> Option<QueuedPrompt> {
        self.entries.pop_front()
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
}
