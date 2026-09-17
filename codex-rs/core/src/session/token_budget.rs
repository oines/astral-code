use super::session::Session;
use super::turn_context::TurnContext;
use codex_features::Feature;
use codex_protocol::models::ContentItem;
use codex_protocol::models::TranscriptItem;

const DEFAULT_REMINDER_THRESHOLD_TOKENS: i64 = 6_144;
pub(crate) const CHECKPOINT_RESERVE_TOKENS: i64 = 16_384;

pub(crate) async fn maybe_record(
    sess: &Session,
    turn_context: &TurnContext,
    base_window_tokens_remaining: Option<i64>,
) {
    if !turn_context
        .config
        .features
        .enabled(Feature::ContextManagement)
    {
        return;
    }
    let Some(tokens_remaining) = base_window_tokens_remaining else {
        return;
    };
    if tokens_remaining == 0 {
        let fallback_due = {
            let mut state = sess.state.lock().await;
            state.claim_auto_compact_fallback()
        };
        if fallback_due {
            let fallback = TranscriptItem::Message {
                id: None,
                role: "developer".to_string(),
                content: vec![ContentItem::InputText {
                    text: format!(
                        "<context_window_reminder>\nThe current context window is exhausted. Do not continue the task or give a final answer in this window. You are using a bounded {CHECKPOINT_RESERVE_TOKENS}-token checkpoint reserve. The next window will not automatically include this conversation. Make exactly one write or append call to `notes` now to save a concise checkpoint with the goal, decisions, progress, learnings, next steps, and the window ID and item ID of every relevant user request still being solved, as well as important actions and tool calls for future reference. Every non-assistant item, such as user, developer, and tool response items, has an item ID `[id: ...]` immediately after its content. After the notes result returns, call `new_context`; do not use any tools other than `notes` and `new_context`.\n</context_window_reminder>"
                    ),
                }],
                phase: None,
            };
            sess.record_conversation_items(turn_context, std::slice::from_ref(&fallback))
                .await;
        }
        return;
    }
    if tokens_remaining > DEFAULT_REMINDER_THRESHOLD_TOKENS {
        return;
    }

    let reminder_due = {
        let mut state = sess.state.lock().await;
        state.claim_token_budget_reminder()
    };
    if !reminder_due {
        return;
    }

    let reminder = TranscriptItem::Message {
        id: None,
        role: "developer".to_string(),
        content: vec![ContentItem::InputText {
            text: format!(
                "<context_window_reminder>\nYour current context window is nearly exhausted; only {tokens_remaining} weighted tokens remain. Before starting a new context window, save concise progress notes with the `notes` tool covering the goal, decisions, progress, learnings, and next steps. Include the current window ID and the `[id: ...]` item ID of every relevant user request still being solved, along with important actions and tool calls needed for recovery. Write or append notes in the form most useful for resuming work, and clean up obsolete notes when appropriate. Future context windows will not automatically include the current conversation. After saving your state, call `new_context` to continue in a fresh context window.\n</context_window_reminder>"
            ),
        }],
        phase: None,
    };
    sess.record_conversation_items(turn_context, std::slice::from_ref(&reminder))
        .await;
}
