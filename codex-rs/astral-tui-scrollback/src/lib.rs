//! Canonical Astral transcript state that preserves app-server `ThreadItem`
//! values instead of flattening them into renderer-specific text.

mod block;
mod display;
mod live_item;
mod markdown;
mod transcript;

pub use block::EntryBlock;
pub use block::ReasoningBlock;
pub use block::ReasoningVisibility;
pub use display::DisplayMode;
pub use display::EntryDisplayState;
pub use live_item::LiveItem;
pub use markdown::CodeLineHighlighter;
pub use markdown::LineJoiner;
pub use markdown::MarkdownLine;
pub use markdown::MarkdownLink;
pub use markdown::MarkdownStyle;
pub use markdown::MarkdownSyntaxTheme;
pub use markdown::MarkdownTable;
pub use markdown::MarkdownTableAlignment;
pub use markdown::highlight_fenced_code;
pub use markdown::render_literal_with_metadata;
pub use markdown::render_markdown;
pub use markdown::render_markdown_with_metadata;
pub use markdown::wrap_styled_line_with_metadata;
pub use transcript::ApplyOutcome;
pub use transcript::EntryLifecycle;
pub use transcript::Transcript;
pub use transcript::TranscriptEntry;
pub use transcript::TranscriptEntryId;
pub use transcript::TranscriptGap;
pub use transcript::TranscriptTurn;
