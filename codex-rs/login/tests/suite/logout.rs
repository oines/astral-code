use anyhow::Result;
use codex_config::types::AuthCredentialsStoreMode;
use codex_login::AuthDotJson;
use codex_login::AuthManager;
use codex_login::logout_with_revoke;
use codex_login::save_auth;
use tempfile::TempDir;

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
        &chatgpt_auth(),
        AuthCredentialsStoreMode::File,
    )?;
    let manager = AuthManager::new(
        codex_home.path().to_path_buf(),
        /*enable_astral_api_key_env*/ false,
        AuthCredentialsStoreMode::File,
    )
    .await;
    save_auth(
        codex_home.path(),
        &chatgpt_auth(),
        AuthCredentialsStoreMode::File,
    )?;

    let removed = manager.logout_with_revoke().await?;

    assert!(removed);
    assert!(manager.auth_cached().is_none());
    assert!(!codex_home.path().join("auth.json").exists());
    Ok(())
}

fn chatgpt_auth() -> AuthDotJson {
    AuthDotJson {
        auth_mode: Some("chatgpt".to_string()),
        api_key: None,
        tokens: None,
        last_refresh: None,
    }
}
