use std::collections::HashMap;

use astral_tui_scrollback::TimelineStream;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnPlanUpdatedNotification;

/// Whether a notification changed the active timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReduceOutcome {
    Applied,
    Ignored,
    DifferentThread,
}

/// One stable transcript position.
///
/// App-server may intentionally reuse a core tool call id for the structured
/// file-change item that supersedes it within one turn. Upserting by turn and
/// item id therefore preserves the correct position and naturally avoids
/// duplicate Edit/Write rows without conflating provider-generated ids across
/// different turns.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineEntry {
    id: String,
    turn_id: String,
    item: Option<ThreadItem>,
    stream: TimelineStream,
    finalized: bool,
    started_at_ms: Option<i64>,
    completed_at_ms: Option<i64>,
}

impl TimelineEntry {
    fn pending(id: String, turn_id: String) -> Self {
        Self {
            id,
            turn_id,
            item: None,
            stream: TimelineStream::None,
            finalized: false,
            started_at_ms: None,
            completed_at_ms: None,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }

    pub fn item(&self) -> Option<&ThreadItem> {
        self.item.as_ref()
    }

    pub fn stream(&self) -> &TimelineStream {
        &self.stream
    }

    pub fn is_finalized(&self) -> bool {
        self.finalized
    }

    pub fn started_at_ms(&self) -> Option<i64> {
        self.started_at_ms
    }

    pub fn completed_at_ms(&self) -> Option<i64> {
        self.completed_at_ms
    }

    pub fn effective_agent_message(&self) -> Option<String> {
        let base = match self.item.as_ref() {
            Some(ThreadItem::AgentMessage { text, .. }) => text.as_str(),
            Some(_) => return None,
            None => "",
        };
        let delta = match &self.stream {
            TimelineStream::AgentMessage(delta) => delta.as_str(),
            TimelineStream::None if !base.is_empty() => "",
            _ if self.item.is_none() => return None,
            _ => "",
        };
        Some(format!("{base}{delta}"))
    }
}

/// Ordered state for one active app-server thread.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineState {
    thread_id: String,
    entries: Vec<TimelineEntry>,
    entry_indices: HashMap<String, HashMap<String, usize>>,
    process_indices: HashMap<String, usize>,
    turn_plan: Option<TurnPlanUpdatedNotification>,
    turn_diff: Option<String>,
    skipped_events: usize,
}

impl TimelineState {
    pub fn new(thread_id: impl Into<String>) -> Self {
        Self {
            thread_id: thread_id.into(),
            entries: Vec::new(),
            entry_indices: HashMap::new(),
            process_indices: HashMap::new(),
            turn_plan: None,
            turn_diff: None,
            skipped_events: 0,
        }
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub fn entries(&self) -> &[TimelineEntry] {
        &self.entries
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

    pub fn apply(&mut self, notification: &ServerNotification) -> ReduceOutcome {
        match notification {
            ServerNotification::ItemStarted(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ReduceOutcome::DifferentThread;
                }
                {
                    let entry = self.entry_mut(event.item.id(), &event.turn_id);
                    entry.turn_id.clone_from(&event.turn_id);
                    entry.id = event.item.id().to_owned();
                    entry.item = Some(event.item.clone());
                    entry.finalized = false;
                    entry.started_at_ms = Some(event.started_at_ms);
                }
                self.index_process(&event.turn_id, &event.item);
                ReduceOutcome::Applied
            }
            ServerNotification::ItemCompleted(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ReduceOutcome::DifferentThread;
                }
                {
                    let entry = self.entry_mut(event.item.id(), &event.turn_id);
                    entry.turn_id.clone_from(&event.turn_id);
                    entry.id = event.item.id().to_owned();
                    entry.item = Some(event.item.clone());
                    entry.stream = TimelineStream::None;
                    entry.finalized = true;
                    entry.completed_at_ms = Some(event.completed_at_ms);
                }
                self.index_process(&event.turn_id, &event.item);
                ReduceOutcome::Applied
            }
            ServerNotification::AgentMessageDelta(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ReduceOutcome::DifferentThread;
                }
                self.entry_mut(&event.item_id, &event.turn_id)
                    .stream
                    .append_agent_message(&event.delta);
                ReduceOutcome::Applied
            }
            ServerNotification::PlanDelta(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ReduceOutcome::DifferentThread;
                }
                self.entry_mut(&event.item_id, &event.turn_id)
                    .stream
                    .append_plan(&event.delta);
                ReduceOutcome::Applied
            }
            ServerNotification::ReasoningSummaryTextDelta(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ReduceOutcome::DifferentThread;
                }
                let Ok(index) = usize::try_from(event.summary_index) else {
                    return ReduceOutcome::Ignored;
                };
                self.entry_mut(&event.item_id, &event.turn_id)
                    .stream
                    .append_reasoning_summary(index, &event.delta);
                ReduceOutcome::Applied
            }
            ServerNotification::ReasoningTextDelta(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ReduceOutcome::DifferentThread;
                }
                let Ok(index) = usize::try_from(event.content_index) else {
                    return ReduceOutcome::Ignored;
                };
                self.entry_mut(&event.item_id, &event.turn_id)
                    .stream
                    .append_reasoning_content(index, &event.delta);
                ReduceOutcome::Applied
            }
            ServerNotification::CommandExecutionOutputDelta(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ReduceOutcome::DifferentThread;
                }
                self.entry_mut(&event.item_id, &event.turn_id)
                    .stream
                    .append_command_output(&event.delta);
                ReduceOutcome::Applied
            }
            ServerNotification::TerminalInteraction(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ReduceOutcome::DifferentThread;
                }
                let index = self
                    .process_indices
                    .get(&event.process_id)
                    .copied()
                    .unwrap_or_else(|| {
                        let index = self.entry_index(&event.item_id, &event.turn_id);
                        self.process_indices.insert(event.process_id.clone(), index);
                        index
                    });
                self.entries[index]
                    .stream
                    .append_terminal_input(&event.process_id, &event.stdin);
                ReduceOutcome::Applied
            }
            ServerNotification::FileChangeOutputDelta(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ReduceOutcome::DifferentThread;
                }
                self.entry_mut(&event.item_id, &event.turn_id)
                    .stream
                    .append_file_change_output(&event.delta);
                ReduceOutcome::Applied
            }
            ServerNotification::FileChangePatchUpdated(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ReduceOutcome::DifferentThread;
                }
                self.entry_mut(&event.item_id, &event.turn_id)
                    .stream
                    .replace_file_changes(event.changes.clone());
                ReduceOutcome::Applied
            }
            ServerNotification::TurnPlanUpdated(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ReduceOutcome::DifferentThread;
                }
                self.turn_plan = Some(event.clone());
                ReduceOutcome::Applied
            }
            ServerNotification::TurnDiffUpdated(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ReduceOutcome::DifferentThread;
                }
                self.turn_diff = Some(event.diff.clone());
                ReduceOutcome::Applied
            }
            // Non-transcript notifications are handled by the app shell,
            // approval controller, status surfaces, or session layer.
            _ => ReduceOutcome::Ignored,
        }
    }

    pub fn replace_from_turns<'a>(
        &mut self,
        turns: impl IntoIterator<Item = (&'a str, &'a [ThreadItem])>,
    ) {
        self.entries.clear();
        self.entry_indices.clear();
        self.process_indices.clear();
        for (turn_id, items) in turns {
            for item in items {
                {
                    let entry = self.entry_mut(item.id(), turn_id);
                    entry.item = Some(item.clone());
                    entry.stream = TimelineStream::None;
                    entry.finalized = true;
                }
                self.index_process(turn_id, item);
            }
        }
    }

    fn is_active_thread(&self, thread_id: &str) -> bool {
        self.thread_id == thread_id
    }

    fn entry_mut(&mut self, item_id: &str, turn_id: &str) -> &mut TimelineEntry {
        let index = self.entry_index(item_id, turn_id);
        &mut self.entries[index]
    }

    fn entry_index(&mut self, item_id: &str, turn_id: &str) -> usize {
        if let Some(index) = self
            .entry_indices
            .get(turn_id)
            .and_then(|entries| entries.get(item_id))
            .copied()
        {
            return index;
        }
        let index = self.entries.len();
        self.entries.push(TimelineEntry::pending(
            item_id.to_owned(),
            turn_id.to_owned(),
        ));
        self.entry_indices
            .entry(turn_id.to_owned())
            .or_default()
            .insert(item_id.to_owned(), index);
        index
    }

    fn index_process(&mut self, turn_id: &str, item: &ThreadItem) {
        let ThreadItem::CommandExecution { id, process_id, .. } = item else {
            return;
        };
        let Some(index) = self
            .entry_indices
            .get(turn_id)
            .and_then(|entries| entries.get(id))
            .copied()
        else {
            return;
        };
        self.process_indices
            .retain(|_, current_index| *current_index != index);
        if let Some(process_id) = process_id {
            self.process_indices.insert(process_id.clone(), index);
        }
    }
}
