//! Shared retry decisions for streaming model requests.

use std::time::Duration;

use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::util::backoff;
use codex_protocol::error::CodexErr;
use tracing::warn;

#[derive(Debug, Clone, Copy)]
pub(crate) enum ResponsesStreamRequest {
    Sampling,
}

/// Handles a retryable stream error and returns `Ok(())` when the caller should
/// retry the request loop.
pub(crate) async fn handle_retryable_response_stream_error(
    retries: &mut u64,
    max_retries: u64,
    err: CodexErr,
    _client_session: &mut crate::client::ModelClientSession,
    sess: &Session,
    turn_context: &TurnContext,
    request: ResponsesStreamRequest,
) -> Result<(), CodexErr> {
    if *retries < max_retries {
        *retries += 1;
        let retry_count = *retries;
        let delay = match &err {
            CodexErr::Stream(_, requested_delay) => {
                requested_delay.unwrap_or_else(|| backoff(retry_count))
            }
            _ => backoff(retry_count),
        };
        log_retry(request, retry_count, max_retries, delay);
        sess.notify_stream_error(
            turn_context,
            format!("Reconnecting... {retry_count}/{max_retries}"),
            err,
        )
        .await;
        tokio::time::sleep(delay).await;
        return Ok(());
    }

    Err(err)
}

fn log_retry(request: ResponsesStreamRequest, retries: u64, max_retries: u64, delay: Duration) {
    match request {
        ResponsesStreamRequest::Sampling => {
            warn!(
                "stream disconnected - retrying sampling request ({retries}/{max_retries} in {delay:?})...",
            );
        }
    }
}
