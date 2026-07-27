use std::collections::HashMap;
use std::collections::HashSet;

use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnStatus;

use crate::PresentationBlock;
use crate::ReduceOutcome;
use crate::TimelineEntry;
use crate::TimelineState;

#[derive(Debug, Clone, PartialEq)]
pub struct CommittedBlock {
    pub item_id: String,
    pub turn_id: String,
    pub block: PresentationBlock,
    pub started_at_ms: Option<i64>,
    pub completed_at_ms: Option<i64>,
    pub turn_started_at_ms: Option<i64>,
    pub turn_completed_at_ms: Option<i64>,
    pub turn_duration_ms: Option<i64>,
    pub ends_turn: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TranscriptBlock {
    pub(crate) item_id: String,
    pub(crate) block: PresentationBlock,
    pub(crate) started_at_ms: Option<i64>,
    pub(crate) completed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TranscriptTurn {
    pub(crate) id: String,
    pub(crate) blocks: Vec<TranscriptBlock>,
    pub(crate) started_at_ms: Option<i64>,
    pub(crate) completed_at_ms: Option<i64>,
    pub(crate) duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TurnTiming {
    started_at_ms: Option<i64>,
    completed_at_ms: Option<i64>,
    duration_ms: Option<i64>,
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
    turn_timings: HashMap<String, TurnTiming>,
}

impl ConversationState {
    pub fn new(thread_id: impl Into<String>) -> Self {
        Self {
            timeline: TimelineState::new(thread_id),
            committed_entries: 0,
            completed_turns: HashSet::new(),
            turn_timings: HashMap::new(),
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
        state.turn_timings.extend(turns.iter().map(|turn| {
            (
                turn.id.clone(),
                TurnTiming {
                    started_at_ms: turn.started_at.map(seconds_to_millis),
                    completed_at_ms: turn.completed_at.map(seconds_to_millis),
                    duration_ms: turn.duration_ms,
                },
            )
        }));
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
                self.record_turn_timing(&params.turn);
            }
            ServerNotification::TurnCompleted(params)
                if params.thread_id == self.timeline.thread_id() =>
            {
                self.completed_turns.insert(params.turn.id.clone());
                self.record_turn_timing(&params.turn);
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
                let timing = self.turn_timing(entry.turn_id());
                let ends_turn = self
                    .timeline
                    .entries()
                    .get(self.committed_entries)
                    .is_none_or(|next| next.turn_id() != entry.turn_id());
                blocks.push(CommittedBlock {
                    item_id: entry.id().to_string(),
                    turn_id: entry.turn_id().to_string(),
                    block,
                    started_at_ms: entry.started_at_ms(),
                    completed_at_ms: entry.completed_at_ms(),
                    turn_started_at_ms: timing.started_at_ms,
                    turn_completed_at_ms: timing.completed_at_ms,
                    turn_duration_ms: timing.duration_ms,
                    ends_turn,
                });
            }
        }
        blocks
    }

    pub(crate) fn live_turns(&self) -> Vec<TranscriptTurn> {
        self.project_turns(&self.timeline.entries()[self.committed_entries..])
    }

    pub(crate) fn all_turns(&self) -> Vec<TranscriptTurn> {
        self.project_turns(self.timeline.entries())
    }

    pub fn last_agent_response(&self) -> Option<&str> {
        self.timeline
            .entries()
            .iter()
            .rev()
            .filter_map(TimelineEntry::item)
            .find_map(|item| match item {
                ThreadItem::AgentMessage { text, .. } => Some(text.as_str()),
                _ => None,
            })
    }

    pub fn committed_entries(&self) -> usize {
        self.committed_entries
    }

    fn record_turn_timing(&mut self, turn: &Turn) {
        self.turn_timings.insert(
            turn.id.clone(),
            TurnTiming {
                started_at_ms: turn.started_at.map(seconds_to_millis),
                completed_at_ms: turn.completed_at.map(seconds_to_millis),
                duration_ms: turn.duration_ms,
            },
        );
    }

    fn turn_timing(&self, turn_id: &str) -> TurnTiming {
        self.turn_timings.get(turn_id).copied().unwrap_or_default()
    }

    fn project_turns(&self, entries: &[TimelineEntry]) -> Vec<TranscriptTurn> {
        let mut turns = Vec::<TranscriptTurn>::new();
        for entry in entries {
            let Some(block) = project_entry(entry) else {
                continue;
            };
            if turns.last().is_none_or(|turn| turn.id != entry.turn_id()) {
                let timing = self.turn_timing(entry.turn_id());
                turns.push(TranscriptTurn {
                    id: entry.turn_id().to_string(),
                    blocks: Vec::new(),
                    started_at_ms: timing.started_at_ms,
                    completed_at_ms: timing.completed_at_ms,
                    duration_ms: timing.duration_ms,
                });
            }
            if let Some(turn) = turns.last_mut() {
                turn.blocks.push(TranscriptBlock {
                    item_id: entry.id().to_string(),
                    block,
                    started_at_ms: entry.started_at_ms(),
                    completed_at_ms: entry.completed_at_ms(),
                });
            }
        }
        turns
    }
}

fn seconds_to_millis(seconds: i64) -> i64 {
    seconds.saturating_mul(1_000)
}

fn project_entry(entry: &TimelineEntry) -> Option<PresentationBlock> {
    if let Some(presentation) = entry.presentation() {
        return Some(presentation.clone());
    }
    match entry.item() {
        Some(item) => PresentationBlock::from_item(item, entry.stream()),
        None => PresentationBlock::from_stream(entry.stream()),
    }
}

#[cfg(test)]
#[path = "conversation_tests.rs"]
mod tests;
