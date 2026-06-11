use super::*;
use crate::error_code::INVALID_REQUEST_ERROR_CODE;
use pretty_assertions::assert_eq;

fn expected_disabled_error() -> JSONRPCErrorError {
    JSONRPCErrorError {
        code: INVALID_REQUEST_ERROR_CODE,
        data: None,
        message: LEGACY_REMOTE_CONTROL_DISABLED_MESSAGE.to_string(),
    }
}

#[test]
fn enable_returns_disabled_error() {
    let err = RemoteControlRequestProcessor::new(/*remote_control_handle*/ None)
        .enable()
        .expect_err("legacy remote control should be disabled");

    assert_eq!(err, expected_disabled_error());
}

#[tokio::test]
async fn pairing_start_returns_disabled_error() {
    let err = RemoteControlRequestProcessor::new(/*remote_control_handle*/ None)
        .pairing_start(
            RemoteControlPairingStartParams::default(),
            /*app_server_client_name*/ None,
        )
        .await
        .expect_err("legacy remote control pairing should be disabled");

    assert_eq!(err, expected_disabled_error());
}

#[tokio::test]
async fn pairing_status_returns_disabled_error() {
    let err = RemoteControlRequestProcessor::new(/*remote_control_handle*/ None)
        .pairing_status(RemoteControlPairingStatusParams {
            pairing_code: Some("pairing-code".to_string()),
            manual_pairing_code: None,
        })
        .await
        .expect_err("legacy remote control pairing status should be disabled");

    assert_eq!(err, expected_disabled_error());
}

#[tokio::test]
async fn clients_list_returns_disabled_error() {
    let err = RemoteControlRequestProcessor::new(/*remote_control_handle*/ None)
        .clients_list(RemoteControlClientsListParams {
            environment_id: "environment-id".to_string(),
            cursor: None,
            limit: None,
            order: None,
        })
        .await
        .expect_err("legacy remote control client listing should be disabled");

    assert_eq!(err, expected_disabled_error());
}

#[tokio::test]
async fn clients_revoke_returns_disabled_error() {
    let err = RemoteControlRequestProcessor::new(/*remote_control_handle*/ None)
        .clients_revoke(RemoteControlClientsRevokeParams {
            environment_id: "environment-id".to_string(),
            client_id: "client-id".to_string(),
        })
        .await
        .expect_err("legacy remote control client revocation should be disabled");

    assert_eq!(err, expected_disabled_error());
}
