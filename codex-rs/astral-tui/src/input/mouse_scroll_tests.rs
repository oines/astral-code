use std::time::Duration;
use std::time::Instant;

use codex_terminal_detection::TerminalName;

use super::MouseScrollState;
use super::ScrollConfig;
use super::ScrollDirection;

#[test]
fn apple_terminal_wheel_reports_normalize_to_one_three_line_notch() {
    let start = Instant::now();
    let mut state = MouseScrollState::new_at(start);
    let config = ScrollConfig::for_terminal(
        TerminalName::AppleTerminal,
        /* remultiplexed */ false,
        /* viewport_height */ 30,
    );

    let lines = [0, 4, 8]
        .into_iter()
        .map(|millis| {
            state.on_scroll_event_at(
                start + Duration::from_millis(millis),
                ScrollDirection::Down,
                config,
            )
        })
        .sum::<i32>();

    assert_eq!(lines, 3);
}

#[test]
fn burst_events_wait_for_the_scroll_clock_instead_of_drawing_each_event() {
    let start = Instant::now();
    let mut state = MouseScrollState::new_at(start);
    let config = ScrollConfig::for_terminal(
        TerminalName::Ghostty,
        /* remultiplexed */ false,
        /* viewport_height */ 30,
    );

    assert_eq!(
        state.on_scroll_event_at(start, ScrollDirection::Down, config),
        0
    );
    assert_eq!(
        state.on_scroll_event_at(
            start + Duration::from_millis(2),
            ScrollDirection::Down,
            config,
        ),
        0
    );
    assert!(
        state
            .clock_deadline(start + Duration::from_millis(2))
            .is_some()
    );
}
