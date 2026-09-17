use std::collections::HashMap;

use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ImageDetail;
use codex_protocol::models::TranscriptItem;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::TranscriptEnvelope;
use serde_json::json;
use uuid::Uuid;

use crate::state::AutoCompactWindowIds;

#[derive(Debug, Clone)]
pub(crate) struct HistoryEntry {
    pub(crate) agent_path: Option<String>,
    pub(crate) turn_id: Option<String>,
    pub(crate) window_id: String,
    pub(crate) window_number: u64,
    pub(crate) item_id: String,
    pub(crate) ordinal: u64,
    pub(crate) role: String,
    pub(crate) item_type: String,
    pub(crate) tool_namespace: Option<String>,
    pub(crate) tool_name: Option<String>,
    pub(crate) content: String,
    pub(crate) media: Vec<(String, Option<ImageDetail>)>,
}

/// Rebuildable, process-local view of the append-only rollout.
///
/// The rollout remains authoritative. This archive is created once while a thread is restored and
/// then advanced only after new transcript envelopes have been persisted successfully.
#[derive(Debug, Default)]
pub(crate) struct HistoryArchive {
    entries: Vec<HistoryEntry>,
    item_positions: HashMap<String, usize>,
}

impl HistoryArchive {
    pub(crate) fn from_rollout(items: &[RolloutItem], current_ids: AutoCompactWindowIds) -> Self {
        let first_window_id = crate::session::context_window::infer_window_lineage(items)
            .map(|(_, ids)| ids.first_window_id)
            .unwrap_or(current_ids.first_window_id);
        let mut window_ids = AutoCompactWindowIds {
            first_window_id,
            previous_window_id: None,
            window_id: first_window_id,
        };
        let mut window_number = 0_u64;
        let mut ordinal = 0_u64;
        let mut archive = Self::default();

        for rollout_item in items {
            match rollout_item {
                RolloutItem::TranscriptItem(item) => {
                    let canonical = serde_json::to_string(item).unwrap_or_default();
                    let item_id = Uuid::new_v5(
                        &Uuid::NAMESPACE_OID,
                        format!("astral-history:{ordinal}:{canonical}").as_bytes(),
                    )
                    .to_string();
                    archive.insert(HistoryEntry::from_item(
                        item,
                        None,
                        None,
                        window_ids.window_id.to_string(),
                        window_number,
                        ordinal,
                        item_id,
                    ));
                    ordinal = ordinal.saturating_add(1);
                }
                RolloutItem::TranscriptEnvelope(envelope) => {
                    let identity = &envelope.identity;
                    archive.insert(HistoryEntry::from_item(
                        &envelope.item,
                        identity.agent_path.clone(),
                        identity.turn_id.clone(),
                        identity.window_id.clone(),
                        identity.window_number,
                        identity.ordinal,
                        identity.item_id.clone(),
                    ));
                    ordinal = ordinal.max(identity.ordinal.saturating_add(1));
                }
                RolloutItem::Compacted(compacted) => {
                    (window_number, window_ids) =
                        crate::session::context_window::advance_window_lineage(
                            window_number,
                            window_ids,
                            compacted,
                            rollout_item,
                        );
                }
                _ => {}
            }
        }

        archive
    }

    pub(crate) fn append_envelopes(&mut self, envelopes: &[TranscriptEnvelope]) {
        for envelope in envelopes {
            let identity = &envelope.identity;
            self.insert(HistoryEntry::from_item(
                &envelope.item,
                identity.agent_path.clone(),
                identity.turn_id.clone(),
                identity.window_id.clone(),
                identity.window_number,
                identity.ordinal,
                identity.item_id.clone(),
            ));
        }
    }

    pub(crate) fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    pub(crate) fn get(&self, window_id: &str, item_id: &str) -> Option<&HistoryEntry> {
        let position = *self.item_positions.get(item_id)?;
        self.entries
            .get(position)
            .filter(|entry| entry.window_id == window_id)
    }

    fn insert(&mut self, entry: HistoryEntry) {
        if let Some(position) = self.item_positions.get(&entry.item_id).copied() {
            self.entries[position] = entry;
            return;
        }
        let position = self.entries.len();
        self.item_positions.insert(entry.item_id.clone(), position);
        self.entries.push(entry);
    }
}

impl HistoryEntry {
    fn from_item(
        item: &TranscriptItem,
        agent_path: Option<String>,
        turn_id: Option<String>,
        window_id: String,
        window_number: u64,
        ordinal: u64,
        item_id: String,
    ) -> Self {
        let (role, item_type, tool_namespace, tool_name, content, media) = normalize_item(item);
        Self {
            agent_path,
            turn_id,
            window_id,
            window_number,
            item_id,
            ordinal,
            role,
            item_type,
            tool_namespace,
            tool_name,
            content,
            media,
        }
    }
}

fn normalize_item(
    item: &TranscriptItem,
) -> (
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    Vec<(String, Option<ImageDetail>)>,
) {
    match item {
        TranscriptItem::Message { role, content, .. } => {
            let mut media = Vec::new();
            let text = content
                .iter()
                .filter_map(|content| match content {
                    ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                        Some(text.clone())
                    }
                    ContentItem::InputImage { image_url, detail } => {
                        media.push((image_url.clone(), *detail));
                        Some("[image]".to_string())
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            (role.clone(), "message".to_string(), None, None, text, media)
        }
        TranscriptItem::FunctionCall {
            name,
            namespace,
            arguments,
            ..
        } => (
            "assistant".to_string(),
            "function_call".to_string(),
            namespace.clone(),
            Some(name.clone()),
            arguments.clone(),
            Vec::new(),
        ),
        TranscriptItem::FunctionCallOutput { output, .. }
        | TranscriptItem::CustomToolCallOutput { output, .. } => {
            let mut media = Vec::new();
            if let FunctionCallOutputBody::ContentItems(items) = &output.body {
                for item in items {
                    if let FunctionCallOutputContentItem::InputImage { image_url, detail } = item {
                        media.push((image_url.clone(), *detail));
                    }
                }
            }
            (
                "tool".to_string(),
                "tool_output".to_string(),
                None,
                None,
                output.body.to_text().unwrap_or_default(),
                media,
            )
        }
        TranscriptItem::CustomToolCall { name, input, .. } => (
            "assistant".to_string(),
            "custom_tool_call".to_string(),
            None,
            Some(name.clone()),
            input.clone(),
            Vec::new(),
        ),
        TranscriptItem::Reasoning {
            summary, content, ..
        } => (
            "assistant".to_string(),
            "reasoning".to_string(),
            None,
            None,
            serde_json::to_string(&json!({"summary":summary,"content":content}))
                .unwrap_or_default(),
            Vec::new(),
        ),
        TranscriptItem::LocalCompaction { text } => (
            "developer".to_string(),
            "local_compaction".to_string(),
            None,
            None,
            text.clone(),
            Vec::new(),
        ),
        _ => (
            "assistant".to_string(),
            transcript_item_type(item).to_string(),
            None,
            None,
            serde_json::to_string(item).unwrap_or_default(),
            Vec::new(),
        ),
    }
}

fn transcript_item_type(item: &TranscriptItem) -> &'static str {
    match item {
        TranscriptItem::Message { .. } => "message",
        TranscriptItem::AgentMessage { .. } => "agent_message",
        TranscriptItem::Reasoning { .. } => "reasoning",
        TranscriptItem::LocalShellCall { .. } => "local_shell_call",
        TranscriptItem::FunctionCall { .. } => "function_call",
        TranscriptItem::ToolSearchCall { .. } => "tool_search_call",
        TranscriptItem::FunctionCallOutput { .. } => "function_call_output",
        TranscriptItem::CustomToolCall { .. } => "custom_tool_call",
        TranscriptItem::CustomToolCallOutput { .. } => "custom_tool_call_output",
        TranscriptItem::ToolSearchOutput { .. } => "tool_search_output",
        TranscriptItem::WebSearchCall { .. } => "web_search_call",
        TranscriptItem::ImageGenerationCall { .. } => "image_generation_call",
        TranscriptItem::LocalCompaction { .. } => "local_compaction",
        TranscriptItem::Compaction { .. } => "compaction",
        TranscriptItem::CompactionTrigger => "compaction_trigger",
        TranscriptItem::ContextCompaction { .. } => "context_compaction",
        TranscriptItem::Other => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::protocol::TranscriptIdentity;

    fn envelope(item_id: &str, text: &str) -> TranscriptEnvelope {
        TranscriptEnvelope {
            item: TranscriptItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: text.to_string(),
                }],
                phase: None,
            },
            identity: TranscriptIdentity {
                thread_id: "thread".to_string(),
                agent_path: None,
                window_id: "window".to_string(),
                window_number: 0,
                turn_id: Some("turn".to_string()),
                ordinal: 0,
                item_id: item_id.to_string(),
            },
        }
    }

    #[test]
    fn appended_envelope_is_immediately_readable() {
        let mut archive = HistoryArchive::default();

        archive.append_envelopes(&[envelope("item", "persisted")]);

        assert_eq!(archive.entries().len(), 1);
        assert_eq!(
            archive.get("window", "item").map(|entry| &*entry.content),
            Some("persisted")
        );
    }

    #[test]
    fn duplicate_item_id_updates_in_place() {
        let mut archive = HistoryArchive::default();
        archive.append_envelopes(&[envelope("item", "old")]);

        archive.append_envelopes(&[envelope("item", "new")]);

        assert_eq!(archive.entries().len(), 1);
        assert_eq!(
            archive.get("window", "item").map(|entry| &*entry.content),
            Some("new")
        );
    }
}
