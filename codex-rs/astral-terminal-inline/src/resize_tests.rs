// Derived from xai-ratatui-inline at grok-build commit
// 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Modified by Astral Code contributors for workspace integration.

use pretty_assertions::assert_eq;

use crate::tests::MockTerminal;

use super::*;

#[test]
fn test_viewport_resize_shrink() {
    let mut terminal = MockTerminal::new(80, 25, 5);
    let original_y = terminal.viewport_area.y; // Should be 20 (25-5)

    // Shrink viewport from 5 to 3 (always anchors at top)
    resize_viewport_height(&mut terminal, 3).unwrap();

    // Check viewport was updated - y should stay the same
    assert_eq!(terminal.viewport_area.height, 3);
    assert_eq!(terminal.viewport_area.y, original_y); // Should still be 20

    // Should have cleared once
    assert_eq!(terminal.clear_count, 1);
}

#[test]
fn test_viewport_resize_smart_expand() {
    let mut terminal = MockTerminal::new(80, 25, 3);

    // Start at position 20 (not at bottom)
    terminal.viewport_area.y = 20;

    // Expand viewport from 3 to 5 - should expand downward first
    resize_viewport_height(&mut terminal, 5).unwrap();

    // Check that it expanded down (kept same y)
    assert_eq!(terminal.viewport_area.height, 5);
    assert_eq!(terminal.viewport_area.y, 20); // Should stay at 20
    assert_eq!(terminal.clear_count, 1);

    // Now expand more - should hit bottom and push content up
    resize_viewport_height(&mut terminal, 6).unwrap();
    assert_eq!(terminal.viewport_area.height, 6);
    assert_eq!(terminal.viewport_area.y, 19); // Should move up to 19
    assert_eq!(terminal.clear_count, 2);
}

#[test]
fn test_viewport_resize_invalid() {
    let mut terminal = MockTerminal::new(80, 25, 3);

    // Try invalid heights
    assert!(resize_viewport_height(&mut terminal, 0).is_err());
    assert!(resize_viewport_height(&mut terminal, 25).is_err());
    assert!(resize_viewport_height(&mut terminal, 26).is_err());

    // Valid edge cases
    assert!(resize_viewport_height(&mut terminal, 1).is_ok());
    assert!(resize_viewport_height(&mut terminal, 24).is_ok());
}

#[test]
fn test_viewport_resize_no_op() {
    let mut terminal = MockTerminal::new(80, 25, 3);

    // Resize to same height
    resize_viewport_height(&mut terminal, 3).unwrap();

    // Should not have cleared
    assert_eq!(terminal.clear_count, 0);
    assert_eq!(terminal.viewport_area.height, 3);
}

#[test]
fn test_resize_purge_rerender_empty_history() {
    let mut terminal = MockTerminal::new(80, 25, 3);
    terminal.viewport_area.y = 22; // Bottom position

    // Test with empty history
    resize_purge_rerender(&mut terminal, "").unwrap();

    // Viewport should be at top since there's no content
    assert_eq!(terminal.viewport_area.y, 0);
    assert_eq!(terminal.viewport_area.height, 3);
    assert_eq!(terminal.clear_count, 1);
}

#[test]
fn test_resize_purge_rerender_small_history() {
    let mut terminal = MockTerminal::new(80, 25, 3);
    terminal.viewport_area.y = 22; // Bottom position

    // Test with small history (just a few lines)
    let history = "Line 1\r\nLine 2\r\nLine 3\r\n";
    resize_purge_rerender(&mut terminal, history).unwrap();

    // split_into_line_segments will count this as 3 segments (one per line)
    // So viewport should be positioned at y=3
    assert_eq!(terminal.viewport_area.y, 3);
    assert_eq!(terminal.viewport_area.height, 3);
    assert_eq!(terminal.clear_count, 1);
}

#[test]
fn test_resize_purge_rerender_full_screen_history() {
    let mut terminal = MockTerminal::new(80, 25, 3);
    terminal.viewport_area.y = 22; // Bottom position

    // Create history with more lines than screen height
    let mut history = String::new();
    for i in 1..=30 {
        history.push_str(&format!("Line {i}\r\n"));
    }

    resize_purge_rerender(&mut terminal, &history).unwrap();

    // With full screen of content, viewport should be at bottom
    assert_eq!(terminal.viewport_area.y, 25 - 3); // screen_height - viewport_height
    assert_eq!(terminal.viewport_area.height, 3);
    assert_eq!(terminal.clear_count, 1);
}

#[test]
fn test_resize_purge_rerender_with_wrapped_lines() {
    let mut terminal = MockTerminal::new(40, 10, 2); // Narrow terminal
    terminal.viewport_area.y = 8;

    // Create a line that will wrap
    let long_line = "A".repeat(100); // Will wrap to ~3 lines on 40-column terminal
    let history = format!("{long_line}\r\nShort line\r\n");

    resize_purge_rerender(&mut terminal, &history).unwrap();

    // The actual position depends on split_into_line_segments calculation
    // But it should position the viewport appropriately
    assert!(terminal.viewport_area.y <= 10 - 2);
    assert_eq!(terminal.viewport_area.height, 2);
    assert_eq!(terminal.clear_count, 1);
}

#[test]
fn test_resize_purge_rerender_preserves_viewport_dimensions() {
    let mut terminal = MockTerminal::new(100, 30, 5);
    let original_width = terminal.viewport_area.width;
    let original_height = terminal.viewport_area.height;

    let history = "Some content\r\n";
    resize_purge_rerender(&mut terminal, history).unwrap();

    // Width and height should be preserved, only y position changes
    assert_eq!(terminal.viewport_area.width, original_width);
    assert_eq!(terminal.viewport_area.height, original_height);
}

#[test]
fn test_resize_purge_rerender_captures_output() {
    let mut terminal = MockTerminal::new(80, 25, 3);

    let history = "Test line\r\n";
    resize_purge_rerender(&mut terminal, history).unwrap();

    // Verify RIS command was sent to writer (not real stdout)
    let output = String::from_utf8_lossy(&terminal.writer.buffer);
    assert!(
        output.contains("\x1b[2J\x1b[3J\x1b[H"),
        "Should contain reset commands"
    );
    assert!(output.contains("Test line"), "Should contain history");

    // Ensure we flushed the writer
    assert!(
        terminal.writer.flush_count > 0,
        "Should have flushed writer"
    );
}
