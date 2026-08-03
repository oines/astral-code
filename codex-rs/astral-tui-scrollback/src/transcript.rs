use std::collections::HashMap;
use std::collections::VecDeque;

use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnError;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnPlanUpdatedNotification;
use codex_app_server_protocol::TurnStatus;

use crate::LiveItem;

mod apply;
mod turn;

/// Stable local identity used by selection, expansion, and scroll anchoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TranscriptEntryId(u64);

/// Lifecycle state for one transcript item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryLifecycle {
    /// Loaded from an authoritative thread or turn snapshot.
    Restored,
    Running {
        started_at_ms: Option<i64>,
    },
    Completed {
        started_at_ms: Option<i64>,
        completed_at_ms: i64,
    },
}

/// One presentation entry anchored to an exact protocol item.
///
/// A provider may reuse one assistant item id across semantic boundaries. In
/// that case multiple entries retain the same authoritative completed item,
/// while `presentation_text` carries the TUI-only slice rendered at each
/// source position.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptEntry {
    id: TranscriptEntryId,
    item: ThreadItem,
    live: LiveItem,
    lifecycle: EntryLifecycle,
    presentation_text: Option<String>,
}

impl TranscriptEntry {
    pub fn id(&self) -> TranscriptEntryId {
        self.id
    }

    pub fn item(&self) -> &ThreadItem {
        &self.item
    }

    pub fn live(&self) -> &LiveItem {
        &self.live
    }

    pub fn lifecycle(&self) -> EntryLifecycle {
        self.lifecycle
    }

    pub(crate) fn presentation_text(&self) -> Option<&str> {
        self.presentation_text.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextStreamKind {
    AgentMessage,
    Plan,
}

/// One authoritative app-server turn and its TUI-only auxiliary state.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptTurn {
    id: String,
    entries: Vec<TranscriptEntry>,
    entry_indices: HashMap<String, usize>,
    active_text: Option<(usize, TextStreamKind)>,
    active_reasoning: Option<usize>,
    status: TurnStatus,
    error: Option<TurnError>,
    items_view: TurnItemsView,
    plan: Option<TurnPlanUpdatedNotification>,
    diff: Option<String>,
}

impl TranscriptTurn {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn entries(&self) -> &[TranscriptEntry] {
        &self.entries
    }

    pub fn status(&self) -> &TurnStatus {
        &self.status
    }

    pub fn plan(&self) -> Option<&TurnPlanUpdatedNotification> {
        self.plan.as_ref()
    }

    pub fn diff(&self) -> Option<&str> {
        self.diff.as_deref()
    }
}

/// A lifecycle gap that should be recovered from an authoritative thread snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptGap {
    MissingTurn,
    MissingItem,
    ItemNotRunning,
    InvalidItemId,
    InvalidPartIndex,
}

/// Result of projecting one app-server notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    Applied,
    DifferentThread,
    NotTranscript,
    NeedsSnapshot(TranscriptGap),
}

/// Ordered transcript for exactly one app-server thread.
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    thread_id: String,
    turns: Vec<TranscriptTurn>,
    turn_indices: HashMap<String, usize>,
    next_entry_id: u64,
}

impl Transcript {
    pub fn new(thread_id: impl Into<String>) -> Self {
        Self {
            thread_id: thread_id.into(),
            turns: Vec::new(),
            turn_indices: HashMap::new(),
            next_entry_id: 0,
        }
    }

    pub fn from_thread(thread: &Thread) -> Self {
        let mut transcript = Self::new(thread.id.clone());
        transcript.reset_from_thread(thread);
        transcript
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub fn turns(&self) -> &[TranscriptTurn] {
        &self.turns
    }

    /// Replace authoritative content from an app-server thread snapshot while
    /// retaining local identities for matching turn/item occurrences.
    pub fn reset_from_thread(&mut self, thread: &Thread) {
        let mut previous_turns = if self.thread_id == thread.id {
            std::mem::take(&mut self.turns)
                .into_iter()
                .map(|turn| (turn.id.clone(), turn))
                .collect::<HashMap<_, _>>()
        } else {
            self.turns.clear();
            HashMap::new()
        };
        self.thread_id.clone_from(&thread.id);
        self.turn_indices.clear();
        for turn in &thread.turns {
            let transcript_turn = if let Some(mut previous) = previous_turns.remove(&turn.id) {
                previous.plan = None;
                previous.diff = None;
                previous.replace_from_snapshot(turn, &mut self.next_entry_id);
                previous
            } else {
                Self::turn_from_snapshot(turn, &mut self.next_entry_id)
            };
            self.turn_indices
                .insert(transcript_turn.id.clone(), self.turns.len());
            self.turns.push(transcript_turn);
        }
    }

    fn replace_turn(&mut self, turn_index: usize, snapshot: &Turn) {
        self.turns[turn_index].replace_from_snapshot(snapshot, &mut self.next_entry_id);
    }

    fn turn_from_snapshot(snapshot: &Turn, next_entry_id: &mut u64) -> TranscriptTurn {
        let mut turn = TranscriptTurn {
            id: snapshot.id.clone(),
            entries: Vec::new(),
            entry_indices: HashMap::new(),
            active_text: None,
            active_reasoning: None,
            status: snapshot.status.clone(),
            error: snapshot.error.clone(),
            items_view: snapshot.items_view,
            plan: None,
            diff: None,
        };
        turn.replace_from_snapshot(snapshot, next_entry_id);
        turn
    }

    fn is_active_thread(&self, thread_id: &str) -> bool {
        self.thread_id == thread_id
    }

    fn turn_index(&self, turn_id: &str) -> Option<usize> {
        self.turn_indices.get(turn_id).copied()
    }

    fn turn_mut(&mut self, turn_id: &str) -> Option<&mut TranscriptTurn> {
        let index = self.turn_index(turn_id)?;
        self.turns.get_mut(index)
    }
}

impl TranscriptTurn {
    fn replace_from_snapshot(&mut self, snapshot: &Turn, next_entry_id: &mut u64) {
        if matches!(snapshot.items_view, TurnItemsView::Full) {
            let mut previous = HashMap::<String, VecDeque<TranscriptEntry>>::new();
            for entry in std::mem::take(&mut self.entries) {
                previous
                    .entry(entry.item.id().to_owned())
                    .or_default()
                    .push_back(entry);
            }
            self.entries = snapshot
                .items
                .iter()
                .cloned()
                .map(|item| {
                    if let Some(mut entry) =
                        previous.get_mut(item.id()).and_then(VecDeque::pop_front)
                    {
                        entry.item = item;
                        entry.live = LiveItem::None;
                        entry.presentation_text = None;
                        if matches!(entry.lifecycle, EntryLifecycle::Running { .. }) {
                            entry.lifecycle = EntryLifecycle::Restored;
                        }
                        entry
                    } else {
                        TranscriptEntry {
                            id: allocate_entry_id(next_entry_id),
                            item,
                            live: LiveItem::None,
                            lifecycle: EntryLifecycle::Restored,
                            presentation_text: None,
                        }
                    }
                })
                .collect();
            self.rebuild_entry_indices();
        }
        self.id.clone_from(&snapshot.id);
        self.status.clone_from(&snapshot.status);
        self.error.clone_from(&snapshot.error);
        self.items_view = snapshot.items_view;
        self.active_text = None;
        self.active_reasoning = None;
    }

    fn rebuild_entry_indices(&mut self) {
        self.entry_indices.clear();
        for (index, entry) in self.entries.iter().enumerate() {
            if !entry.item.id().is_empty() {
                self.entry_indices.insert(entry.item.id().to_owned(), index);
            }
        }
    }

    fn item_index(&self, item_id: &str) -> Option<usize> {
        if item_id.is_empty() {
            return None;
        }
        self.entry_indices.get(item_id).copied()
    }

    fn entry_mut(&mut self, item_id: &str) -> Option<&mut TranscriptEntry> {
        let index = self.item_index(item_id)?;
        self.entries.get_mut(index)
    }
}

fn allocate_entry_id(next_entry_id: &mut u64) -> TranscriptEntryId {
    let id = TranscriptEntryId(*next_entry_id);
    *next_entry_id = next_entry_id.saturating_add(1);
    id
}

fn text_stream_kind(item: &ThreadItem) -> Option<TextStreamKind> {
    match item {
        ThreadItem::AgentMessage { .. } => Some(TextStreamKind::AgentMessage),
        ThreadItem::Plan { .. } => Some(TextStreamKind::Plan),
        _ => None,
    }
}

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;
