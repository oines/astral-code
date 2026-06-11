//! Rate-limit warning, prompt, and notice surfaces for `ChatWidget`.

use super::*;
use codex_app_server_protocol::CodexErrorInfo as AppServerCodexErrorInfo;

const RATE_LIMIT_WARNING_THRESHOLDS: [f64; 3] = [75.0, 90.0, 95.0];
const PRIMARY_LIMIT_FALLBACK_LABEL: &str = "usage";
const SECONDARY_LIMIT_FALLBACK_LABEL: &str = "secondary usage";

#[derive(Default)]
pub(super) struct RateLimitWarningState {
    pub(super) secondary_index: usize,
    pub(super) primary_index: usize,
}

impl RateLimitWarningState {
    pub(super) fn take_warnings(
        &mut self,
        secondary_used_percent: Option<f64>,
        secondary_window_minutes: Option<i64>,
        primary_used_percent: Option<f64>,
        primary_window_minutes: Option<i64>,
    ) -> Vec<String> {
        let reached_secondary_cap =
            matches!(secondary_used_percent, Some(percent) if percent == 100.0);
        let reached_primary_cap = matches!(primary_used_percent, Some(percent) if percent == 100.0);
        if reached_secondary_cap || reached_primary_cap {
            return Vec::new();
        }

        let mut warnings = Vec::new();

        if let Some(secondary_used_percent) = secondary_used_percent {
            let mut highest_secondary: Option<f64> = None;
            while self.secondary_index < RATE_LIMIT_WARNING_THRESHOLDS.len()
                && secondary_used_percent >= RATE_LIMIT_WARNING_THRESHOLDS[self.secondary_index]
            {
                highest_secondary = Some(RATE_LIMIT_WARNING_THRESHOLDS[self.secondary_index]);
                self.secondary_index += 1;
            }
            if let Some(threshold) = highest_secondary {
                let limit_label =
                    limit_label_for_window(secondary_window_minutes, /*is_secondary*/ true);
                let remaining_percent = 100.0 - threshold;
                warnings.push(format!(
                    "Heads up, you have less than {remaining_percent:.0}% of your {limit_label} limit left. Run /status for a breakdown."
                ));
            }
        }

        if let Some(primary_used_percent) = primary_used_percent {
            let mut highest_primary: Option<f64> = None;
            while self.primary_index < RATE_LIMIT_WARNING_THRESHOLDS.len()
                && primary_used_percent >= RATE_LIMIT_WARNING_THRESHOLDS[self.primary_index]
            {
                highest_primary = Some(RATE_LIMIT_WARNING_THRESHOLDS[self.primary_index]);
                self.primary_index += 1;
            }
            if let Some(threshold) = highest_primary {
                let limit_label =
                    limit_label_for_window(primary_window_minutes, /*is_secondary*/ false);
                let remaining_percent = 100.0 - threshold;
                warnings.push(format!(
                    "Heads up, you have less than {remaining_percent:.0}% of your {limit_label} limit left. Run /status for a breakdown."
                ));
            }
        }

        warnings
    }
}

pub(crate) fn limit_label_for_window(window_minutes: Option<i64>, is_secondary: bool) -> String {
    window_minutes
        .and_then(get_limits_duration)
        .unwrap_or_else(|| fallback_limit_label(is_secondary).to_string())
}

pub(crate) fn get_limits_duration(windows_minutes: i64) -> Option<String> {
    const MINUTES_PER_HOUR: i64 = 60;
    const MINUTES_PER_5_HOURS: i64 = 5 * MINUTES_PER_HOUR;
    const MINUTES_PER_DAY: i64 = 24 * MINUTES_PER_HOUR;
    const MINUTES_PER_WEEK: i64 = 7 * MINUTES_PER_DAY;
    const MINUTES_PER_MONTH: i64 = 30 * MINUTES_PER_DAY;
    const MINUTES_PER_YEAR: i64 = 365 * MINUTES_PER_DAY;

    let windows_minutes = windows_minutes.max(0);

    if is_approximate_window(windows_minutes, MINUTES_PER_5_HOURS) {
        Some("5h".to_string())
    } else if is_approximate_window(windows_minutes, MINUTES_PER_DAY) {
        Some("daily".to_string())
    } else if is_approximate_window(windows_minutes, MINUTES_PER_WEEK) {
        Some("weekly".to_string())
    } else if is_approximate_window(windows_minutes, MINUTES_PER_MONTH) {
        Some("monthly".to_string())
    } else if is_approximate_window(windows_minutes, MINUTES_PER_YEAR) {
        Some("annual".to_string())
    } else {
        None
    }
}

pub(crate) fn fallback_limit_label(is_secondary: bool) -> &'static str {
    if is_secondary {
        SECONDARY_LIMIT_FALLBACK_LABEL
    } else {
        PRIMARY_LIMIT_FALLBACK_LABEL
    }
}

fn is_approximate_window(minutes: i64, expected_minutes: i64) -> bool {
    let minutes = minutes as f64;
    let expected_minutes = expected_minutes as f64;
    minutes >= expected_minutes * 0.95 && minutes <= expected_minutes * 1.05
}

#[derive(Debug)]
pub(super) enum RateLimitErrorKind {
    ServerOverloaded,
    UsageLimit,
    Generic,
}

pub(super) fn app_server_rate_limit_error_kind(
    info: &AppServerCodexErrorInfo,
) -> Option<RateLimitErrorKind> {
    match info {
        AppServerCodexErrorInfo::ServerOverloaded => Some(RateLimitErrorKind::ServerOverloaded),
        AppServerCodexErrorInfo::UsageLimitExceeded => Some(RateLimitErrorKind::UsageLimit),
        AppServerCodexErrorInfo::ResponseTooManyFailedAttempts {
            http_status_code: Some(429),
        } => Some(RateLimitErrorKind::Generic),
        _ => None,
    }
}

pub(super) fn is_app_server_cyber_policy_error(info: &AppServerCodexErrorInfo) -> bool {
    matches!(info, AppServerCodexErrorInfo::CyberPolicy)
}

#[derive(Clone, Copy)]
enum RateLimitSnapshotSource {
    AccountUsage,
    RollingUpdate,
}

impl ChatWidget {
    pub(crate) fn on_rate_limit_snapshot(&mut self, snapshot: Option<RateLimitSnapshot>) {
        self.on_rate_limit_snapshot_from(snapshot, RateLimitSnapshotSource::AccountUsage);
    }

    pub(crate) fn on_rolling_rate_limit_snapshot(&mut self, snapshot: RateLimitSnapshot) {
        // Rolling app-server notifications are sparse. Preserve metadata learned from the full read.
        self.on_rate_limit_snapshot_from(Some(snapshot), RateLimitSnapshotSource::RollingUpdate);
    }

    fn on_rate_limit_snapshot_from(
        &mut self,
        snapshot: Option<RateLimitSnapshot>,
        source: RateLimitSnapshotSource,
    ) {
        if let Some(mut snapshot) = snapshot {
            let limit_id = snapshot
                .limit_id
                .clone()
                .unwrap_or_else(|| "codex".to_string());
            let limit_label = snapshot
                .limit_name
                .clone()
                .unwrap_or_else(|| limit_id.clone());
            if snapshot.credits.is_none() {
                snapshot.credits = self
                    .rate_limit_snapshots_by_limit_id
                    .get(&limit_id)
                    .and_then(|display| display.credits.as_ref())
                    .map(|credits| CreditsSnapshot {
                        has_credits: credits.has_credits,
                        unlimited: credits.unlimited,
                        balance: credits.balance.clone(),
                    });
            }
            let preserved_individual_limit =
                if matches!(source, RateLimitSnapshotSource::RollingUpdate)
                    && snapshot.individual_limit.is_none()
                {
                    self.rate_limit_snapshots_by_limit_id
                        .get(&limit_id)
                        .and_then(|display| display.individual_limit.clone())
                } else {
                    None
                };
            self.plan_type = snapshot.plan_type.or(self.plan_type);

            let is_codex_limit = limit_id.eq_ignore_ascii_case("codex");
            if is_codex_limit
                && let Some(rate_limit_reached_type) = snapshot.rate_limit_reached_type
            {
                self.codex_rate_limit_reached_type = Some(rate_limit_reached_type);
            }
            let warnings = if is_codex_limit {
                self.rate_limit_warnings.take_warnings(
                    snapshot
                        .secondary
                        .as_ref()
                        .map(|window| f64::from(window.used_percent)),
                    snapshot
                        .secondary
                        .as_ref()
                        .and_then(|window| window.window_duration_mins),
                    snapshot
                        .primary
                        .as_ref()
                        .map(|window| f64::from(window.used_percent)),
                    snapshot
                        .primary
                        .as_ref()
                        .and_then(|window| window.window_duration_mins),
                )
            } else {
                vec![]
            };

            let mut display =
                rate_limit_snapshot_display_for_limit(&snapshot, limit_label, Local::now());
            if display.individual_limit.is_none() {
                display.individual_limit = preserved_individual_limit;
            }
            self.rate_limit_snapshots_by_limit_id
                .insert(limit_id, display);

            if !warnings.is_empty() {
                for warning in warnings {
                    self.add_to_history(history_cell::new_warning_event(warning));
                }
                self.request_redraw();
            }
        } else {
            self.rate_limit_snapshots_by_limit_id.clear();
            self.codex_rate_limit_reached_type = None;
        }
        self.refresh_status_line();
    }

    pub(super) fn stop_rate_limit_poller(&mut self) {}

    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn prefetch_rate_limits(&mut self) {
        self.stop_rate_limit_poller();
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn should_prefetch_rate_limits(&self) -> bool {
        self.config.model_provider.requires_astral_auth && self.has_chatgpt_account
    }
}
