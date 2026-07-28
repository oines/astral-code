use astral_tui_scrollback::TimelineStream;
use codex_app_server_protocol::ThreadItem;

use super::ConversationState;
use super::model::ConversationEntry;
use super::model::EntryPhase;
use super::model::MutationSource;
use super::model::TextStreamKind;

impl ConversationState {
    pub(super) fn finalize_segmented_text(
        &mut self,
        turn_index: usize,
        item: &ThreadItem,
        completed_at_ms: Option<i64>,
    ) -> bool {
        let Some(provider_id) = non_empty(item.id()) else {
            return false;
        };
        let Some(kind) = text_kind(item) else {
            return false;
        };
        let entry_indices = self.turns[turn_index]
            .entries
            .iter()
            .enumerate()
            .filter_map(|(entry_index, entry)| {
                (entry.phase != EntryPhase::Stable
                    && entry.provider_id.as_deref() == Some(provider_id)
                    && entry_text_kind(entry) == Some(kind))
                .then_some(entry_index)
            })
            .collect::<Vec<_>>();
        if entry_indices.len() < 2 {
            return false;
        }

        self.close_reasoning(turn_index);
        let mut segment_texts = entry_indices
            .iter()
            .map(|entry_index| projected_text(&self.turns[turn_index].entries[*entry_index], kind))
            .collect::<Vec<_>>();
        let authoritative = item_text(item);
        let leading = segment_texts[..segment_texts.len() - 1].concat();
        if let Some(tail) = authoritative.strip_prefix(&leading) {
            let last = segment_texts.len() - 1;
            segment_texts[last] = tail.to_string();
        }

        for (entry_index, text) in entry_indices.iter().copied().zip(segment_texts) {
            let entry = &mut self.turns[turn_index].entries[entry_index];
            entry.item = Some(text_item_with_text(item, text));
            entry.stream = TimelineStream::None;
            entry.completed_at_ms = completed_at_ms;
            entry.phase = EntryPhase::Stable;
        }
        if self.turns[turn_index]
            .active_text
            .is_some_and(|(entry_index, _)| entry_indices.contains(&entry_index))
        {
            self.turns[turn_index].active_text = None;
        }
        true
    }

    pub(super) fn stream_entry(
        &mut self,
        turn_index: usize,
        item_id: &str,
        kind: TextStreamKind,
    ) -> usize {
        if let Some((entry_index, active_kind)) = self.turns[turn_index].active_text
            && active_kind == kind
            && entry_matches_id(&self.turns[turn_index].entries[entry_index], item_id)
        {
            return entry_index;
        }
        self.close_text(turn_index);
        self.allocate_entry(turn_index, non_empty(item_id).map(str::to_owned))
    }

    pub(super) fn reasoning_entry(&mut self, turn_index: usize, item_id: &str) -> usize {
        self.close_text(turn_index);
        if let Some(entry_index) = self.turns[turn_index].active_reasoning
            && entry_matches_id(&self.turns[turn_index].entries[entry_index], item_id)
        {
            return entry_index;
        }
        self.close_reasoning(turn_index);
        self.allocate_entry(turn_index, non_empty(item_id).map(str::to_owned))
    }

    pub(super) fn text_item_entry(
        &mut self,
        turn_index: usize,
        item_id: &str,
        kind: TextStreamKind,
        source: MutationSource,
        lifecycle: ItemLifecycle,
    ) -> usize {
        if source == MutationSource::Live {
            if let Some((entry_index, active_kind)) = self.turns[turn_index].active_text
                && active_kind == kind
                && entry_matches_id(&self.turns[turn_index].entries[entry_index], item_id)
            {
                return entry_index;
            }
            if lifecycle == ItemLifecycle::Completed
                && let Some(provider_id) = non_empty(item_id)
                && let Some(entry_index) = self.turns[turn_index]
                    .provider_indices
                    .get(provider_id)
                    .copied()
                && entry_text_kind(&self.turns[turn_index].entries[entry_index]) == Some(kind)
                && self.turns[turn_index].entries[entry_index].phase != EntryPhase::Stable
            {
                return entry_index;
            }
        }
        self.close_text(turn_index);
        self.allocate_entry(turn_index, non_empty(item_id).map(str::to_owned))
    }

    pub(super) fn reasoning_item_entry(
        &mut self,
        turn_index: usize,
        item_id: &str,
        source: MutationSource,
        lifecycle: ItemLifecycle,
    ) -> usize {
        self.close_text(turn_index);
        if source == MutationSource::Live {
            if let Some(entry_index) = self.turns[turn_index].active_reasoning
                && entry_matches_id(&self.turns[turn_index].entries[entry_index], item_id)
            {
                return entry_index;
            }
            if lifecycle == ItemLifecycle::Completed
                && let Some(provider_id) = non_empty(item_id)
                && let Some(entry_index) = self.turns[turn_index]
                    .provider_indices
                    .get(provider_id)
                    .copied()
                && entry_is_reasoning(&self.turns[turn_index].entries[entry_index])
                && self.turns[turn_index].entries[entry_index].phase != EntryPhase::Stable
            {
                return entry_index;
            }
        }
        self.close_reasoning(turn_index);
        self.allocate_entry(turn_index, non_empty(item_id).map(str::to_owned))
    }
}

pub(super) fn text_kind(item: &ThreadItem) -> Option<TextStreamKind> {
    match item {
        ThreadItem::AgentMessage { .. } => Some(TextStreamKind::Agent),
        ThreadItem::Plan { .. } => Some(TextStreamKind::Plan),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ItemLifecycle {
    Started,
    Completed,
}

fn entry_matches_id(entry: &ConversationEntry, item_id: &str) -> bool {
    match (entry.provider_id.as_deref(), non_empty(item_id)) {
        (Some(provider_id), Some(item_id)) => provider_id == item_id,
        (None, None) => true,
        _ => false,
    }
}

fn entry_text_kind(entry: &ConversationEntry) -> Option<TextStreamKind> {
    entry
        .item
        .as_ref()
        .and_then(text_kind)
        .or(match &entry.stream {
            TimelineStream::AgentMessage(_) => Some(TextStreamKind::Agent),
            TimelineStream::Plan(_) => Some(TextStreamKind::Plan),
            _ => None,
        })
}

fn entry_is_reasoning(entry: &ConversationEntry) -> bool {
    matches!(&entry.item, Some(ThreadItem::Reasoning { .. }))
        || matches!(&entry.stream, TimelineStream::Reasoning { .. })
}

fn projected_text(entry: &ConversationEntry, kind: TextStreamKind) -> String {
    let mut text = entry.item.as_ref().map_or_else(String::new, |item| {
        text_kind(item)
            .filter(|item_kind| *item_kind == kind)
            .map_or_else(String::new, |_| item_text(item).to_string())
    });
    match (&entry.stream, kind) {
        (TimelineStream::AgentMessage(delta), TextStreamKind::Agent)
        | (TimelineStream::Plan(delta), TextStreamKind::Plan) => text.push_str(delta),
        _ => {}
    }
    text
}

fn item_text(item: &ThreadItem) -> &str {
    match item {
        ThreadItem::AgentMessage { text, .. } | ThreadItem::Plan { text, .. } => text,
        _ => "",
    }
}

fn text_item_with_text(item: &ThreadItem, text: String) -> ThreadItem {
    let mut item = item.clone();
    match &mut item {
        ThreadItem::AgentMessage {
            text: item_text, ..
        }
        | ThreadItem::Plan {
            text: item_text, ..
        } => *item_text = text,
        _ => {}
    }
    item
}

fn non_empty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}
