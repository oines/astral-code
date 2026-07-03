use crate::error::TransportError;
use crate::request::Request;
use http::header::RETRY_AFTER;
use rand::Rng;
use std::future::Future;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_attempts: u64,
    pub base_delay: Duration,
    pub retry_on: RetryOn,
}

#[derive(Debug, Clone)]
pub struct RetryOn {
    pub retry_429: bool,
    pub retry_5xx: bool,
    pub retry_transport: bool,
}

impl RetryOn {
    pub fn should_retry(&self, err: &TransportError, attempt: u64, max_attempts: u64) -> bool {
        if attempt >= max_attempts {
            return false;
        }
        match err {
            TransportError::Http { status, body, .. } => {
                (self.retry_429
                    && status.as_u16() == 429
                    && !is_usage_limit_reached_body(body.as_deref()))
                    || (self.retry_5xx && status.is_server_error())
            }
            TransportError::Timeout | TransportError::Network(_) => self.retry_transport,
            _ => false,
        }
    }
}

pub fn backoff(base: Duration, attempt: u64) -> Duration {
    if attempt == 0 {
        return base;
    }
    let exp = 2u64.saturating_pow(attempt as u32 - 1);
    let millis = base.as_millis() as u64;
    let raw = millis.saturating_mul(exp);
    let jitter: f64 = rand::rng().random_range(0.9..1.1);
    Duration::from_millis((raw as f64 * jitter) as u64)
}

pub async fn run_with_retry<T, F, Fut>(
    policy: RetryPolicy,
    mut make_req: impl FnMut() -> Request,
    op: F,
) -> Result<T, TransportError>
where
    F: Fn(Request, u64) -> Fut,
    Fut: Future<Output = Result<T, TransportError>>,
{
    for attempt in 0..=policy.max_attempts {
        let req = make_req();
        match op(req, attempt).await {
            Ok(resp) => return Ok(resp),
            Err(err)
                if policy
                    .retry_on
                    .should_retry(&err, attempt, policy.max_attempts) =>
            {
                let delay = retry_after_delay(&err)
                    .unwrap_or_else(|| backoff(policy.base_delay, attempt + 1));
                sleep(delay).await;
            }
            Err(err) => return Err(err),
        }
    }
    Err(TransportError::RetryLimit)
}

fn retry_after_delay(err: &TransportError) -> Option<Duration> {
    let TransportError::Http {
        headers: Some(headers),
        ..
    } = err
    else {
        return None;
    };

    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    let seconds = value.parse::<u64>().ok()?;
    Some(Duration::from_secs(seconds))
}

fn is_usage_limit_reached_body(body: Option<&str>) -> bool {
    let Some(body) = body else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    value
        .get("error")
        .and_then(|error| error.get("type"))
        .and_then(serde_json::Value::as_str)
        == Some("usage_limit_reached")
}

#[cfg(test)]
mod tests {
    use super::RetryOn;
    use super::retry_after_delay;
    use crate::TransportError;
    use http::HeaderMap;
    use http::HeaderValue;
    use http::StatusCode;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn retry_after_delay_parses_delta_seconds() {
        let mut headers = HeaderMap::new();
        headers.insert(http::header::RETRY_AFTER, HeaderValue::from_static("3"));
        let err = TransportError::Http {
            status: StatusCode::TOO_MANY_REQUESTS,
            url: None,
            headers: Some(headers),
            body: None,
        };

        assert_eq!(retry_after_delay(&err), Some(Duration::from_secs(3)));
    }

    #[test]
    fn retry_after_delay_ignores_non_delta_values() {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::RETRY_AFTER,
            HeaderValue::from_static("Fri, 03 Jul 2026 10:00:00 GMT"),
        );
        let err = TransportError::Http {
            status: StatusCode::TOO_MANY_REQUESTS,
            url: None,
            headers: Some(headers),
            body: None,
        };

        assert_eq!(retry_after_delay(&err), None);
    }

    #[test]
    fn retry_on_429_skips_usage_limit_reached_body() {
        let retry_on = RetryOn {
            retry_429: true,
            retry_5xx: true,
            retry_transport: true,
        };
        let err = TransportError::Http {
            status: StatusCode::TOO_MANY_REQUESTS,
            url: None,
            headers: None,
            body: Some(r#"{"error":{"type":"usage_limit_reached"}}"#.to_string()),
        };

        assert!(!retry_on.should_retry(&err, /*attempt*/ 0, /*max_attempts*/ 3));
    }

    #[test]
    fn retry_on_429_retries_when_usage_limit_body_is_not_json() {
        let retry_on = RetryOn {
            retry_429: true,
            retry_5xx: true,
            retry_transport: true,
        };
        let err = TransportError::Http {
            status: StatusCode::TOO_MANY_REQUESTS,
            url: None,
            headers: None,
            body: Some(r#""type":"usage_limit_reached""#.to_string()),
        };

        assert!(retry_on.should_retry(&err, /*attempt*/ 0, /*max_attempts*/ 3));
    }
}
