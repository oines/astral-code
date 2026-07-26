//! Astral's terminal user interface.
//!
//! The crate treats app-server v2 items as the authoritative runtime model.
//! UI-specific state is limited to ordering, streamed deltas, and presentation
//! state; it does not emulate Grok's ACP payloads or duplicate Astral runtime
//! semantics.

mod client_tools;
mod conversation;
mod input;
mod launch;
mod request;
mod runtime;
mod session;
mod surface;
mod timeline;

pub use astral_tui_scrollback::PresentationBlock;
pub use astral_tui_scrollback::RenderOptions;
pub use astral_tui_scrollback::TimelineStream;
pub use astral_tui_scrollback::ToolKind;
pub use astral_tui_scrollback::ToolPresentation;
pub use astral_tui_scrollback::ToolStatus;
pub use astral_tui_scrollback::render_block;
pub use client_tools::ClientToolError;
pub use client_tools::ClientToolRegistry;
pub use conversation::CommittedBlock;
pub use conversation::ConversationState;
pub use input::InputAction;
pub use input::handle_key;
pub use input::handle_paste;
pub use launch::LaunchError;
pub use launch::LaunchOptions;
pub use launch::ThreadLaunch;
pub use launch::run_main;
pub use request::PendingRequest;
pub use request::PendingRequestError;
pub use request::PendingRequestResponse;
pub use request::PendingRequests;
pub use request::RequestResolution;
pub use runtime::RunError;
pub use runtime::RunExit;
pub use runtime::RunExitReason;
pub use runtime::RunOptions;
pub use runtime::run;
pub use session::AstralSession;
pub use session::SessionError;
pub use session::SessionState;
pub use surface::SurfaceActivity;
pub use surface::SurfaceState;
pub use surface::committed_height;
pub use surface::paint_committed;
pub use surface::render_surface;
pub use timeline::ReduceOutcome;
pub use timeline::TimelineEntry;
pub use timeline::TimelineState;
