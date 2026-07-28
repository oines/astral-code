mod model;
mod reducer;
mod state;

use std::collections::HashMap;

use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnPlanUpdatedNotification;

use crate::PresentationBlock;

use self::model::ConversationEntry;
use self::model::ConversationTurn;
use self::model::EntryLocation;
use self::model::EntryPhase;

/// Whether an app-server notification changed the active conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReduceOutcome {
    Applied,
    Ignored,
    DifferentThread,
}

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

/// Canonical transcript state for one app-server thread.
///
/// The app-server remains authoritative for runtime semantics. This state only
/// preserves Codex's presentation lifecycle: turns own their entries, streamed
/// assistant text delays interrupting tool rows, and mutable tool projections
/// settle before they are printed into terminal-native scrollback.
#[derive(Debug, Clone)]
pub struct ConversationState {
    thread_id: String,
    turns: Vec<ConversationTurn>,
    turn_indices: HashMap<String, usize>,
    process_entries: HashMap<String, EntryLocation>,
    next_entry_id: u64,
    commit_turn: usize,
    turn_plan: Option<TurnPlanUpdatedNotification>,
    turn_diff: Option<String>,
    skipped_events: usize,
}

impl ConversationState {
    pub fn new(thread_id: impl Into<String>) -> Self {
        Self {
            thread_id: thread_id.into(),
            turns: Vec::new(),
            turn_indices: HashMap::new(),
            process_entries: HashMap::new(),
            next_entry_id: 0,
            commit_turn: 0,
            turn_plan: None,
            turn_diff: None,
            skipped_events: 0,
        }
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub fn turn_plan(&self) -> Option<&TurnPlanUpdatedNotification> {
        self.turn_plan.as_ref()
    }

    pub fn turn_diff(&self) -> Option<&str> {
        self.turn_diff.as_deref()
    }

    pub fn skipped_events(&self) -> usize {
        self.skipped_events
    }

    pub fn record_lag(&mut self, skipped: usize) {
        self.skipped_events = self.skipped_events.saturating_add(skipped);
    }

    pub fn drain_committable(&mut self) -> Vec<CommittedBlock> {
        let mut blocks = Vec::new();
        while let Some(turn) = self.turns.get(self.commit_turn) {
            let Some(entry) = turn.entries.get(turn.committed_entries) else {
                if turn.sealed {
                    self.commit_turn += 1;
                    continue;
                }
                break;
            };
            let is_tail = turn.committed_entries + 1 == turn.entries.len();
            if entry.phase != EntryPhase::Stable || is_tail && !turn.sealed {
                break;
            }

            let entry = entry.clone();
            let turn_id = turn.id.clone();
            let timing = turn.timing;
            let ends_turn = turn.sealed && is_tail;
            self.turns[self.commit_turn].committed_entries += 1;
            if let Some(block) = project_entry(&entry) {
                blocks.push(CommittedBlock {
                    item_id: entry.render_id(),
                    turn_id,
                    block,
                    started_at_ms: entry.started_at_ms,
                    completed_at_ms: entry.completed_at_ms,
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
        self.turns
            .iter()
            .enumerate()
            .skip(self.commit_turn)
            .filter_map(|(turn_index, turn)| {
                let start = if turn_index == self.commit_turn {
                    turn.committed_entries
                } else {
                    0
                };
                project_turn(turn, &turn.entries[start..])
            })
            .collect()
    }

    pub(crate) fn all_turns(&self) -> Vec<TranscriptTurn> {
        self.turns
            .iter()
            .filter_map(|turn| project_turn(turn, &turn.entries))
            .collect()
    }

    pub fn last_agent_response(&self) -> Option<&str> {
        self.turns
            .iter()
            .rev()
            .flat_map(|turn| turn.entries.iter().rev())
            .filter_map(|entry| entry.item.as_ref())
            .find_map(|item| match item {
                ThreadItem::AgentMessage { text, .. } => Some(text.as_str()),
                _ => None,
            })
    }

    pub fn committed_entries(&self) -> usize {
        self.turns.iter().map(|turn| turn.committed_entries).sum()
    }
}

fn project_turn(turn: &ConversationTurn, entries: &[ConversationEntry]) -> Option<TranscriptTurn> {
    let blocks = entries
        .iter()
        .filter_map(|entry| {
            project_entry(entry).map(|block| TranscriptBlock {
                item_id: entry.render_id(),
                block,
                started_at_ms: entry.started_at_ms,
                completed_at_ms: entry.completed_at_ms,
            })
        })
        .collect::<Vec<_>>();
    (!blocks.is_empty()).then_some(TranscriptTurn {
        id: turn.id.clone(),
        blocks,
        started_at_ms: turn.timing.started_at_ms,
        completed_at_ms: turn.timing.completed_at_ms,
        duration_ms: turn.timing.duration_ms,
    })
}

fn project_entry(entry: &ConversationEntry) -> Option<PresentationBlock> {
    let mut block = if let Some(presentation) = &entry.presentation {
        Some(presentation.clone())
    } else {
        match &entry.item {
            Some(item) => PresentationBlock::from_item(item, &entry.stream),
            None => PresentationBlock::from_stream(&entry.stream),
        }
    }?;
    if entry.phase == EntryPhase::Running {
        match &mut block {
            PresentationBlock::Plan { running, .. }
            | PresentationBlock::Thinking { running, .. } => *running = true,
            _ => {}
        }
    }
    Some(block)
}

#[cfg(test)]
#[path = "conversation_tests.rs"]
mod tests;
