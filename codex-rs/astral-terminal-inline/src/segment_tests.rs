// Derived from xai-ratatui-inline at grok-build commit
// 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Modified by Astral Code contributors for workspace integration.

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_empty_string() {
    let segments = split_into_line_segments("", 10);
    assert_eq!(segments.len(), 0);
}

#[test]
fn test_simple_text() {
    let input = "hello";
    let segments = split_into_line_segments(input, 10);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].content, "hello");
    assert!(!segments[0].ends_with_crlf);
}

#[test]
fn test_text_wrapping() {
    let input = "hello world";
    let segments = split_into_line_segments(input, 8);
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].content, "hello wo");
    assert!(!segments[0].ends_with_crlf);
    assert_eq!(segments[1].content, "rld");
    assert!(!segments[1].ends_with_crlf);
}

#[test]
fn test_newline_handling() {
    let input = "line1\nline2";
    let segments = split_into_line_segments(input, 20);
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].content, "line1");
    assert!(segments[0].ends_with_crlf);
    assert_eq!(segments[1].content, "line2");
    assert!(!segments[1].ends_with_crlf);
}

#[test]
fn test_crlf_handling() {
    let input = "line1\r\nline2\nline3";
    let segments = split_into_line_segments(input, 20);
    assert_eq!(segments.len(), 3);
    // First segment: "line1" (the \r\n is stripped)
    assert_eq!(segments[0].content, "line1");
    assert!(segments[0].ends_with_crlf);
    // Second segment: "line2"
    assert_eq!(segments[1].content, "line2");
    assert!(segments[1].ends_with_crlf);
    // Third segment: "line3"
    assert_eq!(segments[2].content, "line3");
    assert!(!segments[2].ends_with_crlf);
}

#[test]
fn test_bare_cr_resets_width() {
    // CR resets visual position, so "12345\r67" fits in width 10
    let input = "12345\r67";
    let segments = split_into_line_segments(input, 10);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].content, "12345\r67");
    assert!(!segments[0].ends_with_crlf);
}

#[test]
fn test_edge_case_char_wider_than_terminal() {
    // Emoji is 2 wide, terminal is 1 wide
    let input = "😊";
    let segments = split_into_line_segments(input, 1);
    // Should still create one segment even though it exceeds width
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].content, "😊");
}

#[test]
fn test_zero_width_segment_merging() {
    // Test merging of trailing zero-width content (no newline at end)
    let input = "line1\x1b[31m";
    let segments = split_into_line_segments(input, 20);
    // The color code should be in the same segment
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].content, "line1\x1b[31m");
    assert!(!segments[0].ends_with_crlf);

    // Test that ANSI after newline creates a separate segment
    let input2 = "line1\n\x1b[31m";
    let segments2 = split_into_line_segments(input2, 20);
    assert_eq!(segments2.len(), 2);
    assert_eq!(segments2[0].content, "line1");
    assert!(segments2[0].ends_with_crlf);
    assert_eq!(segments2[1].content, "\x1b[31m");
    assert!(!segments2[1].ends_with_crlf);
}

#[test]
fn test_multiple_ansi_codes() {
    let input = "\x1b[1m\x1b[31mBold Red\x1b[0m";
    let segments = split_into_line_segments(input, 20);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].content, input);
}

#[test]
fn test_wrap_at_exact_width() {
    let input = "12345678"; // exactly 8 chars
    let segments = split_into_line_segments(input, 8);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].content, "12345678");
}

#[test]
fn test_wrap_with_trailing_ansi() {
    // Text fills line, then ANSI codes
    let input = "12345678\x1b[0m90";
    let segments = split_into_line_segments(input, 8);
    assert_eq!(segments.len(), 2);
    // First segment gets the reset code since no visual content follows it on same line
    assert_eq!(segments[0].content, "12345678\x1b[0m");
    assert_eq!(segments[1].content, "90");
}

#[test]
fn test_cr_before_lf() {
    // Make sure \r right before \n is stripped
    let input = "test\r\n";
    let segments = split_into_line_segments(input, 10);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].content, "test");
    assert!(segments[0].ends_with_crlf);
}

#[test]
fn test_multiple_segments_with_ansi() {
    let input = "\x1b[32mline1\nline2\nline3\x1b[0m";
    let segments = split_into_line_segments(input, 20);
    assert_eq!(segments.len(), 3);

    assert!(segments[0].content.starts_with("\x1b[32m"));
    assert!(segments[0].ends_with_crlf);

    assert_eq!(segments[1].content, "line2");
    assert!(segments[1].ends_with_crlf);

    assert!(segments[2].content.ends_with("\x1b[0m"));
    assert!(!segments[2].ends_with_crlf);
}

#[test]
fn test_visual_width_calculation_with_unicode() {
    // "你好" is 4 visual width (2 per character)
    let input = "hello 你好";
    let segments = split_into_line_segments(input, 10);
    assert_eq!(segments.len(), 1); // "hello 你好" = 6 + 4 = 10, exactly fits

    let segments2 = split_into_line_segments(input, 9);
    assert_eq!(segments2.len(), 2); // Doesn't fit, must wrap
}
