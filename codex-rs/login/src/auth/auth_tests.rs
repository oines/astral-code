use super::*;
use crate::auth::storage::FileAuthStorage;
use crate::auth::storage::get_auth_file;
use codex_app_server_protocol::AuthMode;

use base64::Engine;
use codex_protocol::config_types::ModelProviderAuthInfo;
use pretty_assertions::assert_eq;
use serde::Serialize;
use serde_json::json;
use tempfile::TempDir;
use tempfile::tempdir;

#[test]
fn login_with_api_key_overwrites_existing_auth_json() {
    let dir = tempdir().unwrap();
    let auth_path = dir.path().join("auth.json");
    let stale_auth = json!({
        "ASTRAL_API_KEY": "sk-old",
        "tokens": {
            "id_token": "stale.header.payload",
            "access_token": "stale-access",
            "refresh_token": "stale-refresh",
            "account_id": "stale-acc"
        }
    });
    std::fs::write(
        &auth_path,
        serde_json::to_string_pretty(&stale_auth).unwrap(),
    )
    .unwrap();

    super::login_with_api_key(dir.path(), "sk-new", AuthCredentialsStoreMode::File)
        .expect("login_with_api_key should succeed");

    let storage = FileAuthStorage::new(dir.path().to_path_buf());
    let auth = storage
        .try_read_auth_json(&auth_path)
        .expect("auth.json should parse");
    assert_eq!(auth.openai_api_key.as_deref(), Some("sk-new"));
    assert!(auth.tokens.is_none(), "tokens should be cleared");
}

#[tokio::test]
#[serial(codex_auth_env)]
async fn missing_auth_json_returns_none() {
    let dir = tempdir().unwrap();
    let auth = CodexAuth::from_auth_storage(
        dir.path(),
        AuthCredentialsStoreMode::File,
        /*chatgpt_base_url*/ None,
    )
    .await
    .expect("call should succeed");
    assert_eq!(auth, None);
}

#[tokio::test]
#[serial(codex_auth_env)]
async fn stored_chatgpt_auth_without_api_key_is_rejected() {
    let codex_home = tempdir().unwrap();
    write_auth_file(
        AuthFileParams {
            openai_api_key: None,
            chatgpt_plan_type: Some("pro".to_string()),
            chatgpt_account_id: None,
        },
        codex_home.path(),
    )
    .expect("failed to write auth file");

    let err = super::load_auth(
        codex_home.path(),
        /*enable_codex_api_key_env*/ false,
        AuthCredentialsStoreMode::File,
        /*chatgpt_base_url*/ None,
    )
    .await
    .expect_err("stored ChatGPT auth should be rejected");

    assert_eq!(err.to_string(), UNSUPPORTED_OPENAI_AUTH_MESSAGE);
}

#[tokio::test]
#[serial(codex_auth_env)]
async fn loads_api_key_from_auth_json() {
    let dir = tempdir().unwrap();
    let auth_file = dir.path().join("auth.json");
    std::fs::write(
        auth_file,
        r#"{"ASTRAL_API_KEY":"sk-test-key","tokens":null,"last_refresh":null}"#,
    )
    .unwrap();

    let auth = super::load_auth(
        dir.path(),
        /*enable_codex_api_key_env*/ false,
        AuthCredentialsStoreMode::File,
        /*chatgpt_base_url*/ None,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(auth.auth_mode(), AuthMode::ApiKey);
    assert_eq!(auth.api_key(), Some("sk-test-key"));

    assert!(auth.get_token_data().is_err());
}

#[test]
fn logout_removes_auth_file() -> Result<(), std::io::Error> {
    let dir = tempdir()?;
    let auth_dot_json = AuthDotJson {
        auth_mode: Some(ApiAuthMode::ApiKey),
        openai_api_key: Some("sk-test-key".to_string()),
        tokens: None,
        last_refresh: None,
        agent_identity: None,
        personal_access_token: None,
    };
    super::save_auth(dir.path(), &auth_dot_json, AuthCredentialsStoreMode::File)?;
    let auth_file = get_auth_file(dir.path());
    assert!(auth_file.exists());
    assert!(logout(dir.path(), AuthCredentialsStoreMode::File)?);
    assert!(!auth_file.exists());
    Ok(())
}

#[tokio::test]
async fn unauthorized_recovery_reports_mode_and_step_names() {
    let dir = tempdir().unwrap();
    let manager = AuthManager::shared(
        dir.path().to_path_buf(),
        /*enable_codex_api_key_env*/ false,
        AuthCredentialsStoreMode::File,
        /*chatgpt_base_url*/ None,
    )
    .await;
    let managed = manager.unauthorized_recovery();
    assert_eq!(managed.mode_name(), "managed");
    assert_eq!(managed.step_name(), "done");

    let external = UnauthorizedRecovery {
        manager,
        step: UnauthorizedRecoveryStep::ExternalRefresh,
        mode: UnauthorizedRecoveryMode::External,
    };
    assert_eq!(external.mode_name(), "external");
    assert_eq!(external.step_name(), "external_refresh");
}

#[tokio::test]
async fn external_bearer_only_auth_manager_uses_cached_provider_token() {
    let script = ProviderAuthScript::new(&["provider-token", "next-token"]).unwrap();
    let manager = AuthManager::external_bearer_only(script.auth_config());

    let first = manager
        .auth()
        .await
        .and_then(|auth| auth.api_key().map(str::to_string));
    let second = manager
        .auth()
        .await
        .and_then(|auth| auth.api_key().map(str::to_string));

    assert_eq!(first.as_deref(), Some("provider-token"));
    assert_eq!(second.as_deref(), Some("provider-token"));
    assert_eq!(manager.auth_mode(), Some(AuthMode::ApiKey));
    assert_eq!(manager.get_api_auth_mode(), Some(ApiAuthMode::ApiKey));
}

#[tokio::test]
async fn external_bearer_only_auth_manager_disables_auto_refresh_when_interval_is_zero() {
    let script = ProviderAuthScript::new(&["provider-token", "next-token"]).unwrap();
    let mut auth_config = script.auth_config();
    auth_config.refresh_interval_ms = 0;
    let manager = AuthManager::external_bearer_only(auth_config);

    let first = manager
        .auth()
        .await
        .and_then(|auth| auth.api_key().map(str::to_string));
    let second = manager
        .auth()
        .await
        .and_then(|auth| auth.api_key().map(str::to_string));

    assert_eq!(first.as_deref(), Some("provider-token"));
    assert_eq!(second.as_deref(), Some("provider-token"));
}

#[tokio::test]
async fn external_bearer_only_auth_manager_returns_none_when_command_fails() {
    let script = ProviderAuthScript::new_failing().unwrap();
    let manager = AuthManager::external_bearer_only(script.auth_config());

    assert_eq!(manager.auth().await, None);
}

#[tokio::test]
async fn unauthorized_recovery_uses_external_refresh_for_bearer_manager() {
    let script = ProviderAuthScript::new(&["provider-token", "refreshed-provider-token"]).unwrap();
    let mut auth_config = script.auth_config();
    auth_config.refresh_interval_ms = 0;
    let manager = AuthManager::external_bearer_only(auth_config);
    let initial_token = manager
        .auth()
        .await
        .and_then(|auth| auth.api_key().map(str::to_string));
    let mut recovery = manager.unauthorized_recovery();

    assert!(recovery.has_next());
    assert_eq!(recovery.mode_name(), "external");
    assert_eq!(recovery.step_name(), "external_refresh");

    let result = recovery
        .next()
        .await
        .expect("external refresh should succeed");

    assert_eq!(result.auth_state_changed(), Some(true));
    let refreshed_token = manager
        .auth()
        .await
        .and_then(|auth| auth.api_key().map(str::to_string));
    assert_eq!(initial_token.as_deref(), Some("provider-token"));
    assert_eq!(refreshed_token.as_deref(), Some("refreshed-provider-token"));
}

struct ProviderAuthScript {
    tempdir: TempDir,
    command: String,
    args: Vec<String>,
}

impl ProviderAuthScript {
    fn new(tokens: &[&str]) -> std::io::Result<Self> {
        let tempdir = tempfile::tempdir()?;
        let token_file = tempdir.path().join("tokens.txt");
        // `cmd.exe`'s `set /p` treats LF-only input as one line, so use CRLF on Windows.
        let token_line_ending = if cfg!(windows) { "\r\n" } else { "\n" };
        let mut token_file_contents = String::new();
        for token in tokens {
            token_file_contents.push_str(token);
            token_file_contents.push_str(token_line_ending);
        }
        std::fs::write(&token_file, token_file_contents)?;

        #[cfg(unix)]
        let (command, args) = {
            let script_path = tempdir.path().join("print-token.sh");
            std::fs::write(
                &script_path,
                r#"#!/bin/sh
first_line=$(sed -n '1p' tokens.txt)
printf '%s\n' "$first_line"
tail -n +2 tokens.txt > tokens.next
mv tokens.next tokens.txt
"#,
            )?;
            let mut permissions = std::fs::metadata(&script_path)?.permissions();
            {
                use std::os::unix::fs::PermissionsExt;
                permissions.set_mode(0o755);
            }
            std::fs::set_permissions(&script_path, permissions)?;
            ("./print-token.sh".to_string(), Vec::new())
        };

        #[cfg(windows)]
        let (command, args) = {
            let script_path = tempdir.path().join("print-token.cmd");
            std::fs::write(
                &script_path,
                r#"@echo off
setlocal EnableExtensions DisableDelayedExpansion
set "first_line="
<tokens.txt set /p "first_line="
if not defined first_line exit /b 1
setlocal EnableDelayedExpansion
echo(!first_line!
endlocal
more +1 tokens.txt > tokens.next
move /y tokens.next tokens.txt >nul
"#,
            )?;
            (
                "cmd.exe".to_string(),
                vec![
                    "/d".to_string(),
                    "/s".to_string(),
                    "/c".to_string(),
                    ".\\print-token.cmd".to_string(),
                ],
            )
        };

        Ok(Self {
            tempdir,
            command,
            args,
        })
    }

    fn new_failing() -> std::io::Result<Self> {
        let tempdir = tempfile::tempdir()?;

        #[cfg(unix)]
        let (command, args) = {
            let script_path = tempdir.path().join("fail.sh");
            std::fs::write(
                &script_path,
                r#"#!/bin/sh
exit 1
"#,
            )?;
            let mut permissions = std::fs::metadata(&script_path)?.permissions();
            {
                use std::os::unix::fs::PermissionsExt;
                permissions.set_mode(0o755);
            }
            std::fs::set_permissions(&script_path, permissions)?;
            ("./fail.sh".to_string(), Vec::new())
        };

        #[cfg(windows)]
        let (command, args) = (
            "cmd.exe".to_string(),
            vec![
                "/d".to_string(),
                "/s".to_string(),
                "/c".to_string(),
                "exit /b 1".to_string(),
            ],
        );

        Ok(Self {
            tempdir,
            command,
            args,
        })
    }

    fn auth_config(&self) -> ModelProviderAuthInfo {
        serde_json::from_value(json!({
            "command": self.command,
            "args": self.args,
            // Process startup can be slow on loaded Windows CI workers, so leave enough slack to
            // avoid turning these auth-cache assertions into a process-launch timing test.
            "timeout_ms": 10_000,
            "refresh_interval_ms": 60000,
            "cwd": self.tempdir.path(),
        }))
        .expect("provider auth config should deserialize")
    }
}

struct AuthFileParams {
    openai_api_key: Option<String>,
    chatgpt_plan_type: Option<String>,
    chatgpt_account_id: Option<String>,
}

fn write_auth_file(params: AuthFileParams, codex_home: &Path) -> std::io::Result<String> {
    let fake_jwt = fake_jwt_for_auth_file_params(&params)?;
    let auth_file = get_auth_file(codex_home);
    let auth_json_data = json!({
        "ASTRAL_API_KEY": params.openai_api_key,
        "tokens": {
            "id_token": fake_jwt,
            "access_token": "test-access-token",
            "refresh_token": "test-refresh-token"
        },
        "last_refresh": Utc::now(),
    });
    let auth_json = serde_json::to_string_pretty(&auth_json_data)?;
    std::fs::write(auth_file, auth_json)?;
    Ok(fake_jwt)
}

fn fake_jwt_for_auth_file_params(params: &AuthFileParams) -> std::io::Result<String> {
    #[derive(Serialize)]
    struct Header {
        alg: &'static str,
        typ: &'static str,
    }

    let header = Header {
        alg: "none",
        typ: "JWT",
    };
    let mut auth_payload = serde_json::json!({
        "chatgpt_user_id": "user-12345",
        "user_id": "user-12345",
    });

    if let Some(chatgpt_plan_type) = params.chatgpt_plan_type.as_ref() {
        auth_payload["chatgpt_plan_type"] = serde_json::Value::String(chatgpt_plan_type.clone());
    }

    if let Some(chatgpt_account_id) = params.chatgpt_account_id.as_ref() {
        auth_payload["chatgpt_account_id"] = serde_json::Value::String(chatgpt_account_id.clone());
    }

    let payload = serde_json::json!({
        "email": "user@example.com",
        "email_verified": true,
        "https://api.openai.com/auth": auth_payload,
    });
    let b64 = |b: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
    let header_b64 = b64(&serde_json::to_vec(&header)?);
    let payload_b64 = b64(&serde_json::to_vec(&payload)?);
    let signature_b64 = b64(b"sig");
    Ok(format!("{header_b64}.{payload_b64}.{signature_b64}"))
}

/// Use sparingly.
/// TODO (gpeal): replace this with an injectable env var provider.
#[cfg(test)]
struct EnvVarGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

#[cfg(test)]
impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original = env::var_os(key);
        unsafe {
            env::set_var(key, value);
        }
        Self { key, original }
    }
}

#[cfg(test)]
impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.original {
                Some(value) => env::set_var(self.key, value),
                None => env::remove_var(self.key),
            }
        }
    }
}

#[tokio::test]
#[serial(codex_auth_env)]
async fn load_auth_keeps_astral_api_key_env_precedence() {
    let codex_home = tempdir().unwrap();
    let _api_key_guard = EnvVarGuard::set(ASTRAL_API_KEY_ENV_VAR, "sk-env");

    let auth = super::load_auth(
        codex_home.path(),
        /*enable_codex_api_key_env*/ true,
        AuthCredentialsStoreMode::File,
        /*chatgpt_base_url*/ None,
    )
    .await
    .expect("env auth should load")
    .expect("env auth should be present");

    assert_eq!(auth.api_key(), Some("sk-env"));
}
