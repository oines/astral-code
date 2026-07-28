use astral_tui_scrollback::PresentationBlock;
use astral_tui_scrollback::TodoPresentation;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnStatus;

use super::ConversationState;
use super::ReduceOutcome;
use super::model::ConversationEntry;
use super::model::ConversationTurn;
use super::model::EntryLocation;
use super::model::EntryPhase;
use super::model::MutationSource;
use super::model::TextStreamKind;
use super::model::TranscriptMutation;
use super::model::TurnTiming;
use super::state::text_kind;

impl ConversationState {
    pub fn from_turns(thread_id: impl Into<String>, turns: &[Turn]) -> Self {
        let mut state = Self::new(thread_id);
        for turn in turns {
            let turn_index = state.ensure_turn(&turn.id);
            state.turns[turn_index].timing = turn_timing(turn);
            for item in &turn.items {
                state.apply_item(
                    turn_index,
                    item.clone(),
                    /*completed_at_ms*/ None,
                    MutationSource::Replay,
                );
            }
            if turn.status != TurnStatus::InProgress {
                state.finish_turn(turn_index);
            }
        }
        state
    }

    pub fn apply(&mut self, notification: &ServerNotification) -> ReduceOutcome {
        match notification {
            ServerNotification::TurnStarted(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ReduceOutcome::DifferentThread;
                }
                let turn_index = self.ensure_turn(&event.turn.id);
                let turn = &mut self.turns[turn_index];
                turn.sealed = false;
                turn.timing = turn_timing(&event.turn);
                ReduceOutcome::Applied
            }
            ServerNotification::TurnCompleted(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ReduceOutcome::DifferentThread;
                }
                let turn_index = self.ensure_turn(&event.turn.id);
                self.turns[turn_index].timing = turn_timing(&event.turn);
                if event.turn.status != TurnStatus::InProgress {
                    self.finish_turn(turn_index);
                }
                ReduceOutcome::Applied
            }
            ServerNotification::ItemStarted(event) => self.apply_thread_mutation(
                &event.thread_id,
                &event.turn_id,
                TranscriptMutation::ItemStarted {
                    item: event.item.clone(),
                    started_at_ms: event.started_at_ms,
                },
            ),
            ServerNotification::ItemCompleted(event) => self.apply_thread_mutation(
                &event.thread_id,
                &event.turn_id,
                TranscriptMutation::ItemCompleted {
                    item: event.item.clone(),
                    completed_at_ms: event.completed_at_ms,
                },
            ),
            ServerNotification::AgentMessageDelta(event) => self.apply_thread_mutation(
                &event.thread_id,
                &event.turn_id,
                TranscriptMutation::AgentMessageDelta {
                    item_id: event.item_id.clone(),
                    delta: event.delta.clone(),
                },
            ),
            ServerNotification::PlanDelta(event) => self.apply_thread_mutation(
                &event.thread_id,
                &event.turn_id,
                TranscriptMutation::PlanDelta {
                    item_id: event.item_id.clone(),
                    delta: event.delta.clone(),
                },
            ),
            ServerNotification::ReasoningSummaryTextDelta(event) => {
                let Ok(index) = usize::try_from(event.summary_index) else {
                    return ReduceOutcome::Ignored;
                };
                self.apply_thread_mutation(
                    &event.thread_id,
                    &event.turn_id,
                    TranscriptMutation::ReasoningSummaryDelta {
                        item_id: event.item_id.clone(),
                        index,
                        delta: event.delta.clone(),
                    },
                )
            }
            ServerNotification::ReasoningTextDelta(event) => {
                let Ok(index) = usize::try_from(event.content_index) else {
                    return ReduceOutcome::Ignored;
                };
                self.apply_thread_mutation(
                    &event.thread_id,
                    &event.turn_id,
                    TranscriptMutation::ReasoningContentDelta {
                        item_id: event.item_id.clone(),
                        index,
                        delta: event.delta.clone(),
                    },
                )
            }
            ServerNotification::CommandExecutionOutputDelta(event) => self.apply_thread_mutation(
                &event.thread_id,
                &event.turn_id,
                TranscriptMutation::CommandOutputDelta {
                    item_id: event.item_id.clone(),
                    delta: event.delta.clone(),
                },
            ),
            ServerNotification::TerminalInteraction(event) => self.apply_thread_mutation(
                &event.thread_id,
                &event.turn_id,
                TranscriptMutation::TerminalInteraction {
                    item_id: event.item_id.clone(),
                    process_id: event.process_id.clone(),
                    stdin: event.stdin.clone(),
                },
            ),
            ServerNotification::FileChangeOutputDelta(event) => self.apply_thread_mutation(
                &event.thread_id,
                &event.turn_id,
                TranscriptMutation::FileChangeOutputDelta {
                    item_id: event.item_id.clone(),
                    delta: event.delta.clone(),
                },
            ),
            ServerNotification::FileChangePatchUpdated(event) => self.apply_thread_mutation(
                &event.thread_id,
                &event.turn_id,
                TranscriptMutation::FileChangePatchUpdated {
                    item_id: event.item_id.clone(),
                    changes: event.changes.clone(),
                },
            ),
            ServerNotification::TurnPlanUpdated(event) => self.apply_thread_mutation(
                &event.thread_id,
                &event.turn_id,
                TranscriptMutation::TurnPlanUpdated(event.clone()),
            ),
            ServerNotification::TurnDiffUpdated(event) => {
                if !self.is_active_thread(&event.thread_id) {
                    return ReduceOutcome::DifferentThread;
                }
                self.turn_diff = Some(event.diff.clone());
                ReduceOutcome::Applied
            }
            _ => ReduceOutcome::Ignored,
        }
    }

    fn apply_thread_mutation(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        mutation: TranscriptMutation,
    ) -> ReduceOutcome {
        if !self.is_active_thread(thread_id) {
            return ReduceOutcome::DifferentThread;
        }
        let turn_index = self.ensure_turn(turn_id);
        self.apply_mutation(turn_index, mutation, MutationSource::Live, true);
        ReduceOutcome::Applied
    }

    fn apply_mutation(
        &mut self,
        turn_index: usize,
        mutation: TranscriptMutation,
        source: MutationSource,
        allow_defer: bool,
    ) {
        if allow_defer && should_defer(&self.turns[turn_index], &mutation) {
            self.turns[turn_index].deferred.push_back(mutation);
            return;
        }

        let completed_active_text = active_text_completed(&self.turns[turn_index], &mutation);
        match mutation {
            TranscriptMutation::ItemStarted {
                item,
                started_at_ms,
            } => {
                let entry_index = self.prepare_item(turn_index, &item, source);
                let entry = &mut self.turns[turn_index].entries[entry_index];
                entry.item = Some(item.clone());
                entry.started_at_ms = Some(started_at_ms);
                entry.phase = EntryPhase::Running;
                self.note_item_role(turn_index, entry_index, &item);
                self.index_process(turn_index, entry_index, &item);
            }
            TranscriptMutation::ItemCompleted {
                item,
                completed_at_ms,
            } => {
                self.apply_item(turn_index, item, Some(completed_at_ms), source);
            }
            TranscriptMutation::AgentMessageDelta { item_id, delta } => {
                self.close_reasoning(turn_index);
                let entry_index = self.stream_entry(turn_index, &item_id, TextStreamKind::Agent);
                let turn = &mut self.turns[turn_index];
                turn.entries[entry_index]
                    .stream
                    .append_agent_message(&delta);
                turn.entries[entry_index].phase = EntryPhase::Running;
                turn.active_text = Some((entry_index, TextStreamKind::Agent));
            }
            TranscriptMutation::PlanDelta { item_id, delta } => {
                self.close_reasoning(turn_index);
                let entry_index = self.stream_entry(turn_index, &item_id, TextStreamKind::Plan);
                let turn = &mut self.turns[turn_index];
                turn.entries[entry_index].stream.append_plan(&delta);
                turn.entries[entry_index].phase = EntryPhase::Running;
                turn.active_text = Some((entry_index, TextStreamKind::Plan));
            }
            TranscriptMutation::ReasoningSummaryDelta {
                item_id,
                index,
                delta,
            } => {
                let entry_index = self.reasoning_entry(turn_index, &item_id);
                let turn = &mut self.turns[turn_index];
                turn.entries[entry_index]
                    .stream
                    .append_reasoning_summary(index, &delta);
                turn.entries[entry_index].phase = EntryPhase::Running;
                turn.active_reasoning = Some(entry_index);
            }
            TranscriptMutation::ReasoningContentDelta {
                item_id,
                index,
                delta,
            } => {
                let entry_index = self.reasoning_entry(turn_index, &item_id);
                let turn = &mut self.turns[turn_index];
                turn.entries[entry_index]
                    .stream
                    .append_reasoning_content(index, &delta);
                turn.entries[entry_index].phase = EntryPhase::Running;
                turn.active_reasoning = Some(entry_index);
            }
            TranscriptMutation::CommandOutputDelta { item_id, delta } => {
                let entry_index = self.provider_entry(turn_index, &item_id);
                self.turns[turn_index].entries[entry_index]
                    .stream
                    .append_command_output(&delta);
            }
            TranscriptMutation::TerminalInteraction {
                item_id,
                process_id,
                stdin,
            } => {
                let entry_index = self
                    .process_entries
                    .get(&process_id)
                    .filter(|location| location.turn == turn_index)
                    .map(|location| location.entry)
                    .unwrap_or_else(|| self.provider_entry(turn_index, &item_id));
                self.process_entries.insert(
                    process_id.clone(),
                    EntryLocation {
                        turn: turn_index,
                        entry: entry_index,
                    },
                );
                self.turns[turn_index].entries[entry_index]
                    .stream
                    .append_terminal_input(&process_id, &stdin);
            }
            TranscriptMutation::FileChangeOutputDelta { item_id, delta } => {
                let entry_index = self.provider_entry(turn_index, &item_id);
                self.turns[turn_index].entries[entry_index]
                    .stream
                    .append_file_change_output(&delta);
            }
            TranscriptMutation::FileChangePatchUpdated { item_id, changes } => {
                let entry_index = self.provider_entry(turn_index, &item_id);
                self.turns[turn_index].entries[entry_index]
                    .stream
                    .replace_file_changes(changes);
            }
            TranscriptMutation::TurnPlanUpdated(event) => {
                let entry_index = self.todo_entry(turn_index, None);
                self.turn_plan = Some(event.clone());
                let entry = &mut self.turns[turn_index].entries[entry_index];
                entry.presentation = Some(PresentationBlock::Todo(TodoPresentation::from(&event)));
                entry.phase = EntryPhase::Settling;
            }
        }

        if completed_active_text {
            self.turns[turn_index].active_text = None;
            self.drain_deferred(turn_index);
        }
    }

    pub(super) fn drain_deferred(&mut self, turn_index: usize) {
        while let Some(mutation) = self.turns[turn_index].deferred.pop_front() {
            self.apply_mutation(turn_index, mutation, MutationSource::Live, false);
        }
    }

    fn is_active_thread(&self, thread_id: &str) -> bool {
        self.thread_id == thread_id
    }
}

fn should_defer(turn: &ConversationTurn, mutation: &TranscriptMutation) -> bool {
    let Some((entry_index, kind)) = turn.active_text else {
        return false;
    };
    let entry = &turn.entries[entry_index];
    match mutation {
        TranscriptMutation::AgentMessageDelta { item_id, .. } => {
            kind != TextStreamKind::Agent || !entry_matches_id(entry, item_id)
        }
        TranscriptMutation::PlanDelta { item_id, .. } => {
            kind != TextStreamKind::Plan || !entry_matches_id(entry, item_id)
        }
        TranscriptMutation::ItemStarted { item, .. }
        | TranscriptMutation::ItemCompleted { item, .. } => {
            text_kind(item) != Some(kind) || !entry_matches_id(entry, item.id())
        }
        _ => true,
    }
}

fn active_text_completed(turn: &ConversationTurn, mutation: &TranscriptMutation) -> bool {
    let Some((entry_index, kind)) = turn.active_text else {
        return false;
    };
    let TranscriptMutation::ItemCompleted { item, .. } = mutation else {
        return false;
    };
    text_kind(item) == Some(kind) && entry_matches_id(&turn.entries[entry_index], item.id())
}

fn entry_matches_id(entry: &ConversationEntry, item_id: &str) -> bool {
    match (entry.provider_id.as_deref(), non_empty(item_id)) {
        (Some(provider_id), Some(item_id)) => provider_id == item_id,
        (None, None) => true,
        _ => false,
    }
}

fn non_empty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

fn turn_timing(turn: &Turn) -> TurnTiming {
    TurnTiming {
        started_at_ms: turn.started_at.map(seconds_to_millis),
        completed_at_ms: turn.completed_at.map(seconds_to_millis),
        duration_ms: turn.duration_ms,
    }
}

fn seconds_to_millis(seconds: i64) -> i64 {
    seconds.saturating_mul(1_000)
}
