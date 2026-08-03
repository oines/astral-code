//! Astral's terminal user interface runtime.
//!
//! The runtime treats app-server v2 as authoritative. It does not own model,
//! tool, transcript, or rollout semantics; UI state is layered over the typed
//! app-server session and event stream.

mod session;

pub use session::AstralSession;
pub use session::SessionError;
pub use session::SessionState;
