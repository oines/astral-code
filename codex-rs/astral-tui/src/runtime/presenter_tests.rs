use std::time::Duration;
use std::time::Instant;

use pretty_assertions::assert_eq;

use super::MIN_FRAME_INTERVAL;
use super::Presenter;

#[test]
fn streamed_updates_coalesce_until_the_next_frame() {
    let started_at = Instant::now();
    let mut presenter = Presenter::new(started_at);
    presenter.mark_presented(started_at);

    presenter.request(started_at + Duration::from_millis(1));
    presenter.request(started_at + Duration::from_millis(4));
    presenter.request(started_at + Duration::from_millis(8));

    assert_eq!(
        presenter,
        Presenter {
            dirty: true,
            last_presented_at: Some(started_at),
            next_frame_at: Some(started_at + MIN_FRAME_INTERVAL),
        }
    );
}

#[test]
fn update_after_the_frame_interval_can_present_immediately() {
    let started_at = Instant::now();
    let mut presenter = Presenter::new(started_at);
    presenter.mark_presented(started_at);
    let requested_at = started_at + MIN_FRAME_INTERVAL + Duration::from_millis(1);

    presenter.request(requested_at);

    assert_eq!(presenter.deadline(), Some(requested_at));
}
