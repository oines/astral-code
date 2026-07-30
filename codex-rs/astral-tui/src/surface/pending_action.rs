use std::time::Instant;

use crossterm::event::KeyEvent;

use super::SurfaceState;
use crate::actions;
use crate::actions::ActionId;
use crate::pending_action::PendingActionState;

impl SurfaceState {
    pub(crate) fn arm_pending_action(&mut self, action: ActionId) {
        self.pending_action = Some(PendingActionState::new(action));
    }

    pub(crate) fn consume_pending_action(&mut self, key: &KeyEvent) -> Option<ActionId> {
        let pending = self.pending_action.take()?;
        (!pending.expired(Instant::now()) && actions::matches(pending.action, key))
            .then_some(pending.action)
    }

    pub(crate) fn pending_action(&self) -> Option<ActionId> {
        self.pending_action.as_ref().map(|pending| pending.action)
    }

    pub(crate) fn pending_action_deadline(&self) -> Option<Instant> {
        self.pending_action
            .as_ref()
            .map(|pending| pending.expires_at)
    }

    pub(crate) fn expire_pending_action(&mut self, now: Instant) -> bool {
        if !self
            .pending_action
            .as_ref()
            .is_some_and(|pending| pending.expired(now))
        {
            return false;
        }
        self.pending_action = None;
        true
    }
}
