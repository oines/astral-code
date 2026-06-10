use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::prelude::Widget;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;

use crate::motion::MotionMode;
use crate::motion::shimmer_text;

use super::AuthModeWidget;
use super::ContinueWithDeviceCodeState;
#[cfg(test)]
use super::SignInState;
use super::mark_url_hyperlink;

pub(super) fn render_device_code_login(
    widget: &AuthModeWidget,
    area: Rect,
    buf: &mut Buffer,
    state: &ContinueWithDeviceCodeState,
) {
    let banner = if state.is_showing_copyable_auth() {
        "Finish signing in via your browser"
    } else {
        "Preparing device code login"
    };

    let mut spans = vec!["  ".into()];
    if widget.animations_enabled && !widget.animations_suppressed.get() {
        widget
            .request_frame
            .schedule_frame_in(std::time::Duration::from_millis(100));
        spans.extend(shimmer_text(banner, MotionMode::Animated));
    } else {
        spans.push(banner.into());
    }

    let mut lines = vec![spans.into(), "".into()];

    let verification_url = if let (Some(verification_url), Some(user_code)) =
        (&state.verification_url, &state.user_code)
    {
        lines.push("  1. Open this link in your browser and sign in".into());
        lines.push("".into());
        lines.push(Line::from(vec![
            "  ".into(),
            verification_url.as_str().cyan().underlined(),
        ]));
        lines.push("".into());
        lines.push(
            "  2. Enter this one-time code after you are signed in (expires in 15 minutes)".into(),
        );
        lines.push("".into());
        lines.push(Line::from(vec![
            "  ".into(),
            user_code.as_str().cyan().bold(),
        ]));
        lines.push("".into());
        lines.push(
            "  Device codes are a common phishing target. Never share this code."
                .dim()
                .into(),
        );
        lines.push("".into());
        Some(verification_url.clone())
    } else {
        lines.push("  Requesting a one-time code...".dim().into());
        lines.push("".into());
        None
    };

    lines.push(Line::from(vec![
        "  Press ".dim(),
        widget.cancel_binding().into(),
        " to cancel".dim(),
    ]));
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .render(area, buf);

    if let Some(url) = &verification_url {
        mark_url_hyperlink(buf, area, url);
    }
}

#[cfg(test)]
fn device_code_attempt_matches(state: &SignInState, request_id: &str) -> bool {
    matches!(
        state,
        SignInState::ChatGptDeviceCode(state) if state.request_id == request_id
    )
}

#[cfg(test)]
fn set_device_code_state_for_active_attempt(
    sign_in_state: &std::sync::Arc<std::sync::RwLock<SignInState>>,
    request_frame: &crate::tui::FrameRequester,
    request_id: &str,
    next_state: ContinueWithDeviceCodeState,
) -> bool {
    let mut guard = sign_in_state.write().unwrap();
    if !device_code_attempt_matches(&guard, request_id) {
        return false;
    }

    *guard = SignInState::ChatGptDeviceCode(next_state);
    drop(guard);
    request_frame.schedule_frame();
    true
}

#[cfg(test)]
fn set_device_code_error_for_active_attempt(
    sign_in_state: &std::sync::Arc<std::sync::RwLock<SignInState>>,
    request_frame: &crate::tui::FrameRequester,
    error: &std::sync::Arc<std::sync::RwLock<Option<String>>>,
    request_id: &str,
    message: String,
) -> bool {
    let mut guard = sign_in_state.write().unwrap();
    if !device_code_attempt_matches(&guard, request_id) {
        return false;
    }

    *guard = SignInState::PickMode;
    drop(guard);
    *error.write().unwrap() = Some(message);
    request_frame.schedule_frame();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;
    use std::sync::RwLock;

    fn pending_device_code_state(request_id: &str) -> Arc<RwLock<SignInState>> {
        Arc::new(RwLock::new(SignInState::ChatGptDeviceCode(
            ContinueWithDeviceCodeState::pending(request_id.to_string()),
        )))
    }

    #[test]
    fn device_code_attempt_matches_only_for_matching_request_id() {
        let state = SignInState::ChatGptDeviceCode(ContinueWithDeviceCodeState::pending(
            "request-1".to_string(),
        ));

        assert_eq!(device_code_attempt_matches(&state, "request-1"), true);
        assert_eq!(device_code_attempt_matches(&state, "request-2"), false);
        assert_eq!(
            device_code_attempt_matches(&SignInState::PickMode, "request-1"),
            false
        );
    }

    #[test]
    fn set_device_code_state_for_active_attempt_updates_only_when_active() {
        let request_frame = crate::tui::FrameRequester::test_dummy();
        let sign_in_state = pending_device_code_state("request-1");

        assert_eq!(
            set_device_code_state_for_active_attempt(
                &sign_in_state,
                &request_frame,
                "request-1",
                ContinueWithDeviceCodeState::ready(
                    "request-1".to_string(),
                    "login-1".to_string(),
                    "https://example.com/device".to_string(),
                    "ABCD-EFGH".to_string(),
                ),
            ),
            true
        );
        assert!(matches!(
            &*sign_in_state.read().unwrap(),
            SignInState::ChatGptDeviceCode(state) if state.login_id() == Some("login-1")
        ));

        let sign_in_state = pending_device_code_state("request-2");
        assert_eq!(
            set_device_code_state_for_active_attempt(
                &sign_in_state,
                &request_frame,
                "request-1",
                ContinueWithDeviceCodeState::ready(
                    "request-1".to_string(),
                    "login-1".to_string(),
                    "https://example.com/device".to_string(),
                    "ABCD-EFGH".to_string(),
                ),
            ),
            false
        );
        assert!(matches!(
            &*sign_in_state.read().unwrap(),
            SignInState::ChatGptDeviceCode(state) if state.login_id.is_none()
        ));
    }

    #[test]
    fn set_device_code_error_for_active_attempt_updates_only_when_active() {
        let request_frame = crate::tui::FrameRequester::test_dummy();
        let error = Arc::new(RwLock::new(None));
        let sign_in_state = pending_device_code_state("request-1");

        assert_eq!(
            set_device_code_error_for_active_attempt(
                &sign_in_state,
                &request_frame,
                &error,
                "request-1",
                "device code unavailable".to_string(),
            ),
            true
        );
        assert!(matches!(
            &*sign_in_state.read().unwrap(),
            SignInState::PickMode
        ));
        assert_eq!(
            error.read().unwrap().as_deref(),
            Some("device code unavailable")
        );

        let error = Arc::new(RwLock::new(None));
        let sign_in_state = pending_device_code_state("request-2");
        assert_eq!(
            set_device_code_error_for_active_attempt(
                &sign_in_state,
                &request_frame,
                &error,
                "request-1",
                "device code unavailable".to_string(),
            ),
            false
        );
        assert!(matches!(
            &*sign_in_state.read().unwrap(),
            SignInState::ChatGptDeviceCode(_)
        ));
        assert_eq!(*error.read().unwrap(), None);
    }
}
