use super::session::Session;
use super::turn_context::TurnContext;
use crate::state::AutoCompactWindowIds;
use codex_features::Feature;
use codex_protocol::config_types::AutoCompactTokenLimitScope;
use codex_protocol::models::TranscriptItem;
use codex_protocol::protocol::CompactedItem;
use codex_protocol::protocol::RolloutItem;
use std::collections::HashSet;
use uuid::Uuid;

pub(crate) const NEW_CONTEXT_WINDOW_MESSAGE: &str =
    "A new context window will start without summarizing conversation history.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContextWindowTokenStatus {
    pub(crate) active_context_tokens: i64,
    pub(crate) auto_compact_scope_tokens: i64,
    pub(crate) auto_compact_scope_limit: Option<i64>,
    pub(crate) full_context_window_limit: Option<i64>,
    pub(crate) base_window_tokens_remaining: Option<i64>,
    pub(crate) auto_compact_window_ordinal: Option<u64>,
    pub(crate) auto_compact_window_prefill_tokens: Option<i64>,
    pub(crate) full_context_window_limit_reached: bool,
    pub(crate) token_limit_reached: bool,
}

fn tokens_remaining(limit: Option<i64>, used: i64) -> Option<i64> {
    limit.map(|limit| limit.saturating_sub(used).max(0))
}

fn buffered_auto_compact_limit(
    limit: Option<i64>,
    context_management_enabled: bool,
) -> Option<i64> {
    let reserve = if context_management_enabled {
        super::token_budget::CHECKPOINT_RESERVE_TOKENS
    } else {
        0
    };
    limit.map(|limit| limit.saturating_add(reserve))
}

pub(crate) fn stable_legacy_window_id(scope: &str, items: &[RolloutItem]) -> Uuid {
    let canonical = items
        .iter()
        .find_map(|item| match item {
            RolloutItem::TranscriptItem(item) => serde_json::to_string(item).ok(),
            RolloutItem::TranscriptEnvelope(envelope) => Some(envelope.identity.window_id.clone()),
            RolloutItem::Compacted(compacted) => serde_json::to_string(compacted).ok(),
            _ => None,
        })
        .unwrap_or_else(|| "empty".to_string());
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("astral-window:{scope}:{canonical}").as_bytes(),
    )
}

pub(crate) fn advance_window_lineage(
    window_number: u64,
    ids: AutoCompactWindowIds,
    compacted: &CompactedItem,
    fallback_item: &RolloutItem,
) -> (u64, AutoCompactWindowIds) {
    let has_window_lineage = compacted.window_number.is_some()
        || compacted.first_window_id.is_some()
        || compacted.previous_window_id.is_some()
        || compacted.window_id.is_some();
    if !has_window_lineage {
        return (window_number, ids);
    }
    let next_window_number = compacted
        .window_number
        .unwrap_or_else(|| window_number.saturating_add(1));
    let first_window_id = compacted
        .first_window_id
        .as_deref()
        .and_then(|value| Uuid::parse_str(value).ok())
        .unwrap_or(ids.first_window_id);
    let previous_window_id = compacted
        .previous_window_id
        .as_deref()
        .and_then(|value| Uuid::parse_str(value).ok())
        .or(Some(ids.window_id));
    let window_id = compacted
        .window_id
        .as_deref()
        .and_then(|value| Uuid::parse_str(value).ok())
        .unwrap_or_else(|| {
            stable_legacy_window_id(
                &format!("compaction:{next_window_number}"),
                std::slice::from_ref(fallback_item),
            )
        });
    (
        next_window_number,
        AutoCompactWindowIds {
            first_window_id,
            previous_window_id,
            window_id,
        },
    )
}

pub(crate) fn infer_initial_window_lineage(
    items: &[RolloutItem],
) -> Option<(u64, AutoCompactWindowIds)> {
    let has_history = items.iter().any(|item| {
        matches!(
            item,
            RolloutItem::TranscriptItem(_)
                | RolloutItem::TranscriptEnvelope(_)
                | RolloutItem::Compacted(_)
        )
    });
    if !has_history {
        return None;
    }

    let first_window_id = items
        .iter()
        .find_map(|item| match item {
            RolloutItem::Compacted(compacted) => compacted
                .first_window_id
                .as_deref()
                .and_then(|value| Uuid::parse_str(value).ok()),
            _ => None,
        })
        .or_else(|| {
            items.iter().find_map(|item| match item {
                RolloutItem::TranscriptEnvelope(envelope)
                    if envelope.identity.window_number == 0 =>
                {
                    Uuid::parse_str(&envelope.identity.window_id).ok()
                }
                _ => None,
            })
        })
        .unwrap_or_else(|| stable_legacy_window_id("initial", items));
    Some((
        0,
        AutoCompactWindowIds {
            first_window_id,
            previous_window_id: None,
            window_id: first_window_id,
        },
    ))
}

pub(crate) fn infer_window_lineage(items: &[RolloutItem]) -> Option<(u64, AutoCompactWindowIds)> {
    let (mut window_number, mut ids) = infer_initial_window_lineage(items)?;
    for item in items {
        if let RolloutItem::Compacted(compacted) = item {
            (window_number, ids) = advance_window_lineage(window_number, ids, compacted, item);
        }
    }
    Some((window_number, ids))
}

/// Recovers the narrow crash window after `new_context` completed but before the runtime
/// installed its replacement-history checkpoint. A successful tool output is required so a
/// merely emitted (but never executed) function call does not trigger a reset on resume.
pub(crate) fn infer_pending_new_context(items: &[TranscriptItem]) -> bool {
    let mut requested_call_ids = HashSet::new();
    for item in items {
        match item {
            TranscriptItem::FunctionCall {
                name,
                namespace,
                call_id,
                ..
            } if name == "new_context" && namespace.is_none() => {
                requested_call_ids.insert(call_id.as_str());
            }
            TranscriptItem::FunctionCallOutput { call_id, output }
                if requested_call_ids.contains(call_id.as_str())
                    && output.body.to_text().as_deref() == Some(NEW_CONTEXT_WINDOW_MESSAGE) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

pub(crate) async fn context_window_token_status(
    sess: &Session,
    turn_context: &TurnContext,
) -> ContextWindowTokenStatus {
    let active_context_tokens = sess.get_total_token_usage().await;
    let mut auto_compact_window_ordinal = None;
    let mut auto_compact_window_prefill_tokens = None;

    let (auto_compact_scope_tokens, auto_compact_scope_limit) =
        match turn_context.config.model_auto_compact_token_limit_scope {
            AutoCompactTokenLimitScope::Total => (
                active_context_tokens,
                turn_context
                    .config
                    .model_auto_compact_token_limit
                    .or_else(|| turn_context.model_info.auto_compact_token_limit()),
            ),
            AutoCompactTokenLimitScope::BodyAfterPrefix => {
                let window = sess.auto_compact_window_snapshot().await;
                auto_compact_window_ordinal = Some(window.ordinal);
                auto_compact_window_prefill_tokens = window.prefill_input_tokens;
                let baseline = window.prefill_input_tokens.unwrap_or(active_context_tokens);
                (
                    active_context_tokens.saturating_sub(baseline),
                    turn_context
                        .config
                        .model_auto_compact_token_limit
                        .or_else(|| turn_context.model_info.auto_compact_token_limit()),
                )
            }
        };

    let full_context_window_limit = turn_context.model_context_window();
    let base_window_tokens_remaining = [
        tokens_remaining(auto_compact_scope_limit, auto_compact_scope_tokens),
        tokens_remaining(full_context_window_limit, active_context_tokens),
    ]
    .into_iter()
    .flatten()
    .min();
    let full_context_window_limit_reached =
        full_context_window_limit.is_some_and(|limit| active_context_tokens >= limit);
    let context_management_enabled = turn_context
        .config
        .features
        .enabled(Feature::ContextManagement);
    let buffered_auto_compact_limit =
        buffered_auto_compact_limit(auto_compact_scope_limit, context_management_enabled);
    let token_limit_reached = buffered_auto_compact_limit
        .is_some_and(|limit| auto_compact_scope_tokens >= limit)
        || full_context_window_limit_reached;

    ContextWindowTokenStatus {
        active_context_tokens,
        auto_compact_scope_tokens,
        auto_compact_scope_limit,
        full_context_window_limit,
        base_window_tokens_remaining,
        auto_compact_window_ordinal,
        auto_compact_window_prefill_tokens,
        full_context_window_limit_reached,
        token_limit_reached,
    }
}

#[cfg(test)]
mod tests {
    use super::NEW_CONTEXT_WINDOW_MESSAGE;
    use super::buffered_auto_compact_limit;
    use super::infer_pending_new_context;
    use codex_protocol::models::FunctionCallOutputPayload;
    use codex_protocol::models::TranscriptItem;

    #[test]
    fn context_management_reserves_checkpoint_budget() {
        assert_eq!(
            buffered_auto_compact_limit(Some(100_000), true),
            Some(116_384)
        );
        assert_eq!(
            buffered_auto_compact_limit(Some(100_000), false),
            Some(100_000)
        );
        assert_eq!(buffered_auto_compact_limit(None, true), None);
    }

    #[test]
    fn pending_new_context_requires_a_successful_tool_output() {
        let call = TranscriptItem::FunctionCall {
            id: None,
            name: "new_context".to_string(),
            namespace: None,
            arguments: "{}".to_string(),
            call_id: "reset-call".to_string(),
        };
        assert!(!infer_pending_new_context(std::slice::from_ref(&call)));
        assert!(!infer_pending_new_context(&[
            call.clone(),
            TranscriptItem::FunctionCallOutput {
                call_id: "different-call".to_string(),
                output: FunctionCallOutputPayload::from_text(
                    NEW_CONTEXT_WINDOW_MESSAGE.to_string(),
                ),
            },
        ]));
        assert!(infer_pending_new_context(&[
            call,
            TranscriptItem::FunctionCallOutput {
                call_id: "reset-call".to_string(),
                output: FunctionCallOutputPayload::from_text(
                    NEW_CONTEXT_WINDOW_MESSAGE.to_string(),
                ),
            },
        ]));
    }
}
