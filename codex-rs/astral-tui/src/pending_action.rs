//! Short-lived confirmation state for destructive TUI shortcuts.

use std::time::Duration;
use std::time::Instant;

use crate::actions::ActionId;

#[derive(Debug)]
pub(crate) struct PendingActionState {
    pub(crate) action: ActionId,
    pub(crate) expires_at: Instant,
}

impl PendingActionState {
    pub(crate) const TTL: Duration = Duration::from_secs(1);

    pub(crate) fn new(action: ActionId) -> Self {
        Self {
            action,
            expires_at: Instant::now() + Self::TTL,
        }
    }

    pub(crate) fn expired(&self, now: Instant) -> bool {
        now >= self.expires_at
    }
}
