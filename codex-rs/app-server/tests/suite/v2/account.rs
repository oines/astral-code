use anyhow::Result;
use anyhow::bail;
use app_test_support::TestAppServer;
use app_test_support::to_response;

use app_test_support::ChatGptAuthFixture;
use app_test_support::ChatGptIdTokenClaims;
use app_test_support::encode_id_token;
use app_test_support::write_chatgpt_auth;
use app_test_support::write_models_cache;
use chrono::Duration as ChronoDuration;
use chrono::Utc;
use codex_app_server_protocol::Account;
use codex_app_server_protocol::AuthMode;
use codex_app_server_protocol::CancelLoginAccountParams;
use codex_app_server_protocol::CancelLoginAccountResponse;
use codex_app_server_protocol::CancelLoginAccountStatus;
use codex_app_server_protocol::GetAccountParams;
use codex_app_server_protocol::GetAccountResponse;
use codex_app_server_protocol::GetAuthStatusParams;
use codex_app_server_protocol::GetAuthStatusResponse;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::LoginAccountResponse;
use codex_app_server_protocol::LogoutAccountResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_config::types::AuthCredentialsStoreMode;
use codex_login::REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR;
use codex_login::login_with_api_key;
use codex_protocol::account::PlanType as AccountPlanType;
use pretty_assertions::assert_eq;
use serde_json::json;
use serial_test::serial;
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const LOGIN_ISSUER_ENV_VAR: &str = "CODEX_APP_SERVER_LOGIN_ISSUER";
const WORKSPACE_ID_DEVICE: &str = "123e4567-e89b-42d3-a456-426614174013";
const WORKSPACE_ID_STALE: &str = "123e4567-e89b-42d3-a456-426614174014";

// Helper to create a minimal config.toml for the app server
#[derive(Default)]
struct CreateConfigTomlParams {
    requires_openai_auth: Option<bool>,
    base_url: Option<String>,
    model_provider_id: Option<String>,
    extra_provider_config: Option<String>,
}

fn create_config_toml(codex_home: &Path, params: CreateConfigTomlParams) -> std::io::Result<()> {
    let config_toml = codex_home.join("config.toml");
    let base_url = params
        .base_url
        .unwrap_or_else(|| "http://127.0.0.1:0/v1".to_string());
    let requires_line = match params.requires_openai_auth {
        Some(true) => "requires_openai_auth = true\n".to_string(),
        Some(false) => String::new(),
        None => String::new(),
    };
    let model_provider_id = params
        .model_provider_id
        .unwrap_or_else(|| "mock_provider".to_string());
    let provider_section = if model_provider_id == "mock_provider" {
        format!(
            r#"[model_providers.mock_provider]
name = "Mock provider for test"
base_url = "{base_url}"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
{requires_line}
"#
        )
    } else {
        params.extra_provider_config.unwrap_or_default()
    };
    let contents = format!(
        r#"
model = "mock-model"
approval_policy = "never"
sandbox_mode = "danger-full-access"

model_provider = "{model_provider_id}"

[features]
shell_snapshot = false

{provider_section}
"#
    );
    std::fs::write(config_toml, contents)
}

async fn mock_device_code_usercode(server: &MockServer, interval_seconds: u64) {
    Mock::given(method("POST"))
        .and(path("/api/accounts/deviceauth/usercode"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_auth_id": "device-auth-123",
            "user_code": "CODE-12345",
            "interval": interval_seconds.to_string(),
        })))
        .mount(server)
        .await;
}

async fn mock_device_code_token_success(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/api/accounts/deviceauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "authorization_code": "poll-code-321",
            "code_challenge": "code-challenge-321",
            "code_verifier": "code-verifier-321",
        })))
        .mount(server)
        .await;
}

async fn mock_device_code_token_failure(server: &MockServer, status: u16) {
    Mock::given(method("POST"))
        .and(path("/api/accounts/deviceauth/token"))
        .respond_with(ResponseTemplate::new(status))
        .mount(server)
        .await;
}

async fn mock_device_code_oauth_token(server: &MockServer, id_token: &str) {
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id_token": id_token,
            "access_token": "access-token-123",
            "refresh_token": "refresh-token-123",
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn logout_account_removes_auth_and_notifies() -> Result<()> {
    let codex_home = TempDir::new()?;
    create_config_toml(codex_home.path(), CreateConfigTomlParams::default())?;

    login_with_api_key(
        codex_home.path(),
        "sk-test-key",
        AuthCredentialsStoreMode::File,
    )?;
    assert!(codex_home.path().join("auth.json").exists());

    let mut mcp =
        TestAppServer::new_with_env(codex_home.path(), &[("ASTRAL_API_KEY", None)]).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let id = mcp.send_logout_account_request().await?;
    let resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(id)),
    )
    .await??;
    let _ok: LogoutAccountResponse = to_response(resp)?;

    let note = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("account/updated"),
    )
    .await??;
    let parsed: ServerNotification = note.try_into()?;
    let ServerNotification::AccountUpdated(payload) = parsed else {
        bail!("unexpected notification: {parsed:?}");
    };
    assert!(
        payload.auth_mode.is_none(),
        "auth_method should be None after logout"
    );
    assert_eq!(payload.plan_type, None);

    assert!(
        !codex_home.path().join("auth.json").exists(),
        "auth.json should be deleted"
    );

    let get_id = mcp
        .send_get_account_request(GetAccountParams {
            refresh_token: false,
        })
        .await?;
    let get_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(get_id)),
    )
    .await??;
    let account: GetAccountResponse = to_response(get_resp)?;
    assert_eq!(account.account, None);
    Ok(())
}

#[tokio::test]
async fn login_account_api_key_succeeds_and_notifies() -> Result<()> {
    let codex_home = TempDir::new()?;
    create_config_toml(codex_home.path(), CreateConfigTomlParams::default())?;

    let mut mcp = TestAppServer::new(codex_home.path()).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let req_id = mcp
        .send_login_account_api_key_request("sk-test-key")
        .await?;
    let resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(req_id)),
    )
    .await??;
    let login: LoginAccountResponse = to_response(resp)?;
    assert_eq!(login, LoginAccountResponse::ApiKey {});

    let note = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("account/login/completed"),
    )
    .await??;
    let parsed: ServerNotification = note.try_into()?;
    let ServerNotification::AccountLoginCompleted(payload) = parsed else {
        bail!("unexpected notification: {parsed:?}");
    };
    pretty_assertions::assert_eq!(payload.login_id, None);
    pretty_assertions::assert_eq!(payload.success, true);
    pretty_assertions::assert_eq!(payload.error, None);

    let note = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("account/updated"),
    )
    .await??;
    let parsed: ServerNotification = note.try_into()?;
    let ServerNotification::AccountUpdated(payload) = parsed else {
        bail!("unexpected notification: {parsed:?}");
    };
    pretty_assertions::assert_eq!(payload.auth_mode, Some(AuthMode::ApiKey));
    pretty_assertions::assert_eq!(payload.plan_type, None);

    assert!(codex_home.path().join("auth.json").exists());
    Ok(())
}

#[tokio::test]
async fn login_account_chatgpt_rejected() -> Result<()> {
    let codex_home = TempDir::new()?;
    create_config_toml(codex_home.path(), CreateConfigTomlParams::default())?;

    let mut mcp = TestAppServer::new(codex_home.path()).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let request_id = mcp.send_login_account_chatgpt_request().await?;
    let err: JSONRPCError = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;

    assert_eq!(
        err.error.message,
        "ChatGPT login is disabled in astral-code. Set ASTRAL_API_KEY for the active model provider."
    );
    Ok(())
}

#[tokio::test]
async fn login_account_chatgpt_device_code_returns_error_when_disabled() -> Result<()> {
    let codex_home = TempDir::new()?;
    create_config_toml(codex_home.path(), CreateConfigTomlParams::default())?;

    let mut mcp = TestAppServer::new(codex_home.path()).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let request_id = mcp.send_login_account_chatgpt_device_code_request().await?;
    let err: JSONRPCError = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert_eq!(
        err.error.message,
        "ChatGPT login is disabled in astral-code. Set ASTRAL_API_KEY for the active model provider."
    );

    let maybe_completed = timeout(
        Duration::from_millis(500),
        mcp.read_stream_until_notification_message("account/login/completed"),
    )
    .await;
    assert!(
        maybe_completed.is_err(),
        "account/login/completed should not be emitted when device code start fails"
    );
    assert!(
        !codex_home.path().join("auth.json").exists(),
        "auth.json should not be created when device code start fails"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "Astral disables ChatGPT device code login"]
async fn login_account_chatgpt_device_code_succeeds_and_notifies() -> Result<()> {
    let codex_home = TempDir::new()?;
    let mock_server = MockServer::start().await;
    create_config_toml(
        codex_home.path(),
        CreateConfigTomlParams {
            requires_openai_auth: Some(true),
            base_url: Some(format!("{}/v1", mock_server.uri())),
            ..Default::default()
        },
    )?;
    write_models_cache(codex_home.path())?;

    mock_device_code_usercode(&mock_server, /*interval_seconds*/ 0).await;
    mock_device_code_token_success(&mock_server).await;
    let id_token = encode_id_token(
        &ChatGptIdTokenClaims::new()
            .email("device@example.com")
            .plan_type("pro")
            .chatgpt_account_id(WORKSPACE_ID_DEVICE),
    )?;
    mock_device_code_oauth_token(&mock_server, &id_token).await;

    let issuer = mock_server.uri();
    let mut mcp = TestAppServer::new_with_env(
        codex_home.path(),
        &[
            ("ASTRAL_API_KEY", None),
            (LOGIN_ISSUER_ENV_VAR, Some(issuer.as_str())),
        ],
    )
    .await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let request_id = mcp.send_login_account_chatgpt_device_code_request().await?;
    let resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let login: LoginAccountResponse = to_response(resp)?;
    let LoginAccountResponse::ChatgptDeviceCode {
        login_id,
        verification_url,
        user_code,
    } = login
    else {
        bail!("unexpected login response: {login:?}");
    };
    assert_eq!(verification_url, format!("{issuer}/codex/device"));
    assert_eq!(user_code, "CODE-12345");

    let note = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("account/login/completed"),
    )
    .await??;
    let parsed: ServerNotification = note.try_into()?;
    let ServerNotification::AccountLoginCompleted(payload) = parsed else {
        bail!("unexpected notification: {parsed:?}");
    };
    assert_eq!(payload.login_id, Some(login_id));
    assert_eq!(payload.success, true);
    assert_eq!(payload.error, None);

    let note = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("account/updated"),
    )
    .await??;
    let parsed: ServerNotification = note.try_into()?;
    let ServerNotification::AccountUpdated(payload) = parsed else {
        bail!("unexpected notification: {parsed:?}");
    };
    assert_eq!(payload.auth_mode, Some(AuthMode::Chatgpt));
    assert_eq!(payload.plan_type, Some(AccountPlanType::Pro));
    assert!(
        codex_home.path().join("auth.json").exists(),
        "auth.json should be created when device code login succeeds"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "Astral disables ChatGPT device code login"]
async fn login_account_chatgpt_device_code_failure_notifies_without_account_update() -> Result<()> {
    let codex_home = TempDir::new()?;
    let mock_server = MockServer::start().await;
    create_config_toml(
        codex_home.path(),
        CreateConfigTomlParams {
            requires_openai_auth: Some(true),
            base_url: Some(format!("{}/v1", mock_server.uri())),
            ..Default::default()
        },
    )?;
    write_models_cache(codex_home.path())?;

    mock_device_code_usercode(&mock_server, /*interval_seconds*/ 0).await;
    mock_device_code_token_failure(&mock_server, /*status*/ 500).await;

    let issuer = mock_server.uri();
    let mut mcp = TestAppServer::new_with_env(
        codex_home.path(),
        &[
            ("ASTRAL_API_KEY", None),
            (LOGIN_ISSUER_ENV_VAR, Some(issuer.as_str())),
        ],
    )
    .await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let request_id = mcp.send_login_account_chatgpt_device_code_request().await?;
    let resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let login: LoginAccountResponse = to_response(resp)?;
    let LoginAccountResponse::ChatgptDeviceCode { login_id, .. } = login else {
        bail!("unexpected login response: {login:?}");
    };

    let note = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("account/login/completed"),
    )
    .await??;
    let parsed: ServerNotification = note.try_into()?;
    let ServerNotification::AccountLoginCompleted(payload) = parsed else {
        bail!("unexpected notification: {parsed:?}");
    };
    assert_eq!(payload.login_id, Some(login_id));
    assert_eq!(payload.success, false);
    assert!(
        payload
            .error
            .as_deref()
            .is_some_and(|error| error.contains("device auth failed with status")),
        "unexpected error: {:?}",
        payload.error
    );

    let maybe_updated = timeout(
        Duration::from_millis(500),
        mcp.read_stream_until_notification_message("account/updated"),
    )
    .await;
    assert!(
        maybe_updated.is_err(),
        "account/updated should not be emitted when device code login fails"
    );
    assert!(
        !codex_home.path().join("auth.json").exists(),
        "auth.json should not be created when device code login fails"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "Astral disables ChatGPT device code login"]
async fn login_account_chatgpt_device_code_can_be_cancelled() -> Result<()> {
    let codex_home = TempDir::new()?;
    let mock_server = MockServer::start().await;
    create_config_toml(
        codex_home.path(),
        CreateConfigTomlParams {
            requires_openai_auth: Some(true),
            base_url: Some(format!("{}/v1", mock_server.uri())),
            ..Default::default()
        },
    )?;
    write_models_cache(codex_home.path())?;

    mock_device_code_usercode(&mock_server, /*interval_seconds*/ 1).await;
    mock_device_code_token_failure(&mock_server, /*status*/ 404).await;

    let issuer = mock_server.uri();
    let mut mcp = TestAppServer::new_with_env(
        codex_home.path(),
        &[
            ("ASTRAL_API_KEY", None),
            (LOGIN_ISSUER_ENV_VAR, Some(issuer.as_str())),
        ],
    )
    .await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let request_id = mcp.send_login_account_chatgpt_device_code_request().await?;
    let resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let login: LoginAccountResponse = to_response(resp)?;
    let LoginAccountResponse::ChatgptDeviceCode { login_id, .. } = login else {
        bail!("unexpected login response: {login:?}");
    };

    let cancel_id = mcp
        .send_cancel_login_account_request(CancelLoginAccountParams {
            login_id: login_id.clone(),
        })
        .await?;
    let cancel_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(cancel_id)),
    )
    .await??;
    let cancel: CancelLoginAccountResponse = to_response(cancel_resp)?;
    assert_eq!(cancel.status, CancelLoginAccountStatus::Canceled);

    let note = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("account/login/completed"),
    )
    .await??;
    let parsed: ServerNotification = note.try_into()?;
    let ServerNotification::AccountLoginCompleted(payload) = parsed else {
        bail!("unexpected notification: {parsed:?}");
    };
    assert_eq!(payload.login_id, Some(login_id));
    assert_eq!(payload.success, false);
    assert!(
        payload.error.is_some(),
        "expected a non-empty error on device code cancel"
    );

    let maybe_updated = timeout(
        Duration::from_millis(500),
        mcp.read_stream_until_notification_message("account/updated"),
    )
    .await;
    assert!(
        maybe_updated.is_err(),
        "account/updated should not be emitted when device code login is cancelled"
    );
    assert!(
        !codex_home.path().join("auth.json").exists(),
        "auth.json should not be created when device code login is cancelled"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "Astral disables ChatGPT browser login"]
// Serialize tests that launch the login server since it binds to a fixed port.
#[serial(login_port)]
async fn login_account_chatgpt_start_can_be_cancelled() -> Result<()> {
    let codex_home = TempDir::new()?;
    create_config_toml(codex_home.path(), CreateConfigTomlParams::default())?;

    let mut mcp = TestAppServer::new(codex_home.path()).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let request_id = mcp.send_login_account_chatgpt_request().await?;
    let resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;

    let login: LoginAccountResponse = to_response(resp)?;
    let LoginAccountResponse::Chatgpt { login_id, auth_url } = login else {
        bail!("unexpected login response: {login:?}");
    };
    assert!(
        auth_url.contains("redirect_uri=http%3A%2F%2Flocalhost"),
        "auth_url should contain a redirect_uri to localhost"
    );

    let cancel_id = mcp
        .send_cancel_login_account_request(CancelLoginAccountParams {
            login_id: login_id.clone(),
        })
        .await?;
    let cancel_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(cancel_id)),
    )
    .await??;
    let _ok: CancelLoginAccountResponse = to_response(cancel_resp)?;

    let note = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("account/login/completed"),
    )
    .await??;
    let parsed: ServerNotification = note.try_into()?;
    let ServerNotification::AccountLoginCompleted(payload) = parsed else {
        bail!("unexpected notification: {parsed:?}");
    };
    pretty_assertions::assert_eq!(payload.login_id, Some(login_id));
    pretty_assertions::assert_eq!(payload.success, false);
    assert!(
        payload.error.is_some(),
        "expected a non-empty error on cancel"
    );

    let maybe_updated = timeout(
        Duration::from_millis(500),
        mcp.read_stream_until_notification_message("account/updated"),
    )
    .await;
    assert!(
        maybe_updated.is_err(),
        "account/updated should not be emitted when login is cancelled"
    );
    Ok(())
}

#[tokio::test]
async fn get_account_no_auth() -> Result<()> {
    let codex_home = TempDir::new()?;
    create_config_toml(
        codex_home.path(),
        CreateConfigTomlParams {
            requires_openai_auth: Some(true),
            ..Default::default()
        },
    )?;

    let mut mcp =
        TestAppServer::new_with_env(codex_home.path(), &[("ASTRAL_API_KEY", None)]).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let params = GetAccountParams {
        refresh_token: false,
    };
    let request_id = mcp.send_get_account_request(params).await?;

    let resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let account: GetAccountResponse = to_response(resp)?;

    assert_eq!(account.account, None, "expected no account");
    assert_eq!(account.requires_openai_auth, true);
    Ok(())
}

#[tokio::test]
async fn get_account_with_api_key() -> Result<()> {
    let codex_home = TempDir::new()?;
    create_config_toml(
        codex_home.path(),
        CreateConfigTomlParams {
            requires_openai_auth: Some(true),
            ..Default::default()
        },
    )?;

    let mut mcp = TestAppServer::new(codex_home.path()).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let req_id = mcp
        .send_login_account_api_key_request("sk-test-key")
        .await?;
    let resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(req_id)),
    )
    .await??;
    let _login_ok = to_response::<LoginAccountResponse>(resp)?;

    let params = GetAccountParams {
        refresh_token: false,
    };
    let request_id = mcp.send_get_account_request(params).await?;

    let resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let received: GetAccountResponse = to_response(resp)?;

    let expected = GetAccountResponse {
        account: Some(Account::ApiKey {}),
        requires_openai_auth: true,
    };
    assert_eq!(received, expected);
    Ok(())
}

#[tokio::test]
async fn get_account_when_auth_not_required() -> Result<()> {
    let codex_home = TempDir::new()?;
    create_config_toml(
        codex_home.path(),
        CreateConfigTomlParams {
            requires_openai_auth: Some(false),
            ..Default::default()
        },
    )?;

    let mut mcp = TestAppServer::new(codex_home.path()).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let params = GetAccountParams {
        refresh_token: false,
    };
    let request_id = mcp.send_get_account_request(params).await?;

    let resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let received: GetAccountResponse = to_response(resp)?;

    let expected = GetAccountResponse {
        account: None,
        requires_openai_auth: false,
    };
    assert_eq!(received, expected);
    Ok(())
}

#[tokio::test]
async fn get_account_with_aws_provider() -> Result<()> {
    let codex_home = TempDir::new()?;
    create_config_toml(
        codex_home.path(),
        CreateConfigTomlParams {
            model_provider_id: Some("amazon-bedrock".to_string()),
            extra_provider_config: Some(
                r#"[model_providers.amazon-bedrock.aws]
profile = "codex-bedrock"
region = "us-west-2"
"#
                .to_string(),
            ),
            ..Default::default()
        },
    )?;

    let mut mcp = TestAppServer::new(codex_home.path()).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let params = GetAccountParams {
        refresh_token: false,
    };
    let request_id = mcp.send_get_account_request(params).await?;

    let resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let received: GetAccountResponse = to_response(resp)?;

    let expected = GetAccountResponse {
        account: Some(Account::AmazonBedrock {}),
        requires_openai_auth: false,
    };
    assert_eq!(received, expected);
    Ok(())
}

#[tokio::test]
async fn get_account_with_chatgpt() -> Result<()> {
    let codex_home = TempDir::new()?;
    create_config_toml(
        codex_home.path(),
        CreateConfigTomlParams {
            requires_openai_auth: Some(true),
            ..Default::default()
        },
    )?;
    write_chatgpt_auth(
        codex_home.path(),
        ChatGptAuthFixture::new("access-chatgpt")
            .email("user@example.com")
            .plan_type("pro"),
        AuthCredentialsStoreMode::File,
    )?;

    let mut mcp =
        TestAppServer::new_with_env(codex_home.path(), &[("ASTRAL_API_KEY", None)]).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let params = GetAccountParams {
        refresh_token: false,
    };
    let request_id = mcp.send_get_account_request(params).await?;

    let resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let received: GetAccountResponse = to_response(resp)?;

    let expected = GetAccountResponse {
        account: Some(Account::Chatgpt {
            email: "user@example.com".to_string(),
            plan_type: AccountPlanType::Pro,
        }),
        requires_openai_auth: true,
    };
    assert_eq!(received, expected);
    Ok(())
}

#[tokio::test]
async fn get_account_omits_chatgpt_after_permanent_refresh_failure() -> Result<()> {
    let codex_home = TempDir::new()?;
    create_config_toml(
        codex_home.path(),
        CreateConfigTomlParams {
            requires_openai_auth: Some(true),
            ..Default::default()
        },
    )?;
    write_chatgpt_auth(
        codex_home.path(),
        ChatGptAuthFixture::new("stale-access-token")
            .refresh_token("stale-refresh-token")
            .account_id(WORKSPACE_ID_STALE)
            .email("user@example.com")
            .plan_type("pro")
            .last_refresh(Some(Utc::now() - ChronoDuration::days(9))),
        AuthCredentialsStoreMode::File,
    )?;

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": {
                "code": "refresh_token_reused"
            }
        })))
        .expect(1..=2)
        .mount(&server)
        .await;

    let refresh_url = format!("{}/oauth/token", server.uri());
    let mut mcp = TestAppServer::new_with_env(
        codex_home.path(),
        &[
            ("ASTRAL_API_KEY", None),
            (
                REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR,
                Some(refresh_url.as_str()),
            ),
        ],
    )
    .await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let auth_status_request_id = mcp
        .send_get_auth_status_request(GetAuthStatusParams {
            include_token: Some(true),
            refresh_token: Some(true),
        })
        .await?;
    let auth_status_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(auth_status_request_id)),
    )
    .await??;
    let _: GetAuthStatusResponse = to_response(auth_status_resp)?;

    let request_id = mcp
        .send_get_account_request(GetAccountParams {
            refresh_token: false,
        })
        .await?;

    let resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let received: GetAccountResponse = to_response(resp)?;

    assert_eq!(
        received,
        GetAccountResponse {
            account: None,
            requires_openai_auth: true,
        }
    );
    server.verify().await;
    Ok(())
}

#[tokio::test]
async fn get_account_with_chatgpt_missing_plan_claim_returns_unknown() -> Result<()> {
    let codex_home = TempDir::new()?;
    create_config_toml(
        codex_home.path(),
        CreateConfigTomlParams {
            requires_openai_auth: Some(true),
            ..Default::default()
        },
    )?;
    write_chatgpt_auth(
        codex_home.path(),
        ChatGptAuthFixture::new("access-chatgpt").email("user@example.com"),
        AuthCredentialsStoreMode::File,
    )?;

    let mut mcp =
        TestAppServer::new_with_env(codex_home.path(), &[("ASTRAL_API_KEY", None)]).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let params = GetAccountParams {
        refresh_token: false,
    };
    let request_id = mcp.send_get_account_request(params).await?;

    let resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let received: GetAccountResponse = to_response(resp)?;

    let expected = GetAccountResponse {
        account: Some(Account::Chatgpt {
            email: "user@example.com".to_string(),
            plan_type: AccountPlanType::Unknown,
        }),
        requires_openai_auth: true,
    };
    assert_eq!(received, expected);
    Ok(())
}
