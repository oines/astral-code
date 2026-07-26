use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadTokenUsage;
use codex_app_server_protocol::ThreadTokenUsageUpdatedNotification;
use codex_app_server_protocol::TokenUsageBreakdown;
use pretty_assertions::assert_eq;

use super::RunOptions;
use super::RunViewport;
use super::handle_notification;
use super::viewport_rows;
use crate::SurfaceState;

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

#[test]
fn token_usage_notification_updates_the_active_surface() {
    let mut surface = SurfaceState::new("thread-1");
    let token_usage = ThreadTokenUsage {
        total: usage(12_345),
        last: usage(9_200),
        model_context_window: Some(500_000),
    };
    handle_notification(
        &mut surface,
        &ServerNotification::ThreadTokenUsageUpdated(ThreadTokenUsageUpdatedNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            token_usage: token_usage.clone(),
        }),
    );

    assert_eq!(surface.token_usage(), Some(&token_usage));
}

fn usage(total_tokens: i64) -> TokenUsageBreakdown {
    TokenUsageBreakdown {
        total_tokens,
        input_tokens: total_tokens,
        cached_input_tokens: 0,
        output_tokens: 0,
        reasoning_output_tokens: 0,
    }
}
