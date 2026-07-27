use pretty_assertions::assert_eq;
use ratatui::layout::Rect;

use super::RAIL_WIDTH;
use super::RailEligibility;
use super::RailViewport;
use super::compute_rail;
use super::rail_width;

#[test]
fn rail_eligibility_matches_the_ported_visibility_policy() {
    assert_eq!(
        rail_width(RailEligibility {
            visible: true,
            area_width: 80,
            turn_count: 2,
        }),
        RAIL_WIDTH
    );
    assert_eq!(
        rail_width(RailEligibility {
            visible: true,
            area_width: 48,
            turn_count: 2,
        }),
        0
    );
}

#[test]
fn long_timeline_keeps_the_active_turn_in_its_window() {
    let rail = compute_rail(
        Rect::new(0, 0, 72, 8),
        70,
        12,
        RailViewport {
            active: Some(7),
            at_bottom: false,
        },
    )
    .expect("rail has room");

    assert!(rail.window.contains(&7));
    assert_eq!(rail.window.len(), 6);
}
