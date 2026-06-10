use anyhow::Result;
use app_test_support::TestAppServer;
use codex_app_server_protocol::AddCreditsNudgeCreditType;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::SendAddCreditsNudgeEmailParams;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const INVALID_REQUEST_ERROR_CODE: i64 = -32600;
const ACCOUNT_BACKEND_DISABLED_MESSAGE: &str = "Astral-managed account usage and rate-limit APIs are unavailable for the active model provider.";

#[tokio::test]
async fn get_account_rate_limits_is_disabled() -> Result<()> {
    let codex_home = TempDir::new()?;

    let mut mcp =
        TestAppServer::new_with_env(codex_home.path(), &[("ASTRAL_API_KEY", None)]).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let request_id = mcp.send_get_account_rate_limits_request().await?;
    assert_account_backend_disabled_error(&mut mcp, request_id).await
}

#[tokio::test]
async fn send_add_credits_nudge_email_is_disabled() -> Result<()> {
    let codex_home = TempDir::new()?;

    let mut mcp =
        TestAppServer::new_with_env(codex_home.path(), &[("ASTRAL_API_KEY", None)]).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let request_id = mcp
        .send_add_credits_nudge_email_request(SendAddCreditsNudgeEmailParams {
            credit_type: AddCreditsNudgeCreditType::Credits,
        })
        .await?;
    assert_account_backend_disabled_error(&mut mcp, request_id).await
}

async fn assert_account_backend_disabled_error(
    mcp: &mut TestAppServer,
    request_id: i64,
) -> Result<()> {
    let error: JSONRPCError = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;

    assert_eq!(error.id, RequestId::Integer(request_id));
    assert_eq!(error.error.code, INVALID_REQUEST_ERROR_CODE);
    assert_eq!(error.error.message, ACCOUNT_BACKEND_DISABLED_MESSAGE);

    Ok(())
}
