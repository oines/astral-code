use pretty_assertions::assert_eq;

use super::ScrollbackViewport;

#[test]
fn viewport_follows_the_tail_at_zero_distance() {
    assert_eq!(
        ScrollbackViewport::measure(40, 10, 0),
        ScrollbackViewport {
            first_visible_line: 30,
            end_visible_line: 40,
            total_lines: 40,
            viewport_lines: 10,
            has_content_above: true,
            has_content_below: false,
        }
    );
}

#[test]
fn viewport_exposes_content_below_when_scrolled_up() {
    assert_eq!(
        ScrollbackViewport::measure(40, 10, 7),
        ScrollbackViewport {
            first_visible_line: 23,
            end_visible_line: 33,
            total_lines: 40,
            viewport_lines: 10,
            has_content_above: true,
            has_content_below: true,
        }
    );
}

#[test]
fn viewport_clamps_at_the_top() {
    assert_eq!(
        ScrollbackViewport::measure(8, 20, usize::MAX),
        ScrollbackViewport {
            first_visible_line: 0,
            end_visible_line: 8,
            total_lines: 8,
            viewport_lines: 20,
            has_content_above: false,
            has_content_below: false,
        }
    );
}
