use anyhow::Result;
use base64::Engine;
use codex_app_server_protocol::AuthMode;
use codex_config::types::AuthCredentialsStoreMode;
use codex_login::AuthDotJson;
use codex_login::AuthManager;
use codex_login::logout_with_revoke;
use codex_login::save_auth;
use codex_login::token_data::IdTokenInfo;
use codex_login::token_data::TokenData;
use tempfile::TempDir;

const ACCESS_TOKEN: &str = "access-token";
const REFRESH_TOKEN: &str = "refresh-token";

#[tokio::test]
async fn logout_with_revoke_removes_legacy_auth_without_remote_revoke() -> Result<()> {
    let codex_home = TempDir::new()?;
    save_auth(
        codex_home.path(),
        &chatgpt_auth(),
        AuthCredentialsStoreMode::File,
    )?;

    let removed = logout_with_revoke(codex_home.path(), AuthCredentialsStoreMode::File).await?;

    assert!(removed);
    assert!(!codex_home.path().join("auth.json").exists());
    Ok(())
}

#[tokio::test]
async fn logout_with_revoke_returns_false_when_no_auth_exists() -> Result<()> {
    let codex_home = TempDir::new()?;

    let removed = logout_with_revoke(codex_home.path(), AuthCredentialsStoreMode::File).await?;

    assert!(!removed);
    Ok(())
}

#[tokio::test]
async fn auth_manager_logout_with_revoke_clears_cached_auth() -> Result<()> {
    let codex_home = TempDir::new()?;
    save_auth(
        codex_home.path(),
        &chatgpt_auth_with_refresh_token(REFRESH_TOKEN),
        AuthCredentialsStoreMode::File,
    )?;
    let manager = AuthManager::new(
        codex_home.path().to_path_buf(),
        /*enable_codex_api_key_env*/ false,
        AuthCredentialsStoreMode::File,
    )
    .await;
    save_auth(
        codex_home.path(),
        &chatgpt_auth_with_refresh_token("newer-disk-refresh-token"),
        AuthCredentialsStoreMode::File,
    )?;

    let removed = manager.logout_with_revoke().await?;

    assert!(removed);
    assert!(manager.auth_cached().is_none());
    assert!(!codex_home.path().join("auth.json").exists());
    Ok(())
}

fn chatgpt_auth() -> AuthDotJson {
    chatgpt_auth_with_refresh_token(REFRESH_TOKEN)
}

fn chatgpt_auth_with_refresh_token(refresh_token: &str) -> AuthDotJson {
    AuthDotJson {
        auth_mode: Some(AuthMode::Chatgpt),
        api_key: None,
        tokens: Some(TokenData {
            id_token: IdTokenInfo {
                raw_jwt: minimal_jwt(),
                ..Default::default()
            },
            access_token: ACCESS_TOKEN.to_string(),
            refresh_token: refresh_token.to_string(),
            account_id: Some("account-id".to_string()),
        }),
        last_refresh: None,
        agent_identity: None,
        personal_access_token: None,
    }
}

fn minimal_jwt() -> String {
    let b64 = |b: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
    let header_b64 = b64(br#"{"alg":"none"}"#);
    let payload_b64 = b64(br#"{"sub":"user-123"}"#);
    let signature_b64 = b64(b"sig");
    format!("{header_b64}.{payload_b64}.{signature_b64}")
}
