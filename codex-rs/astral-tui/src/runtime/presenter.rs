//! Frame scheduling for the Astral runtime loop.
//!
//! State updates request presentation without drawing synchronously. The runtime
//! loop waits for the resulting deadline alongside terminal and app-server
//! events, so bursts of streamed deltas collapse into one terminal frame.

use std::time::Duration;
use std::time::Instant;

const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Presenter {
    dirty: bool,
    last_presented_at: Option<Instant>,
    next_frame_at: Option<Instant>,
}

#[cfg(test)]
#[path = "presenter_tests.rs"]
mod tests;

impl Presenter {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            dirty: true,
            last_presented_at: None,
            next_frame_at: Some(now),
        }
    }

    pub(super) fn request(&mut self, now: Instant) {
        self.dirty = true;
        let requested_at = self
            .last_presented_at
            .and_then(|last| last.checked_add(MIN_FRAME_INTERVAL))
            .map_or(now, |earliest| now.max(earliest));
        self.next_frame_at = Some(
            self.next_frame_at
                .map_or(requested_at, |scheduled| scheduled.min(requested_at)),
        );
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        if self.dirty { self.next_frame_at } else { None }
    }

    pub(super) fn mark_presented(&mut self, now: Instant) {
        self.dirty = false;
        self.last_presented_at = Some(now);
        self.next_frame_at = None;
    }
}
