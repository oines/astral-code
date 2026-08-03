use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;

use super::ApplyOutcome;
use super::EntryLifecycle;
use super::Transcript;
use super::TranscriptGap;
use crate::LiveItem;

impl Transcript {
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
                self.turns[turn_index].start_item(
                    event.item.clone(),
                    Some(event.started_at_ms),
                    &mut self.next_entry_id,
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
                if !turn.complete_item(event.item.clone(), event.completed_at_ms) {
                    return ApplyOutcome::NeedsSnapshot(TranscriptGap::MissingItem);
                }
                ApplyOutcome::Applied
            }
            ServerNotification::AgentMessageDelta(event) => self.apply_stream_delta(
                &event.thread_id,
                &event.turn_id,
                ThreadItem::AgentMessage {
                    id: event.item_id.clone(),
                    text: String::new(),
                    phase: None,
                    memory_citation: None,
                },
                |live| live.append_agent_message(&event.delta),
            ),
            ServerNotification::PlanDelta(event) => self.apply_stream_delta(
                &event.thread_id,
                &event.turn_id,
                ThreadItem::Plan {
                    id: event.item_id.clone(),
                    text: String::new(),
                },
                |live| live.append_plan(&event.delta),
            ),
            ServerNotification::ReasoningSummaryPartAdded(event) => {
                let Ok(index) = usize::try_from(event.summary_index) else {
                    return ApplyOutcome::NeedsSnapshot(TranscriptGap::InvalidPartIndex);
                };
                self.apply_stream_delta(
                    &event.thread_id,
                    &event.turn_id,
                    empty_reasoning(&event.item_id),
                    |live| live.add_reasoning_summary_part(index),
                )
            }
            ServerNotification::ReasoningSummaryTextDelta(event) => {
                let Ok(index) = usize::try_from(event.summary_index) else {
                    return ApplyOutcome::NeedsSnapshot(TranscriptGap::InvalidPartIndex);
                };
                self.apply_stream_delta(
                    &event.thread_id,
                    &event.turn_id,
                    empty_reasoning(&event.item_id),
                    |live| live.append_reasoning_summary(index, &event.delta),
                )
            }
            ServerNotification::ReasoningTextDelta(event) => {
                let Ok(index) = usize::try_from(event.content_index) else {
                    return ApplyOutcome::NeedsSnapshot(TranscriptGap::InvalidPartIndex);
                };
                self.apply_stream_delta(
                    &event.thread_id,
                    &event.turn_id,
                    empty_reasoning(&event.item_id),
                    |live| live.append_reasoning_content(index, &event.delta),
                )
            }
            ServerNotification::CommandExecutionOutputDelta(event) => {
                self.apply_item_delta(&event.thread_id, &event.turn_id, &event.item_id, |live| {
                    live.append_command_output(&event.delta);
                })
            }
            ServerNotification::TerminalInteraction(event) => {
                self.apply_item_delta(&event.thread_id, &event.turn_id, &event.item_id, |live| {
                    live.append_terminal_input(&event.stdin);
                })
            }
            ServerNotification::FileChangePatchUpdated(event) => {
                self.apply_item_delta(&event.thread_id, &event.turn_id, &event.item_id, |live| {
                    live.replace_file_changes(event.changes.clone());
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

    fn apply_stream_delta(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        placeholder: ThreadItem,
        update: impl FnOnce(&mut LiveItem),
    ) -> ApplyOutcome {
        if !self.is_active_thread(thread_id) {
            return ApplyOutcome::DifferentThread;
        }
        if placeholder.id().is_empty() {
            return ApplyOutcome::NeedsSnapshot(TranscriptGap::InvalidItemId);
        }
        let Some(turn_index) = self.turn_index(turn_id) else {
            return ApplyOutcome::NeedsSnapshot(TranscriptGap::MissingTurn);
        };
        let entry_index = self.turns[turn_index].stream_entry(placeholder, &mut self.next_entry_id);
        let Some(entry_index) = entry_index else {
            return ApplyOutcome::NeedsSnapshot(TranscriptGap::ItemNotRunning);
        };
        update(&mut self.turns[turn_index].entries[entry_index].live);
        ApplyOutcome::Applied
    }

    pub(super) fn insert_or_replace_turn(&mut self, snapshot: &Turn) {
        if let Some(index) = self.turn_index(&snapshot.id) {
            self.replace_turn(index, snapshot);
        } else {
            let turn = Self::turn_from_snapshot(snapshot, &mut self.next_entry_id);
            self.turn_indices.insert(turn.id.clone(), self.turns.len());
            self.turns.push(turn);
        }
    }
}

fn empty_reasoning(item_id: &str) -> ThreadItem {
    ThreadItem::Reasoning {
        id: item_id.to_owned(),
        summary: Vec::new(),
        content: Vec::new(),
    }
}
