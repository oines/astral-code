// Derived from xai-ratatui-inline at grok-build commit
// 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Modified by Astral Code contributors for workspace integration.

mod common;
mod resize;
mod scrollback;
mod segment;
mod terminal;

#[cfg(test)]
mod tests;

pub use self::common::TerminalLike;
pub use self::common::with_synchronized_output;
pub use self::resize::resize_purge_rerender;
pub use self::resize::resize_viewport_height;
pub use self::scrollback::emit_to_scrollback;
pub use self::segment::split_into_line_segments;
pub use self::terminal::LinkSpan;
pub use self::terminal::Terminal;
