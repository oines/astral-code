use std::time::Duration;

use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::to_response;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RemoteControlClientsListOrder;
use codex_app_server_protocol::RemoteControlClientsListParams;
use codex_app_server_protocol::RemoteControlClientsRevokeParams;
use codex_app_server_protocol::RemoteControlConnectionStatus;
use codex_app_server_protocol::RemoteControlDisableResponse;
use codex_app_server_protocol::RemoteControlPairingStartParams;
use codex_app_server_protocol::RemoteControlPairingStatusParams;
use codex_app_server_protocol::RemoteControlStatusReadResponse;
use codex_app_server_protocol::RequestId;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const LEGACY_REMOTE_CONTROL_DISABLED_MESSAGE: &str = "legacy hosted remote control is disabled in Astral until a provider-neutral control plane exists";

#[tokio::test]
async fn remote_control_disable_returns_disabled_status() -> Result<()> {
    let codex_home = TempDir::new()?;
    let mut mcp = TestAppServer::new(codex_home.path()).await?;
    timeout(DEFAULT_TIMEOUT, mcp.initialize()).await??;

    let request_id = mcp.send_remote_control_disable_request().await?;
    let response: JSONRPCResponse = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let received: RemoteControlDisableResponse = to_response(response)?;

    assert_eq!(received.status, RemoteControlConnectionStatus::Disabled);
    assert!(!received.server_name.is_empty());
    assert_eq!(received.environment_id, None);
    assert!(!received.installation_id.is_empty());
    Ok(())
}

#[tokio::test]
async fn remote_control_status_read_returns_disabled_status() -> Result<()> {
    let codex_home = TempDir::new()?;
    let mut mcp = TestAppServer::new(codex_home.path()).await?;
    timeout(DEFAULT_TIMEOUT, mcp.initialize()).await??;

    let request_id = mcp.send_remote_control_status_read_request().await?;
    let response: JSONRPCResponse = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let received: RemoteControlStatusReadResponse = to_response(response)?;

    assert_eq!(received.status, RemoteControlConnectionStatus::Disabled);
    assert!(!received.server_name.is_empty());
    assert_eq!(received.environment_id, None);
    assert!(!received.installation_id.is_empty());
    Ok(())
}

#[tokio::test]
async fn remote_control_enable_returns_disabled_error() -> Result<()> {
    let codex_home = TempDir::new()?;
    let mut mcp = TestAppServer::new(codex_home.path()).await?;
    timeout(DEFAULT_TIMEOUT, mcp.initialize()).await??;

    let request_id = mcp.send_remote_control_enable_request().await?;
    expect_legacy_remote_control_disabled(&mut mcp, request_id).await
}

#[tokio::test]
async fn remote_control_pairing_returns_disabled_error() -> Result<()> {
    let codex_home = TempDir::new()?;
    let mut mcp = TestAppServer::new(codex_home.path()).await?;
    timeout(DEFAULT_TIMEOUT, mcp.initialize()).await??;

    let request_id = mcp
        .send_remote_control_pairing_start_request(RemoteControlPairingStartParams {
            manual_code: true,
        })
        .await?;
    expect_legacy_remote_control_disabled(&mut mcp, request_id).await?;

    let request_id = mcp
        .send_remote_control_pairing_status_request(RemoteControlPairingStatusParams {
            pairing_code: Some("pairing-code".to_string()),
            manual_pairing_code: None,
        })
        .await?;
    expect_legacy_remote_control_disabled(&mut mcp, request_id).await
}

#[tokio::test]
async fn remote_control_client_management_returns_disabled_error() -> Result<()> {
    let codex_home = TempDir::new()?;
    let mut mcp = TestAppServer::new(codex_home.path()).await?;
    timeout(DEFAULT_TIMEOUT, mcp.initialize()).await??;

    let request_id = mcp
        .send_remote_control_clients_list_request(RemoteControlClientsListParams {
            environment_id: "environment-id".to_string(),
            cursor: Some("cursor-id".to_string()),
            limit: Some(10),
            order: Some(RemoteControlClientsListOrder::Desc),
        })
        .await?;
    expect_legacy_remote_control_disabled(&mut mcp, request_id).await?;

    let request_id = mcp
        .send_remote_control_clients_revoke_request(RemoteControlClientsRevokeParams {
            environment_id: "environment-id".to_string(),
            client_id: "client-id".to_string(),
        })
        .await?;
    expect_legacy_remote_control_disabled(&mut mcp, request_id).await
}

async fn expect_legacy_remote_control_disabled(
    mcp: &mut TestAppServer,
    request_id: i64,
) -> Result<()> {
    let error: JSONRPCError = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert_eq!(error.error.message, LEGACY_REMOTE_CONTROL_DISABLED_MESSAGE);
    Ok(())
}
