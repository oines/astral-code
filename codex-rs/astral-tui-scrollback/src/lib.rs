//! Canonical Astral transcript state that preserves app-server `ThreadItem`
//! values instead of flattening them into renderer-specific text.

mod live_item;
mod transcript;

pub use live_item::LiveItem;
pub use transcript::ApplyOutcome;
pub use transcript::EntryLifecycle;
pub use transcript::Transcript;
pub use transcript::TranscriptEntry;
pub use transcript::TranscriptEntryId;
pub use transcript::TranscriptGap;
pub use transcript::TranscriptTurn;
