//! Astral's terminal user interface runtime.
//!
//! The runtime treats app-server v2 as authoritative. It does not own model,
//! tool, transcript, or rollout semantics; UI state is layered over the typed
//! app-server session and event stream.

mod conversation;
mod runtime;
mod session;
mod surface;
mod viewport;

pub use conversation::ConversationState;
pub use conversation::EntryDisplayAction;
pub use conversation::VerbGroupDisplayAction;
pub use runtime::AstralRuntime;
pub use runtime::RuntimeError;
pub use runtime::RuntimeEvent;
pub use runtime::TranscriptUpdate;
pub use session::AstralSession;
pub use session::SessionError;
pub use session::SessionState;
pub use surface::ConversationSurface;
pub use surface::SurfaceAnchor;
pub use surface::SurfaceNode;
pub use surface::SurfaceNodeId;
pub use surface::SurfaceNodeKind;
pub use viewport::ScrollDirection;
pub use viewport::SurfaceViewport;
