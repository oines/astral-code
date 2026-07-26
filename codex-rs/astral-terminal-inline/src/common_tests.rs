// Derived from xai-ratatui-inline at grok-build commit
// 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Modified by Astral Code contributors for workspace integration.

use pretty_assertions::assert_eq;

use std::io::Write;

use crate::tests::MockTerminal;

use super::*;

#[test]
fn test_synchronized_output() {
    let mut terminal = MockTerminal::new(80, 25, 3);

    // Use synchronized output wrapper
    let result = with_synchronized_output(&mut terminal, |terminal| {
        _ = terminal.writer_mut().write(b"Test content")?;
        terminal.writer_mut().flush()?;
        Ok(())
    });

    assert!(result.is_ok());

    // Check that synchronized output markers were written
    let buffer = &terminal.writer.buffer;
    let text = String::from_utf8_lossy(buffer);

    // Should contain begin and end synchronized update sequences
    assert!(
        text.contains("\x1b[?2026h"),
        "Should have begin synchronized update"
    );
    assert!(
        text.contains("\x1b[?2026l"),
        "Should have end synchronized update"
    );

    // Content should be between the markers
    assert!(text.contains("Test content"));

    // Should have flushed (once in emit_to_scrollback, once in with_synchronized_output)
    assert_eq!(terminal.writer.flush_count, 2);
}
