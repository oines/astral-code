use codex_core::config::Config;
use codex_login::AuthManager;
use tracing::debug;

pub(crate) async fn rate_limits_ok(_auth_manager: &AuthManager, _config: &Config) -> bool {
    debug!("skipping legacy remote memories rate-limit check");
    true
}
