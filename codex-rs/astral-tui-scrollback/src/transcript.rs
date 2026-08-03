use std::collections::HashMap;

use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnError;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnPlanUpdatedNotification;
use codex_app_server_protocol::TurnStatus;

use crate::LiveItem;

/// Stable local identity used by selection, expansion, and scroll anchoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TranscriptEntryId(u64);

/// Lifecycle state for one transcript item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryLifecycle {
    /// Loaded from an authoritative thread or turn snapshot.
    Restored,
    Running {
        started_at_ms: i64,
    },
    Completed {
        started_at_ms: Option<i64>,
        completed_at_ms: i64,
    },
}

/// One protocol item plus its transient live content.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptEntry {
    id: TranscriptEntryId,
    item: ThreadItem,
    live: LiveItem,
    lifecycle: EntryLifecycle,
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
}

/// One authoritative app-server turn and its TUI-only auxiliary state.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptTurn {
    id: String,
    entries: Vec<TranscriptEntry>,
    entry_indices: HashMap<String, usize>,
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

/// A lifecycle gap that should be recovered with an authoritative thread read.
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
    /// retaining local identities for matching thread/turn/item keys.
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

    pub fn apply(&mut self, notification: &ServerNotification) -> ApplyOutcome {
        match notification {
            ServerNotification::TurnStarted(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ApplyOutcome::DifferentThread;
                }
                self.insert_or_replace_turn(&event.turn);
                ApplyOutcome::Applied
            }
            ServerNotification::TurnCompleted(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ApplyOutcome::DifferentThread;
                }
                let Some(turn_index) = self.turn_index(&event.turn.id) else {
                    return ApplyOutcome::NeedsSnapshot(TranscriptGap::MissingTurn);
                };
                self.replace_turn(turn_index, &event.turn);
                ApplyOutcome::Applied
            }
            ServerNotification::ItemStarted(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ApplyOutcome::DifferentThread;
                }
                let Some(turn_index) = self.turn_index(&event.turn_id) else {
                    return ApplyOutcome::NeedsSnapshot(TranscriptGap::MissingTurn);
                };
                let entry_id = TranscriptEntryId(self.next_entry_id);
                self.next_entry_id = self.next_entry_id.saturating_add(1);
                self.turns[turn_index].start_item(
                    event.item.clone(),
                    event.started_at_ms,
                    entry_id,
                );
                ApplyOutcome::Applied
            }
            ServerNotification::ItemCompleted(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ApplyOutcome::DifferentThread;
                }
                let Some(turn) = self.turn_mut(&event.turn_id) else {
                    return ApplyOutcome::NeedsSnapshot(TranscriptGap::MissingTurn);
                };
                if turn.complete_item(event.item.clone(), event.completed_at_ms) {
                    ApplyOutcome::Applied
                } else {
                    ApplyOutcome::NeedsSnapshot(TranscriptGap::MissingItem)
                }
            }
            ServerNotification::AgentMessageDelta(event) => {
                self.apply_item_delta(&event.thread_id, &event.turn_id, &event.item_id, |live| {
                    live.append_agent_message(&event.delta)
                })
            }
            ServerNotification::PlanDelta(event) => {
                self.apply_item_delta(&event.thread_id, &event.turn_id, &event.item_id, |live| {
                    live.append_plan(&event.delta)
                })
            }
            ServerNotification::ReasoningSummaryPartAdded(event) => {
                let Ok(index) = usize::try_from(event.summary_index) else {
                    return ApplyOutcome::NeedsSnapshot(TranscriptGap::InvalidPartIndex);
                };
                self.apply_item_delta(&event.thread_id, &event.turn_id, &event.item_id, |live| {
                    live.add_reasoning_summary_part(index)
                })
            }
            ServerNotification::ReasoningSummaryTextDelta(event) => {
                let Ok(index) = usize::try_from(event.summary_index) else {
                    return ApplyOutcome::NeedsSnapshot(TranscriptGap::InvalidPartIndex);
                };
                self.apply_item_delta(&event.thread_id, &event.turn_id, &event.item_id, |live| {
                    live.append_reasoning_summary(index, &event.delta)
                })
            }
            ServerNotification::ReasoningTextDelta(event) => {
                let Ok(index) = usize::try_from(event.content_index) else {
                    return ApplyOutcome::NeedsSnapshot(TranscriptGap::InvalidPartIndex);
                };
                self.apply_item_delta(&event.thread_id, &event.turn_id, &event.item_id, |live| {
                    live.append_reasoning_content(index, &event.delta)
                })
            }
            ServerNotification::CommandExecutionOutputDelta(event) => {
                self.apply_item_delta(&event.thread_id, &event.turn_id, &event.item_id, |live| {
                    live.append_command_output(&event.delta)
                })
            }
            ServerNotification::TerminalInteraction(event) => {
                self.apply_item_delta(&event.thread_id, &event.turn_id, &event.item_id, |live| {
                    live.append_terminal_input(&event.stdin)
                })
            }
            ServerNotification::FileChangePatchUpdated(event) => {
                self.apply_item_delta(&event.thread_id, &event.turn_id, &event.item_id, |live| {
                    live.replace_file_changes(event.changes.clone())
                })
            }
            ServerNotification::TurnPlanUpdated(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ApplyOutcome::DifferentThread;
                }
                let Some(turn) = self.turn_mut(&event.turn_id) else {
                    return ApplyOutcome::NeedsSnapshot(TranscriptGap::MissingTurn);
                };
                turn.plan = Some(event.clone());
                ApplyOutcome::Applied
            }
            ServerNotification::TurnDiffUpdated(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ApplyOutcome::DifferentThread;
                }
                let Some(turn) = self.turn_mut(&event.turn_id) else {
                    return ApplyOutcome::NeedsSnapshot(TranscriptGap::MissingTurn);
                };
                turn.diff = Some(event.diff.clone());
                ApplyOutcome::Applied
            }
            _ => ApplyOutcome::NotTranscript,
        }
    }

    fn apply_item_delta(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        item_id: &str,
        update: impl FnOnce(&mut LiveItem),
    ) -> ApplyOutcome {
        if !self.is_active_thread(thread_id) {
            return ApplyOutcome::DifferentThread;
        }
        if item_id.is_empty() {
            return ApplyOutcome::NeedsSnapshot(TranscriptGap::InvalidItemId);
        }
        let Some(turn) = self.turn_mut(turn_id) else {
            return ApplyOutcome::NeedsSnapshot(TranscriptGap::MissingTurn);
        };
        let Some(entry) = turn.entry_mut(item_id) else {
            return ApplyOutcome::NeedsSnapshot(TranscriptGap::MissingItem);
        };
        if !matches!(entry.lifecycle, EntryLifecycle::Running { .. }) {
            return ApplyOutcome::NeedsSnapshot(TranscriptGap::ItemNotRunning);
        }
        update(&mut entry.live);
        ApplyOutcome::Applied
    }

    fn insert_or_replace_turn(&mut self, snapshot: &Turn) {
        if let Some(index) = self.turn_index(&snapshot.id) {
            self.replace_turn(index, snapshot);
        } else {
            let turn = Self::turn_from_snapshot(snapshot, &mut self.next_entry_id);
            self.turn_indices.insert(turn.id.clone(), self.turns.len());
            self.turns.push(turn);
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
    fn start_item(
        &mut self,
        item: ThreadItem,
        started_at_ms: i64,
        new_entry_id: TranscriptEntryId,
    ) {
        if let Some(index) = self.item_index(item.id()) {
            let entry = &mut self.entries[index];
            entry.item = item;
            entry.live = LiveItem::None;
            entry.lifecycle = EntryLifecycle::Running { started_at_ms };
            return;
        }
        let index = self.entries.len();
        if !item.id().is_empty() {
            self.entry_indices.insert(item.id().to_owned(), index);
        }
        self.entries.push(TranscriptEntry {
            id: new_entry_id,
            item,
            live: LiveItem::None,
            lifecycle: EntryLifecycle::Running { started_at_ms },
        });
    }

    fn complete_item(&mut self, item: ThreadItem, completed_at_ms: i64) -> bool {
        let Some(index) = self.item_index(item.id()) else {
            return false;
        };
        let entry = &mut self.entries[index];
        let started_at_ms = match entry.lifecycle {
            EntryLifecycle::Running { started_at_ms } => Some(started_at_ms),
            EntryLifecycle::Completed { started_at_ms, .. } => started_at_ms,
            EntryLifecycle::Restored => None,
        };
        entry.item = item;
        entry.live = LiveItem::None;
        entry.lifecycle = EntryLifecycle::Completed {
            started_at_ms,
            completed_at_ms,
        };
        true
    }

    fn replace_from_snapshot(&mut self, snapshot: &Turn, next_entry_id: &mut u64) {
        if matches!(snapshot.items_view, TurnItemsView::Full) {
            let mut previous = std::mem::take(&mut self.entries)
                .into_iter()
                .filter(|entry| !entry.item.id().is_empty())
                .map(|entry| (entry.item.id().to_owned(), entry))
                .collect::<HashMap<_, _>>();
            self.entries = snapshot
                .items
                .iter()
                .cloned()
                .map(|item| {
                    if let Some(mut entry) = previous.remove(item.id()) {
                        entry.item = item;
                        entry.live = LiveItem::None;
                        if matches!(entry.lifecycle, EntryLifecycle::Running { .. }) {
                            entry.lifecycle = EntryLifecycle::Restored;
                        }
                        entry
                    } else {
                        let id = TranscriptEntryId(*next_entry_id);
                        *next_entry_id = next_entry_id.saturating_add(1);
                        TranscriptEntry {
                            id,
                            item,
                            live: LiveItem::None,
                            lifecycle: EntryLifecycle::Restored,
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

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;
