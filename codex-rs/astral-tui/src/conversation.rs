use std::collections::HashSet;

use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnStatus;

use crate::PresentationBlock;
use crate::ReduceOutcome;
use crate::TimelineEntry;
use crate::TimelineState;

#[derive(Debug, Clone, PartialEq)]
pub struct CommittedBlock {
    pub item_id: String,
    pub block: PresentationBlock,
}

/// Scrollback commit state for one app-server thread.
///
/// Items remain mutable while their turn is running. This lets a structured
/// file-change item replace the same-id generic tool call before anything is
/// printed into native terminal history.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationState {
    timeline: TimelineState,
    committed_entries: usize,
    completed_turns: HashSet<String>,
}

impl ConversationState {
    pub fn new(thread_id: impl Into<String>) -> Self {
        Self {
            timeline: TimelineState::new(thread_id),
            committed_entries: 0,
            completed_turns: HashSet::new(),
        }
    }

    pub fn from_turns(thread_id: impl Into<String>, turns: &[Turn]) -> Self {
        let mut state = Self::new(thread_id);
        state.timeline.replace_from_turns(
            turns
                .iter()
                .map(|turn| (turn.id.as_str(), turn.items.as_slice())),
        );
        state.completed_turns.extend(
            turns
                .iter()
                .filter(|turn| turn.status != TurnStatus::InProgress)
                .map(|turn| turn.id.clone()),
        );
        state
    }

    pub fn timeline(&self) -> &TimelineState {
        &self.timeline
    }

    pub fn apply(&mut self, notification: &ServerNotification) -> ReduceOutcome {
        let outcome = self.timeline.apply(notification);
        match notification {
            ServerNotification::TurnStarted(params)
                if params.thread_id == self.timeline.thread_id() =>
            {
                self.completed_turns.remove(&params.turn.id);
            }
            ServerNotification::TurnCompleted(params)
                if params.thread_id == self.timeline.thread_id() =>
            {
                self.completed_turns.insert(params.turn.id.clone());
            }
            _ => {}
        }
        outcome
    }

    pub fn record_lag(&mut self, skipped: usize) {
        self.timeline.record_lag(skipped);
    }

    pub fn drain_committable(&mut self) -> Vec<CommittedBlock> {
        let mut blocks = Vec::new();
        while let Some(entry) = self.timeline.entries().get(self.committed_entries) {
            if !entry.is_finalized() || !self.completed_turns.contains(entry.turn_id()) {
                break;
            }
            self.committed_entries += 1;
            if let Some(block) = project_entry(entry) {
                blocks.push(CommittedBlock {
                    item_id: entry.id().to_string(),
                    block,
                });
            }
        }
        blocks
    }

    pub fn live_blocks(&self) -> Vec<PresentationBlock> {
        self.timeline.entries()[self.committed_entries..]
            .iter()
            .filter_map(project_entry)
            .collect()
    }

    pub fn committed_entries(&self) -> usize {
        self.committed_entries
    }
}

fn project_entry(entry: &TimelineEntry) -> Option<PresentationBlock> {
    match entry.item() {
        Some(item) => PresentationBlock::from_item(item, entry.stream()),
        None => PresentationBlock::from_stream(entry.stream()),
    }
}

#[cfg(test)]
#[path = "conversation_tests.rs"]
mod tests;
