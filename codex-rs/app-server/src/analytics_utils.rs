use std::sync::Arc;

use codex_analytics::AnalyticsEventsClient;
use codex_core::config::Config;
use codex_login::AuthManager;

pub(crate) fn analytics_events_client_from_config(
    _auth_manager: Arc<AuthManager>,
    _config: &Config,
) -> AnalyticsEventsClient {
    AnalyticsEventsClient::disabled()
}
