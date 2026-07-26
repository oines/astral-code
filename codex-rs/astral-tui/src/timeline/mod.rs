//! Ordered transcript state derived from app-server notifications.
//!
//! Completed [`ThreadItem`](codex_app_server_protocol::ThreadItem) values stay
//! authoritative. The extra state here only preserves deltas when a
//! best-effort `item/started` notification is missing.

mod reducer;

pub use reducer::ReduceOutcome;
pub use reducer::TimelineEntry;
pub use reducer::TimelineState;

#[cfg(test)]
#[path = "reducer_tests.rs"]
mod tests;
