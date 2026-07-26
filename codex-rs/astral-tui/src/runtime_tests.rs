use pretty_assertions::assert_eq;

use super::RunOptions;
use super::RunViewport;
use super::viewport_rows;

#[test]
fn fullscreen_is_the_default_viewport() {
    assert_eq!(RunOptions::default().viewport, RunViewport::Fullscreen);
}

#[test]
fn viewport_is_bounded_by_terminal_and_keeps_minimum_live_region() {
    assert_eq!(viewport_rows(12, 40), 12);
    assert_eq!(viewport_rows(20, 10), 9);
    assert_eq!(viewport_rows(2, 40), 5);
    assert_eq!(viewport_rows(12, 3), 3);
}
