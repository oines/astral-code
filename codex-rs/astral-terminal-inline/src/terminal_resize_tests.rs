// Derived from xai-ratatui-inline at grok-build commit
// 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Modified by Astral Code contributors for workspace integration.

use pretty_assertions::assert_eq;

use ratatui::TerminalOptions;
use ratatui::Viewport;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

use super::Terminal;

fn full_height_inline(width: u16, height: u16) -> Terminal<TestBackend> {
    Terminal::with_options(
        TestBackend::new(width, height),
        TerminalOptions {
            viewport: Viewport::Inline(height),
        },
    )
    .unwrap()
}

/// A full-height inline viewport (the alt-screen-unavailable case used under
/// Zellij / tmux control mode / `--no-alt-screen`) must GROW to fill the
/// terminal when it is enlarged.
///
/// Regression test for the bug where the viewport height was clamped to the
/// startup height (truncated at the bottom) while the width still tracked the
/// resize.
#[test]
fn inline_full_height_grows_with_terminal() {
    let mut terminal = full_height_inline(80, 24);
    assert_eq!(terminal.viewport_area(), Rect::new(0, 0, 80, 24));

    terminal.backend_mut().resize(80, 40);
    terminal.autoresize().unwrap();

    // Both dimensions track the new terminal size, anchored at the top.
    assert_eq!(terminal.viewport_area(), Rect::new(0, 0, 80, 40));
}

/// Width-only growth keeps working (this part was never broken).
#[test]
fn inline_full_height_grows_in_width() {
    let mut terminal = full_height_inline(80, 24);

    terminal.backend_mut().resize(120, 24);
    terminal.autoresize().unwrap();

    assert_eq!(terminal.viewport_area(), Rect::new(0, 0, 120, 24));
}

/// Shrinking must also track the terminal and must not position the viewport
/// off-screen (which previously panicked the strict `TestBackend` buffer and
/// would leave a real terminal's UI invisible/garbled).
#[test]
fn inline_full_height_shrinks_with_terminal() {
    let mut terminal = full_height_inline(80, 40);

    terminal.backend_mut().resize(80, 20);
    terminal.autoresize().unwrap();

    assert_eq!(terminal.viewport_area(), Rect::new(0, 0, 80, 20));
}

/// Growth after a shrink must expand again — the viewport tracks the live
/// terminal size in both directions, repeatedly.
#[test]
fn inline_full_height_tracks_across_shrink_then_grow() {
    let mut terminal = full_height_inline(80, 30);

    terminal.backend_mut().resize(80, 10);
    terminal.autoresize().unwrap();
    assert_eq!(terminal.viewport_area(), Rect::new(0, 0, 80, 10));

    terminal.backend_mut().resize(100, 50);
    terminal.autoresize().unwrap();
    assert_eq!(terminal.viewport_area(), Rect::new(0, 0, 100, 50));
}

/// A *small* inline viewport (height < terminal height, anchored near the
/// bottom) must NOT be forced to full height — it keeps the standard
/// `compute_inline_size` behavior, so the full-height special-case does not
/// over-apply.
#[test]
fn small_inline_viewport_is_not_forced_full() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Inline(3),
        },
    )
    .unwrap();
    assert_eq!(terminal.viewport_area().height, 3);

    terminal.backend_mut().resize(120, 40);
    terminal.autoresize().unwrap();

    // The full-height special-case keys off the viewport spanning the whole
    // terminal (height >= terminal height). A small inline viewport does not,
    // so its height stays clamped to the small inline target while the width
    // tracks the resize — i.e. it keeps the standard `compute_inline_size`
    // behavior and is not ballooned to full height.
    assert_eq!(terminal.viewport_area().height, 3);
    assert_eq!(terminal.viewport_area().width, 120);
}

/// Fullscreen viewports already track the full size; behavior is unchanged.
#[test]
fn fullscreen_tracks_terminal() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    assert_eq!(terminal.viewport_area(), Rect::new(0, 0, 80, 24));

    terminal.backend_mut().resize(80, 40);
    terminal.autoresize().unwrap();

    assert_eq!(terminal.viewport_area(), Rect::new(0, 0, 80, 40));
}
