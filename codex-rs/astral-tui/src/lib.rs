//! Astral's terminal user interface runtime.
//!
//! The runtime treats app-server v2 as authoritative. It does not own model,
//! tool, transcript, or rollout semantics; UI state is layered over the typed
//! app-server session and event stream.

mod block_viewer;
mod conversation;
mod fullscreen;
mod inline;
mod interactions;
mod modal;
mod plan_implementation;
mod prompt_interaction;
mod runtime;
mod session;
mod surface;
mod surface_renderer;
mod viewport;

pub use astral_tui_scrollback::DisplayMode;
pub use astral_tui_scrollback::EntryLifecycle;
pub use astral_tui_scrollback::LineJoiner;
pub use astral_tui_scrollback::MarkdownLine;
pub use astral_tui_scrollback::MarkdownLink;
pub use astral_tui_scrollback::TranscriptEntryId;
pub use block_viewer::BlockViewerHost;
pub use block_viewer::BlockViewerOutcome;
pub use conversation::ConversationState;
pub use conversation::EntryDisplayAction;
pub use conversation::VerbGroupDisplayAction;
pub use fullscreen::FullscreenHost;
pub use fullscreen::FullscreenOutcome;
pub use fullscreen::ScrollbackKeyMode;
pub use inline::InlineCommitResult;
pub use inline::InlineHost;
pub use interactions::PendingInteraction;
pub use interactions::PendingInteractionError;
pub use interactions::PendingInteractionKind;
pub use interactions::PendingInteractionStatus;
pub use interactions::PendingInteractionUpdate;
pub use interactions::PendingInteractions;
pub use modal::ModalContentArea;
pub use modal::ModalOutcome;
pub use modal::ModalPresentation;
pub use modal::ModalRenderStyle;
pub use modal::ModalShortcut;
pub use modal::ModalSizing;
pub use modal::ModalWindow;
pub use modal::ModalWindowConfig;
pub use plan_implementation::PlanImplementationHost;
pub use plan_implementation::PlanImplementationOutcome;
pub use plan_implementation::PlanImplementationRequest;
pub use plan_implementation::PlanImplementationSelection;
pub use prompt_interaction::PromptInteractionHost;
pub use prompt_interaction::PromptInteractionOutcome;
pub use prompt_interaction::PromptInteractionSubmission;
pub use runtime::AstralRuntime;
pub use runtime::RuntimeError;
pub use runtime::RuntimeEvent;
pub use runtime::TranscriptUpdate;
pub use session::AstralSession;
pub use session::SessionError;
pub use session::SessionState;
pub use surface::ConversationSurface;
pub use surface::MaterializedSurfaceEntry;
pub use surface::SurfaceAnchor;
pub use surface::SurfaceEntryPresentation;
pub use surface::SurfaceNode;
pub use surface::SurfaceNodeId;
pub use surface::SurfaceNodeKind;
pub use surface_renderer::SurfaceRenderStyle;
pub use surface_renderer::SurfaceRenderer;
pub use viewport::ScrollDirection;
pub use viewport::SurfaceViewport;
