//! Context-compaction lifecycle presentation for `ChatWidget`.
//!
//! The app-server owns compaction semantics. This module only retains the matching item lifecycle
//! long enough to show an authoritative running state and a terminal outcome in the TUI.

use super::*;

#[derive(Debug)]
enum CompactionState {
    Requested,
    Started { item_id: String, started_at_ms: i64 },
}

#[derive(Debug, Default)]
pub(super) struct CompactionLifecycle {
    state: Option<CompactionState>,
}

impl CompactionLifecycle {
    pub(super) fn is_active(&self) -> bool {
        self.state.is_some()
    }

    pub(super) fn request(&mut self) {
        self.state.get_or_insert(CompactionState::Requested);
    }

    pub(super) fn start(&mut self, item_id: String, started_at_ms: i64) {
        if self
            .state
            .as_ref()
            .is_some_and(|state| {
                matches!(state, CompactionState::Started { item_id: active_id, .. } if active_id == &item_id)
            })
        {
            return;
        }
        self.state = Some(CompactionState::Started {
            item_id,
            started_at_ms,
        });
    }

    pub(super) fn complete(&mut self, item_id: &str, completed_at_ms: i64) -> Option<u64> {
        match self.state.take()? {
            CompactionState::Requested => None,
            CompactionState::Started {
                item_id: active_id,
                started_at_ms,
            } => {
                if active_id != item_id {
                    tracing::warn!(
                        started_item_id = %active_id,
                        completed_item_id = %item_id,
                        "context compaction completed with a different item id"
                    );
                    return None;
                }
                completed_at_ms
                    .checked_sub(started_at_ms)
                    .and_then(|elapsed| u64::try_from(elapsed).ok())
            }
        }
    }

    pub(super) fn take_active(&mut self) -> bool {
        self.state.take().is_some()
    }

    pub(super) fn clear(&mut self) {
        self.state = None;
    }
}

impl ChatWidget {
    pub(super) fn on_context_compaction_requested(&mut self) {
        self.compaction_lifecycle.request();
        self.show_compaction_running();
    }

    pub(super) fn on_context_compaction_started(&mut self, id: String, started_at_ms: i64) {
        self.compaction_lifecycle.start(id, started_at_ms);
        self.show_compaction_running();
    }

    fn show_compaction_running(&mut self) {
        self.update_task_running_state();
        self.status_state.terminal_title_status_kind = TerminalTitleStatusKind::Working;
        self.set_status_header("Compacting…".to_string());
        self.request_redraw();
    }

    pub(super) fn on_context_compaction_completed(&mut self, id: String, completed_at_ms: i64) {
        let elapsed_ms = self.compaction_lifecycle.complete(&id, completed_at_ms);
        self.add_to_history(history_cell::new_compaction_completed(elapsed_ms));
        if self.turn_lifecycle.agent_turn_running {
            self.set_status_header("Working".to_string());
        }
        self.update_task_running_state();
        self.request_redraw();
    }

    pub(super) fn on_context_compaction_replayed(&mut self, _id: String) {
        self.add_to_history(history_cell::new_compaction_completed(
            /*elapsed_ms*/ None,
        ));
        self.request_redraw();
    }

    pub(super) fn compaction_failure_message(&mut self, message: String) -> String {
        if self.compaction_lifecycle.take_active() {
            compaction_failure_message(message)
        } else {
            message
        }
    }
}

pub(super) fn compaction_failure_message(message: String) -> String {
    let message = message.trim();
    if message.is_empty() {
        return "Compaction failed.".to_string();
    }
    if message
        .get(.."compaction".len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("compaction"))
    {
        return message.to_string();
    }
    format!("Compaction failed: {message}")
}
