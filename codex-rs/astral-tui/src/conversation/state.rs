use astral_tui_scrollback::ToolKind;
use astral_tui_scrollback::classify_tool_name;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::CoreToolCallStatus;
use codex_app_server_protocol::DynamicToolCallStatus;
use codex_app_server_protocol::McpToolCallStatus;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::ThreadItem;

use super::ConversationState;
use super::model::ConversationEntry;
use super::model::ConversationTurn;
use super::model::EntryLocation;
use super::model::EntryPhase;
use super::model::MutationSource;
use super::model::TextStreamKind;
use super::streams::ItemLifecycle;
use super::streams::text_kind;

impl ConversationState {
    pub(super) fn apply_item(
        &mut self,
        turn_index: usize,
        item: ThreadItem,
        completed_at_ms: Option<i64>,
        source: MutationSource,
    ) {
        if let ThreadItem::AgentMessage { text, .. } = &item {
            self.last_agent_response = Some(text.clone());
        }
        if source == MutationSource::Live
            && text_kind(&item).is_some()
            && self.finalize_segmented_text(turn_index, &item, completed_at_ms)
        {
            return;
        }
        let entry_index = self.prepare_completed_item(turn_index, &item, source);
        let phase = if item_is_running(&item) {
            EntryPhase::Running
        } else if item_can_be_superseded(&item) && !self.turns[turn_index].sealed {
            EntryPhase::Settling
        } else {
            EntryPhase::Stable
        };
        let entry = &mut self.turns[turn_index].entries[entry_index];
        entry.item = Some(item.clone());
        entry.stream = Default::default();
        entry.completion_observed = true;
        entry.completed_at_ms = completed_at_ms;
        entry.phase = phase;
        self.note_item_role(turn_index, entry_index, &item);
        self.index_process(turn_index, entry_index, &item);
    }

    pub(super) fn ensure_turn(&mut self, turn_id: &str) -> usize {
        if let Some(index) = self.turn_indices.get(turn_id).copied() {
            return index;
        }
        let index = self.turns.len();
        self.turns.push(ConversationTurn::new(turn_id.to_owned()));
        self.turn_indices.insert(turn_id.to_owned(), index);
        index
    }

    pub(super) fn finish_turn(&mut self, turn_index: usize) {
        self.close_reasoning(turn_index);
        self.close_text(turn_index);
        let turn = &mut self.turns[turn_index];
        turn.active_reasoning = None;
        turn.active_text = None;
        turn.sealed = true;
        for entry in &mut turn.entries {
            entry.phase = EntryPhase::Stable;
        }
    }

    pub(super) fn close_reasoning(&mut self, turn_index: usize) {
        let turn = &mut self.turns[turn_index];
        if let Some(entry_index) = turn.active_reasoning.take()
            && turn.entries[entry_index].phase == EntryPhase::Running
        {
            turn.entries[entry_index].phase = EntryPhase::Settling;
        }
    }

    pub(super) fn close_text(&mut self, turn_index: usize) {
        let turn = &mut self.turns[turn_index];
        if let Some((entry_index, _)) = turn.active_text.take()
            && turn.entries[entry_index].phase == EntryPhase::Running
        {
            turn.entries[entry_index].phase = EntryPhase::Settling;
        }
    }

    pub(super) fn provider_entry(&mut self, turn_index: usize, provider_id: &str) -> usize {
        if let Some(entry_index) = self.turns[turn_index]
            .provider_indices
            .get(provider_id)
            .copied()
        {
            return entry_index;
        }
        self.close_reasoning(turn_index);
        self.close_text(turn_index);
        self.allocate_entry(turn_index, non_empty(provider_id).map(str::to_owned))
    }

    pub(super) fn todo_entry(&mut self, turn_index: usize, provider_id: Option<String>) -> usize {
        let entry_index = match self.turns[turn_index].todo_entry {
            Some(entry_index) => entry_index,
            None => {
                let entry_index = self.allocate_entry(turn_index, provider_id.clone());
                self.turns[turn_index].todo_entry = Some(entry_index);
                entry_index
            }
        };
        if let Some(provider_id) = provider_id {
            self.turns[turn_index]
                .provider_indices
                .insert(provider_id.clone(), entry_index);
            self.turns[turn_index].entries[entry_index].provider_id = Some(provider_id);
        }
        entry_index
    }

    pub(super) fn prepare_started_item(
        &mut self,
        turn_index: usize,
        item: &ThreadItem,
        source: MutationSource,
    ) -> usize {
        self.item_entry(turn_index, item, source, ItemLifecycle::Started)
    }

    fn prepare_completed_item(
        &mut self,
        turn_index: usize,
        item: &ThreadItem,
        source: MutationSource,
    ) -> usize {
        self.item_entry(turn_index, item, source, ItemLifecycle::Completed)
    }

    pub(super) fn note_item_role(
        &mut self,
        turn_index: usize,
        entry_index: usize,
        item: &ThreadItem,
    ) {
        let turn = &mut self.turns[turn_index];
        match item {
            ThreadItem::AgentMessage { .. } => {
                if turn.entries[entry_index].phase == EntryPhase::Running {
                    turn.active_text = Some((entry_index, TextStreamKind::Agent));
                } else if turn
                    .active_text
                    .is_some_and(|(active_entry, _)| active_entry == entry_index)
                {
                    turn.active_text = None;
                }
            }
            ThreadItem::Plan { .. } => {
                if turn.entries[entry_index].phase == EntryPhase::Running {
                    turn.active_text = Some((entry_index, TextStreamKind::Plan));
                } else if turn
                    .active_text
                    .is_some_and(|(active_entry, _)| active_entry == entry_index)
                {
                    turn.active_text = None;
                }
            }
            ThreadItem::Reasoning { .. } => {
                if turn.entries[entry_index].phase == EntryPhase::Running {
                    turn.active_reasoning = Some(entry_index);
                } else if turn.active_reasoning == Some(entry_index) {
                    turn.active_reasoning = None;
                }
            }
            _ => {}
        }
    }

    fn item_entry(
        &mut self,
        turn_index: usize,
        item: &ThreadItem,
        source: MutationSource,
        lifecycle: ItemLifecycle,
    ) -> usize {
        if is_todo_item(item) {
            if self.turns[turn_index].todo_entry.is_none() {
                self.close_reasoning(turn_index);
                self.close_text(turn_index);
            }
            let provider_id = non_empty(item.id()).map(str::to_owned);
            return self.todo_entry(turn_index, provider_id);
        }
        if let Some(kind) = text_kind(item) {
            self.close_reasoning(turn_index);
            return self.text_item_entry(turn_index, item.id(), kind, source, lifecycle);
        }
        if matches!(item, ThreadItem::Reasoning { .. }) {
            return self.reasoning_item_entry(turn_index, item.id(), source, lifecycle);
        }
        let provider_id = non_empty(item.id()).map(str::to_owned);
        if let Some(provider_id) = provider_id.as_deref()
            && let Some(entry_index) = self.turns[turn_index]
                .provider_indices
                .get(provider_id)
                .copied()
        {
            return entry_index;
        }
        self.close_reasoning(turn_index);
        self.close_text(turn_index);
        self.allocate_entry(turn_index, provider_id)
    }

    pub(super) fn allocate_entry(
        &mut self,
        turn_index: usize,
        provider_id: Option<String>,
    ) -> usize {
        let local_id = self.next_entry_id;
        self.next_entry_id = self.next_entry_id.saturating_add(1);
        let turn = &mut self.turns[turn_index];
        let entry_index = turn.entries.len();
        turn.entries
            .push(ConversationEntry::new(local_id, provider_id.clone()));
        if let Some(provider_id) = provider_id {
            turn.provider_indices.insert(provider_id, entry_index);
        }
        entry_index
    }

    pub(super) fn index_process(
        &mut self,
        turn_index: usize,
        entry_index: usize,
        item: &ThreadItem,
    ) {
        let ThreadItem::CommandExecution { process_id, .. } = item else {
            return;
        };
        let location = EntryLocation {
            turn: turn_index,
            entry: entry_index,
        };
        self.process_entries
            .retain(|_, current_location| *current_location != location);
        if let Some(process_id) = process_id {
            self.process_entries.insert(process_id.clone(), location);
        }
    }
}

fn is_todo_item(item: &ThreadItem) -> bool {
    match item {
        ThreadItem::CoreToolCall { tool, .. } | ThreadItem::DynamicToolCall { tool, .. } => {
            classify_tool_name(tool) == ToolKind::Todo
        }
        _ => false,
    }
}

fn item_can_be_superseded(item: &ThreadItem) -> bool {
    match item {
        ThreadItem::CoreToolCall { tool, .. } | ThreadItem::DynamicToolCall { tool, .. } => {
            matches!(classify_tool_name(tool), ToolKind::Edit | ToolKind::Todo)
        }
        _ => false,
    }
}

fn item_is_running(item: &ThreadItem) -> bool {
    match item {
        ThreadItem::CommandExecution { status, .. } => {
            *status == CommandExecutionStatus::InProgress
        }
        ThreadItem::FileChange { status, .. } => *status == PatchApplyStatus::InProgress,
        ThreadItem::McpToolCall { status, .. } => *status == McpToolCallStatus::InProgress,
        ThreadItem::DynamicToolCall { status, .. } => *status == DynamicToolCallStatus::InProgress,
        ThreadItem::CoreToolCall { status, .. } => *status == CoreToolCallStatus::InProgress,
        ThreadItem::CollabAgentToolCall { status, .. } => {
            *status == CollabAgentToolCallStatus::InProgress
        }
        _ => false,
    }
}

fn non_empty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}
