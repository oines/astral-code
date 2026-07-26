//! Astral's terminal user interface.
//!
//! The crate treats app-server v2 items as the authoritative runtime model.
//! UI-specific state is limited to ordering, streamed deltas, and presentation
//! state; it does not emulate Grok's ACP payloads or duplicate Astral runtime
//! semantics.

mod request;
mod session;
mod timeline;

pub use astral_tui_scrollback::PresentationBlock;
pub use astral_tui_scrollback::RenderOptions;
pub use astral_tui_scrollback::TimelineStream;
pub use astral_tui_scrollback::ToolKind;
pub use astral_tui_scrollback::ToolPresentation;
pub use astral_tui_scrollback::ToolStatus;
pub use astral_tui_scrollback::render_block;
pub use request::PendingRequest;
pub use request::PendingRequestError;
pub use request::PendingRequestResponse;
pub use request::PendingRequests;
pub use request::RequestResolution;
pub use session::AstralSession;
pub use session::SessionError;
pub use session::SessionState;
pub use timeline::ReduceOutcome;
pub use timeline::TimelineEntry;
pub use timeline::TimelineState;
