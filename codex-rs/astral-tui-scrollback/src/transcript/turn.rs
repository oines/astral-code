use codex_app_server_protocol::ThreadItem;

use super::EntryLifecycle;
use super::TextStreamKind;
use super::TranscriptEntry;
use super::TranscriptTurn;
use super::allocate_entry_id;
use super::text_stream_kind;
use crate::LiveItem;

enum CompletedPresentation {
    Item,
    Text(String),
}

impl TranscriptTurn {
    pub(super) fn start_item(
        &mut self,
        item: ThreadItem,
        started_at_ms: Option<i64>,
        next_entry_id: &mut u64,
    ) {
        if let Some(kind) = text_stream_kind(&item) {
            self.close_reasoning();
            if let Some((index, active_kind)) = self.active_text
                && active_kind == kind
                && entry_matches_item(&self.entries[index], &item)
            {
                let entry = &mut self.entries[index];
                entry.item = item;
                entry.lifecycle = EntryLifecycle::Running { started_at_ms };
                entry.presentation_text = None;
                return;
            }
            self.close_text();
            let index = self.append_running(item, started_at_ms, next_entry_id);
            self.active_text = Some((index, kind));
            return;
        }
        if matches!(item, ThreadItem::Reasoning { .. }) {
            self.close_text();
            if let Some(index) = self.active_reasoning
                && entry_matches_item(&self.entries[index], &item)
            {
                let entry = &mut self.entries[index];
                entry.item = item;
                entry.lifecycle = EntryLifecycle::Running { started_at_ms };
                entry.presentation_text = None;
                return;
            }
            self.close_reasoning();
            let index = self.append_running(item, started_at_ms, next_entry_id);
            self.active_reasoning = Some(index);
            return;
        }

        self.close_streams();
        if let Some(index) = self.item_index(item.id()) {
            let entry = &mut self.entries[index];
            entry.item = item;
            entry.lifecycle = EntryLifecycle::Running { started_at_ms };
            entry.presentation_text = None;
            return;
        }
        self.append_running(item, started_at_ms, next_entry_id);
    }

    pub(super) fn complete_item(&mut self, item: ThreadItem, completed_at_ms: i64) -> bool {
        if let Some(kind) = text_stream_kind(&item) {
            return self.complete_text(item, kind, completed_at_ms);
        }
        if matches!(item, ThreadItem::Reasoning { .. }) {
            return self.complete_reasoning(item, completed_at_ms);
        }

        let Some(index) = self.item_index(item.id()) else {
            return false;
        };
        self.set_completed(index, item, completed_at_ms);
        true
    }

    pub(super) fn complete_or_append_item(
        &mut self,
        item: ThreadItem,
        completed_at_ms: i64,
        next_entry_id: &mut u64,
    ) {
        if self.complete_item(item.clone(), completed_at_ms) {
            return;
        }
        self.close_streams();
        self.append_entry(
            item,
            EntryLifecycle::Completed {
                started_at_ms: None,
                completed_at_ms,
            },
            next_entry_id,
        );
    }

    fn complete_text(
        &mut self,
        item: ThreadItem,
        kind: TextStreamKind,
        completed_at_ms: i64,
    ) -> bool {
        let running = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                (matches!(entry.lifecycle, EntryLifecycle::Running { .. })
                    && entry_matches_item(entry, &item)
                    && text_stream_kind(&entry.item) == Some(kind))
                .then_some(index)
            })
            .collect::<Vec<_>>();

        let completed_indices = if running.len() > 1 {
            self.complete_text_segments(&running, &item, kind, completed_at_ms);
            running
        } else {
            let Some(index) = running.first().copied().or_else(|| {
                self.entries
                    .last()
                    .filter(|entry| {
                        entry_matches_item(entry, &item)
                            && text_stream_kind(&entry.item) == Some(kind)
                    })
                    .map(|_| self.entries.len() - 1)
            }) else {
                return false;
            };
            self.set_completed(index, item, completed_at_ms);
            vec![index]
        };
        if self
            .active_text
            .is_some_and(|(index, _)| completed_indices.contains(&index))
        {
            self.active_text = None;
        }
        true
    }

    fn complete_text_segments(
        &mut self,
        indices: &[usize],
        item: &ThreadItem,
        kind: TextStreamKind,
        completed_at_ms: i64,
    ) {
        let mut presentation_texts = indices
            .iter()
            .map(|index| projected_text(&self.entries[*index], kind))
            .collect::<Vec<_>>();
        let leading = presentation_texts[..presentation_texts.len() - 1].concat();
        if let Some(tail) = item_text(item).strip_prefix(&leading) {
            let last = presentation_texts.len() - 1;
            presentation_texts[last] = tail.to_owned();
        } else {
            // A completed item is authoritative. If contributor processing or
            // normalization changed the text, the streamed slices no longer
            // define a trustworthy split. Hide the stale leading drafts and
            // render the exact completed text at the final source position.
            presentation_texts.fill(String::new());
            let last = presentation_texts.len() - 1;
            presentation_texts[last] = item_text(item).to_owned();
        }
        for (index, presentation_text) in indices.iter().copied().zip(presentation_texts) {
            self.set_completed_text_segment(
                index,
                item.clone(),
                completed_at_ms,
                presentation_text,
            );
        }
    }

    fn complete_reasoning(&mut self, item: ThreadItem, completed_at_ms: i64) -> bool {
        let index = self
            .entries
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, entry)| {
                (matches!(entry.lifecycle, EntryLifecycle::Running { .. })
                    && entry_matches_item(entry, &item)
                    && matches!(entry.item, ThreadItem::Reasoning { .. }))
                .then_some(index)
            })
            .or_else(|| {
                self.entries
                    .last()
                    .filter(|entry| {
                        entry_matches_item(entry, &item)
                            && matches!(entry.item, ThreadItem::Reasoning { .. })
                    })
                    .map(|_| self.entries.len() - 1)
            });
        let Some(index) = index else {
            return false;
        };
        self.set_completed(index, item, completed_at_ms);
        if self.active_reasoning == Some(index) {
            self.active_reasoning = None;
        }
        true
    }

    pub(super) fn stream_entry(
        &mut self,
        placeholder: ThreadItem,
        next_entry_id: &mut u64,
    ) -> Option<usize> {
        if let Some(kind) = text_stream_kind(&placeholder) {
            self.close_reasoning();
            if let Some((index, active_kind)) = self.active_text
                && active_kind == kind
                && entry_matches_item(&self.entries[index], &placeholder)
            {
                return Some(index);
            }
            if self
                .active_text
                .is_some_and(|(_, active_kind)| active_kind == kind)
                && self.entries.iter().any(|entry| {
                    matches!(entry.lifecycle, EntryLifecycle::Completed { .. })
                        && entry_matches_item(entry, &placeholder)
                        && text_stream_kind(&entry.item) == Some(kind)
                })
            {
                return None;
            }
            self.close_text();
            if let Some(index) = self.last_resumable_stream(&placeholder) {
                self.active_text = Some((index, kind));
                return Some(index);
            }
            if last_completed_matches(&self.entries, &placeholder) {
                return None;
            }
            let index = self.append_running(placeholder, None, next_entry_id);
            self.active_text = Some((index, kind));
            return Some(index);
        }

        self.close_text();
        if let Some(index) = self.active_reasoning
            && entry_matches_item(&self.entries[index], &placeholder)
        {
            return Some(index);
        }
        if self.active_reasoning.is_some()
            && self.entries.iter().any(|entry| {
                matches!(entry.lifecycle, EntryLifecycle::Completed { .. })
                    && entry_matches_item(entry, &placeholder)
                    && matches!(entry.item, ThreadItem::Reasoning { .. })
            })
        {
            return None;
        }
        self.close_reasoning();
        if let Some(index) = self.last_resumable_stream(&placeholder) {
            self.active_reasoning = Some(index);
            return Some(index);
        }
        if last_completed_matches(&self.entries, &placeholder) {
            return None;
        }
        let index = self.append_running(placeholder, None, next_entry_id);
        self.active_reasoning = Some(index);
        Some(index)
    }

    fn append_running(
        &mut self,
        item: ThreadItem,
        started_at_ms: Option<i64>,
        next_entry_id: &mut u64,
    ) -> usize {
        self.append_entry(
            item,
            EntryLifecycle::Running { started_at_ms },
            next_entry_id,
        )
    }

    fn append_entry(
        &mut self,
        item: ThreadItem,
        lifecycle: EntryLifecycle,
        next_entry_id: &mut u64,
    ) -> usize {
        let index = self.entries.len();
        if !item.id().is_empty() {
            self.entry_indices.insert(item.id().to_owned(), index);
        }
        self.entries.push(TranscriptEntry {
            id: allocate_entry_id(next_entry_id),
            item,
            live: LiveItem::None,
            lifecycle,
            presentation_text: None,
        });
        index
    }

    fn set_completed(&mut self, index: usize, item: ThreadItem, completed_at_ms: i64) {
        self.finish_entry(index, item, completed_at_ms, CompletedPresentation::Item);
    }

    fn set_completed_text_segment(
        &mut self,
        index: usize,
        item: ThreadItem,
        completed_at_ms: i64,
        presentation_text: String,
    ) {
        self.finish_entry(
            index,
            item,
            completed_at_ms,
            CompletedPresentation::Text(presentation_text),
        );
    }

    fn finish_entry(
        &mut self,
        index: usize,
        item: ThreadItem,
        completed_at_ms: i64,
        presentation: CompletedPresentation,
    ) {
        let entry = &mut self.entries[index];
        let started_at_ms = match entry.lifecycle {
            EntryLifecycle::Running { started_at_ms } => started_at_ms,
            EntryLifecycle::Completed { started_at_ms, .. } => started_at_ms,
            EntryLifecycle::Restored => None,
        };
        entry.item = item;
        entry.live = LiveItem::None;
        entry.presentation_text = match presentation {
            CompletedPresentation::Item => None,
            CompletedPresentation::Text(text) => Some(text),
        };
        entry.lifecycle = EntryLifecycle::Completed {
            started_at_ms,
            completed_at_ms,
        };
    }

    fn last_resumable_stream(&self, item: &ThreadItem) -> Option<usize> {
        self.entries.last().and_then(|entry| {
            (entry_matches_item(entry, item)
                && same_stream_kind(&entry.item, item)
                && matches!(entry.lifecycle, EntryLifecycle::Running { .. }))
            .then_some(self.entries.len() - 1)
        })
    }

    fn close_streams(&mut self) {
        self.close_text();
        self.close_reasoning();
    }

    fn close_text(&mut self) {
        self.active_text = None;
    }

    fn close_reasoning(&mut self) {
        self.active_reasoning = None;
    }
}

fn entry_matches_item(entry: &TranscriptEntry, item: &ThreadItem) -> bool {
    entry.item.id() == item.id()
}

fn last_completed_matches(entries: &[TranscriptEntry], item: &ThreadItem) -> bool {
    entries.last().is_some_and(|entry| {
        matches!(entry.lifecycle, EntryLifecycle::Completed { .. })
            && entry_matches_item(entry, item)
            && same_stream_kind(&entry.item, item)
    })
}

fn same_stream_kind(current: &ThreadItem, incoming: &ThreadItem) -> bool {
    text_stream_kind(current) == text_stream_kind(incoming)
        && (text_stream_kind(current).is_some()
            || matches!(
                (current, incoming),
                (ThreadItem::Reasoning { .. }, ThreadItem::Reasoning { .. })
            ))
}

fn projected_text(entry: &TranscriptEntry, kind: TextStreamKind) -> String {
    let mut text = item_text(&entry.item).to_owned();
    match (&entry.live, kind) {
        (LiveItem::AgentMessage(delta), TextStreamKind::AgentMessage)
        | (LiveItem::Plan(delta), TextStreamKind::Plan) => text.push_str(delta),
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
