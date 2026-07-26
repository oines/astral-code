// Derived from xai-ratatui-inline at grok-build commit
// 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Modified by Astral Code contributors for workspace integration.

use std::fmt;

use anstyle_parse::DefaultCharAccumulator;
use anstyle_parse::Params;
use anstyle_parse::Parser;
use anstyle_parse::Perform;
use unicode_width::UnicodeWidthChar as _;

/// Represents a line segment (physical row) with its content and ANSI state
#[derive(Debug, Clone)]
pub struct LineSegment<'a> {
    /// Contiguous string content
    pub content: &'a str,
    /// Has a trailing crlf at the end of it
    pub ends_with_crlf: bool,
}

impl fmt::Display for LineSegment<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // To write without crlf, can simply write segment.content
        write!(f, "{}", self.content)?;
        if self.ends_with_crlf {
            write!(f, "\r\n")?;
        }
        Ok(())
    }
}

/// The parse events `split_into_line_segments` distinguishes. Everything the
/// splitter cares about: printable characters (visual width), CR, LF; every
/// other action (SGR colors, cursor moves, OSC, …) merely extends the current
/// segment byte range.
enum SegmentEvent {
    Print(char),
    CarriageReturn,
    LineFeed,
    /// Any other complete escape/control action.
    Other,
}

/// `anstyle_parse::Perform` implementor that records the single event (if
/// any) produced by the byte just fed to the parser.
///
/// The VTE state machine dispatches at most one action per input byte, so a
/// one-slot buffer is sufficient. Print events are dispatched on the *final*
/// byte of a UTF-8 sequence; the char itself tells us how many bytes it spans.
#[derive(Default)]
struct EventCollector {
    event: Option<SegmentEvent>,
}

impl Perform for EventCollector {
    fn print(&mut self, c: char) {
        self.event = Some(SegmentEvent::Print(c));
    }

    fn execute(&mut self, byte: u8) {
        self.event = Some(match byte {
            b'\r' => SegmentEvent::CarriageReturn,
            b'\n' => SegmentEvent::LineFeed,
            _ => SegmentEvent::Other,
        });
    }

    fn csi_dispatch(&mut self, _: &Params, _: &[u8], _: bool, _: u8) {
        self.event = Some(SegmentEvent::Other);
    }

    fn esc_dispatch(&mut self, _: &[u8], _: bool, _: u8) {
        self.event = Some(SegmentEvent::Other);
    }

    fn osc_dispatch(&mut self, _: &[&[u8]], _: bool) {
        self.event = Some(SegmentEvent::Other);
    }

    fn hook(&mut self, _: &Params, _: &[u8], _: bool, _: u8) {
        self.event = Some(SegmentEvent::Other);
    }

    fn put(&mut self, _: u8) {
        self.event = Some(SegmentEvent::Other);
    }

    fn unhook(&mut self) {
        self.event = Some(SegmentEvent::Other);
    }
}

/// Main function for splitting text into line segments with zero-copy slices
pub fn split_into_line_segments<'a>(input: &'a str, term_width: usize) -> Vec<LineSegment<'a>> {
    let mut parser = Parser::<DefaultCharAccumulator>::new();
    let mut performer = EventCollector::default();

    let mut segments = Vec::<LineSegment>::new();
    let mut segment_start = 0_usize;
    let mut segment_end = 0_usize;
    let mut visual_width = 0_usize;
    let mut has_visual = false;
    let mut prev_is_cr = false;

    macro_rules! push_segment {
        ($end:expr, $crlf:expr) => {
            #[allow(unused_assignments)]
            {
                segments.push(LineSegment {
                    content: &input[segment_start..$end],
                    ends_with_crlf: $crlf,
                });
                visual_width = 0;
                has_visual = false;
            }
        };
    }

    for (index, byte) in input.bytes().enumerate() {
        parser.advance(&mut performer, byte);
        let Some(event) = performer.event.take() else {
            // Mid-sequence byte (escape params, UTF-8 continuation, …): the
            // action it belongs to is dispatched on the sequence's final byte
            // and its bytes are claimed then.
            continue;
        };

        let mut is_cr = false;

        match event {
            SegmentEvent::LineFeed => {
                // Emit current segment but strip \r if the segment ended with it.
                // Note: `segment_end` (not `index`) is deliberate — a LF can
                // fire mid-escape-sequence ("\x1b[3\n1m"), and the pending
                // escape bytes must not leak into the emitted segment.
                push_segment!(segment_end - usize::from(prev_is_cr), true);
                // We skip \n itself (and possibly the preceding \r, and any
                // pending escape bytes) so they don't end up in segments
                segment_end = index + 1;
                segment_start = segment_end;
            }
            SegmentEvent::CarriageReturn => {
                // Reset visual width and continue with the current segment
                segment_end = index + 1;
                visual_width = 0;
                is_cr = true;
            }
            SegmentEvent::Print(ch) => {
                // Input is a valid &str, so print fires on the last byte of
                // the char's UTF-8 encoding; anything unclaimed before the
                // char (e.g. an aborted escape) folds into the current
                // segment so the wrap point lands on the char boundary.
                let char_bytes = ch.len_utf8();
                segment_end = index + 1 - char_bytes;

                // The only case where visual width actually grows
                // (assuming we don't have cursor move etc, only CSI::Sgr/Control/Print)
                let char_width = ch.width().unwrap_or(0);
                let new_width = visual_width + char_width;
                if new_width > term_width && has_visual {
                    // We're beyond term width, emit current segment and start next one from this char
                    push_segment!(segment_end, false);
                    segment_start = segment_end;
                    segment_end += char_bytes;
                    visual_width = char_width; // Reset to just this character's width
                    has_visual = true;
                    // Very unlikely edge case: char_width > term size and we have to flush it again
                    if char_width > term_width {
                        push_segment!(segment_end, false);
                        segment_start = segment_end;
                    }
                } else {
                    // We can safely extend our current pending segment
                    segment_end += char_bytes;
                    visual_width = new_width;
                    has_visual = true;
                }
            }
            SegmentEvent::Other => {
                // Extend current segment with other ansi markers
                segment_end = index + 1;
            }
        }

        prev_is_cr = is_cr;
    }

    // Trailing bytes that never completed an action (e.g. a dangling "\x1b[")
    // are left out of `segment_end`, matching the previous termwiz-based
    // implementation which never consumed incomplete sequences.

    // We have pending segment that hasn't been pushed, without crlf
    if segment_end > segment_start {
        let input_start = input.as_ptr();
        if let Some(last) = segments.last_mut() {
            // There's at least one segment
            let last_start = last.content.as_ptr();
            let last_end = unsafe { last_start.add(last.content.len()) };
            if !last.ends_with_crlf && !has_visual {
                // Last segment doesn't end with crlf and the current one has no visual actions, concatenate
                debug_assert_eq!(segment_start, (last_end as usize - input_start as usize));
                let last_offset = last_start as usize - input_start as usize;
                last.content = &input[last_offset..segment_end];
            } else {
                // There's last segment but either it ends with lf or pending segment has visual width
                // note: pending segment can't have lf because otherwise we would have matched on it
                push_segment!(segment_end, false);
            }
        } else {
            // There's no segments, this is the only one (and with no lf)
            push_segment!(segment_end, false);
        }
    }

    segments
}

#[cfg(test)]
#[path = "segment_tests.rs"]
mod tests;
