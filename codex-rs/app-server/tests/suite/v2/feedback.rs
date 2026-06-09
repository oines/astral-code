use anyhow::Result;
use app_test_support::TestAppServer;
use codex_app_server_protocol::FeedbackUploadParams;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::RequestId;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn feedback_upload_is_disabled() -> Result<()> {
    let codex_home = TempDir::new()?;
    std::fs::write(codex_home.path().join("config.toml"), "model = \"mock\"\n")?;

    let mut mcp = TestAppServer::new(codex_home.path()).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let request_id = mcp
        .send_feedback_upload_request(FeedbackUploadParams {
            classification: "bug".to_string(),
            reason: Some("do not upload this".to_string()),
            thread_id: None,
            include_logs: true,
            extra_log_files: None,
            tags: None,
        })
        .await?;
    let err: JSONRPCError = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;

    assert_eq!(
        err.error.message,
        "feedback upload is disabled in astral-code"
    );
    Ok(())
}
