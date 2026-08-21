use std::sync::Arc;

use base64::Engine;
use codex_config::types::AuthCredentialsStoreMode;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::tempdir;
use tokio::task::JoinSet;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

use super::super::storage::get_codex_oauth_file;
use super::AuthDotJson;
use super::CodexOAuthManager;
use super::RefreshTokenError;
use super::create_client;
use super::request_token_refresh;
use crate::auth::save_codex_oauth_auth;
use crate::token_data::TokenData;
use crate::token_data::parse_chatgpt_jwt_claims;

#[cfg(unix)]
#[test]
fn file_storage_is_separate_and_owner_only() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let home = tempdir()?;
    let auth = chatgpt_auth("account-1", "access", "refresh");
    save_codex_oauth_auth(home.path(), &auth, AuthCredentialsStoreMode::File)?;

    let auth_file = get_codex_oauth_file(home.path());
    assert_eq!(auth_file, home.path().join("auth/codex.json"));
    assert_eq!(
        std::fs::metadata(auth_file)?.permissions().mode() & 0o777,
        0o600
    );
    assert!(!home.path().join("auth.json").exists());
    Ok(())
}

#[tokio::test]
async fn concurrent_expired_tokens_refresh_once() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "rotated-access",
            "refresh_token": "rotated-refresh"
        })))
        .mount(&server)
        .await;

    let home = tempdir()?;
    let auth = chatgpt_auth("account-1", &jwt(json!({ "exp": 1 })), "refresh");
    save_codex_oauth_auth(home.path(), &auth, AuthCredentialsStoreMode::File)?;
    let manager = Arc::new(
        CodexOAuthManager::new_with_refresh_token_url(
            home.path().to_path_buf(),
            AuthCredentialsStoreMode::File,
            format!("{}/oauth/token", server.uri()),
        )
        .await,
    );

    let mut tasks = JoinSet::new();
    for _ in 0..8 {
        let manager = Arc::clone(&manager);
        tasks.spawn(async move { manager.auth().await });
    }
    while let Some(result) = tasks.join_next().await {
        assert!(result?.is_some());
    }

    assert_eq!(
        server
            .received_requests()
            .await
            .expect("mock server should retain received requests")
            .len(),
        1
    );
    let refreshed = manager
        .auth_cached()
        .expect("refreshed auth should be cached");
    let tokens = refreshed
        .current_token_data()
        .expect("refreshed token data should be cached");
    assert_eq!(tokens.access_token, "rotated-access");
    assert_eq!(tokens.refresh_token, "rotated-refresh");
    Ok(())
}

#[tokio::test]
async fn invalid_grant_is_permanent() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({ "error": "invalid_grant" })))
        .mount(&server)
        .await;

    let result = request_token_refresh(
        "refresh".to_string(),
        &create_client(),
        &format!("{}/oauth/token", server.uri()),
    )
    .await;
    assert!(matches!(result, Err(RefreshTokenError::Permanent(_))));
    Ok(())
}

fn chatgpt_auth(account_id: &str, access_token: &str, refresh_token: &str) -> AuthDotJson {
    let id_token = jwt(json!({
        "email": "user@example.com",
        "https://api.openai.com/auth": {
            "chatgpt_account_id": account_id,
            "chatgpt_plan_type": "plus"
        }
    }));
    AuthDotJson {
        auth_mode: Some("chatgpt".to_string()),
        api_key: None,
        tokens: Some(TokenData {
            id_token: parse_chatgpt_jwt_claims(&id_token).expect("test ID token should parse"),
            access_token: access_token.to_string(),
            refresh_token: refresh_token.to_string(),
            account_id: Some(account_id.to_string()),
        }),
        last_refresh: None,
    }
}

fn jwt(payload: serde_json::Value) -> String {
    let encode = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let header = encode(br#"{"alg":"none","typ":"JWT"}"#);
    let payload = encode(
        serde_json::to_string(&payload)
            .expect("test JWT payload should serialize")
            .as_bytes(),
    );
    let signature = encode(b"sig");
    format!("{header}.{payload}.{signature}")
}
