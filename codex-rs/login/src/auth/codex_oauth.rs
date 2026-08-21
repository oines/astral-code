//! Provider-scoped ChatGPT OAuth persistence and refresh for the built-in Codex provider.

use chrono::Duration as ChronoDuration;
use chrono::Utc;
use codex_client::CodexHttpClient;
use codex_config::types::AuthCredentialsStoreMode;
use codex_protocol::auth::RefreshTokenFailedError;
use codex_protocol::auth::RefreshTokenFailedReason;
use reqwest::StatusCode;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;
use tokio::sync::Semaphore;

use super::manager::ChatgptAuth;
use super::manager::CodexAuth;
use super::manager::RefreshTokenError;
use super::storage::AuthDotJson;
use super::storage::AuthStorageBackend;
use super::storage::create_codex_oauth_storage;
use crate::default_client::create_client;
use crate::token_data::TokenData;
use crate::token_data::parse_chatgpt_jwt_claims;
use crate::token_data::parse_jwt_expiration;

pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const REFRESH_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const REVOKE_TOKEN_URL: &str = "https://auth.openai.com/oauth/revoke";
pub const REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR: &str = "CODEX_REFRESH_TOKEN_URL_OVERRIDE";
pub const REVOKE_TOKEN_URL_OVERRIDE_ENV_VAR: &str = "CODEX_REVOKE_TOKEN_URL_OVERRIDE";

const ACCESS_TOKEN_REFRESH_WINDOW_MINUTES: i64 = 5;
const REVOKE_HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const REFRESH_TOKEN_EXPIRED_MESSAGE: &str = "Your access token could not be refreshed because your refresh token has expired. Please log out and sign in again.";
const REFRESH_TOKEN_REUSED_MESSAGE: &str = "Your access token could not be refreshed because your refresh token was already used. Please log out and sign in again.";
const REFRESH_TOKEN_INVALIDATED_MESSAGE: &str = "Your access token could not be refreshed because your refresh token was revoked. Please log out and sign in again.";
const REFRESH_TOKEN_UNKNOWN_MESSAGE: &str =
    "Your access token could not be refreshed. Please log out and sign in again.";
const REFRESH_TOKEN_ACCOUNT_MISMATCH_MESSAGE: &str = "Your access token could not be refreshed because you have since logged out or signed in to another account. Please sign in again.";

#[derive(Debug)]
pub(crate) struct CodexOAuthManager {
    codex_home: PathBuf,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
    auth: RwLock<Option<CodexAuth>>,
    permanent_refresh_failure: RwLock<Option<(Option<String>, RefreshTokenFailedError)>>,
    refresh_lock: Semaphore,
    refresh_token_url: String,
}

impl CodexOAuthManager {
    pub(crate) fn empty_for_testing() -> Self {
        Self {
            codex_home: PathBuf::from("non-existent"),
            auth_credentials_store_mode: AuthCredentialsStoreMode::File,
            auth: RwLock::new(None),
            permanent_refresh_failure: RwLock::new(None),
            refresh_lock: Semaphore::new(/*permits*/ 1),
            refresh_token_url: REFRESH_TOKEN_URL.to_string(),
        }
    }

    pub(crate) async fn new(
        codex_home: PathBuf,
        auth_credentials_store_mode: AuthCredentialsStoreMode,
    ) -> Self {
        Self::new_with_refresh_token_url(
            codex_home,
            auth_credentials_store_mode,
            refresh_token_endpoint(),
        )
        .await
    }

    async fn new_with_refresh_token_url(
        codex_home: PathBuf,
        auth_credentials_store_mode: AuthCredentialsStoreMode,
        refresh_token_url: String,
    ) -> Self {
        let auth = load_codex_oauth(&codex_home, auth_credentials_store_mode)
            .ok()
            .flatten();
        Self {
            codex_home,
            auth_credentials_store_mode,
            auth: RwLock::new(auth),
            permanent_refresh_failure: RwLock::new(None),
            refresh_lock: Semaphore::new(/*permits*/ 1),
            refresh_token_url,
        }
    }

    pub(crate) fn auth_cached(&self) -> Option<CodexAuth> {
        self.auth.read().ok().and_then(|auth| auth.clone())
    }

    pub(crate) async fn auth(&self) -> Option<CodexAuth> {
        let auth = self.auth_cached();
        if auth.as_ref().is_some_and(should_refresh_proactively)
            && let Err(err) = self.refresh().await
        {
            tracing::warn!(reason = ?err.failed_reason(), "failed to proactively refresh Codex OAuth token");
        }
        self.auth_cached()
    }

    pub(crate) async fn reload(&self) -> bool {
        let new_auth = load_codex_oauth(&self.codex_home, self.auth_credentials_store_mode)
            .ok()
            .flatten();
        self.set_auth(new_auth)
    }

    pub(crate) fn refresh_failure_for_auth(
        &self,
        auth: &CodexAuth,
    ) -> Option<RefreshTokenFailedError> {
        let account_id = auth.get_account_id();
        self.permanent_refresh_failure
            .read()
            .ok()
            .and_then(|failure| failure.as_ref().cloned())
            .and_then(|(failed_account_id, error)| {
                (failed_account_id == account_id).then_some(error)
            })
    }

    pub(crate) async fn refresh(&self) -> Result<(), RefreshTokenError> {
        let _guard = self.refresh_lock.acquire().await.map_err(|_| {
            RefreshTokenError::Permanent(RefreshTokenFailedError::new(
                RefreshTokenFailedReason::Other,
                REFRESH_TOKEN_UNKNOWN_MESSAGE,
            ))
        })?;

        let before = self.auth_cached();
        let expected_account_id = before.as_ref().and_then(CodexAuth::get_account_id);
        let expected_access_token = before
            .as_ref()
            .and_then(CodexAuth::current_token_data)
            .map(|tokens| tokens.access_token);
        let stored = load_codex_oauth(&self.codex_home, self.auth_credentials_store_mode)
            .map_err(RefreshTokenError::Transient)?;
        let stored_account_id = stored.as_ref().and_then(CodexAuth::get_account_id);
        if expected_account_id.is_some() && stored_account_id != expected_account_id {
            return Err(RefreshTokenError::Permanent(RefreshTokenFailedError::new(
                RefreshTokenFailedReason::Other,
                REFRESH_TOKEN_ACCOUNT_MISMATCH_MESSAGE,
            )));
        }

        let stored_access_token = stored
            .as_ref()
            .and_then(CodexAuth::current_token_data)
            .map(|tokens| tokens.access_token);
        if stored_access_token != expected_access_token {
            self.set_auth(stored);
            return Ok(());
        }
        self.set_auth(stored);
        if self
            .auth_cached()
            .as_ref()
            .is_some_and(|auth| !should_refresh_proactively(auth))
        {
            return Ok(());
        }
        self.refresh_from_authority_locked().await
    }

    pub(crate) async fn refresh_from_authority(&self) -> Result<(), RefreshTokenError> {
        let _guard = self.refresh_lock.acquire().await.map_err(|_| {
            RefreshTokenError::Permanent(RefreshTokenFailedError::new(
                RefreshTokenFailedReason::Other,
                REFRESH_TOKEN_UNKNOWN_MESSAGE,
            ))
        })?;
        self.refresh_from_authority_locked().await
    }

    pub(crate) async fn refresh_from_authority_if_unchanged(
        &self,
        expected_account_id: Option<&str>,
        expected_access_token: Option<&str>,
    ) -> Result<(), RefreshTokenError> {
        let _guard = self.refresh_lock.acquire().await.map_err(|_| {
            RefreshTokenError::Permanent(RefreshTokenFailedError::new(
                RefreshTokenFailedReason::Other,
                REFRESH_TOKEN_UNKNOWN_MESSAGE,
            ))
        })?;
        let auth = self.auth_cached();
        let current_account_id = auth.as_ref().and_then(CodexAuth::get_account_id);
        if current_account_id.as_deref() != expected_account_id {
            return Err(RefreshTokenError::Permanent(RefreshTokenFailedError::new(
                RefreshTokenFailedReason::Other,
                REFRESH_TOKEN_ACCOUNT_MISMATCH_MESSAGE,
            )));
        }
        let current_access_token = auth
            .and_then(|auth| auth.current_token_data())
            .map(|tokens| tokens.access_token);
        if current_access_token.as_deref() != expected_access_token {
            return Ok(());
        }
        self.refresh_from_authority_locked().await
    }

    async fn refresh_from_authority_locked(&self) -> Result<(), RefreshTokenError> {
        let auth = match self.auth_cached() {
            Some(auth) => auth,
            None => return Ok(()),
        };
        if let Some(error) = self.refresh_failure_for_auth(&auth) {
            return Err(RefreshTokenError::Permanent(error));
        }
        let token_data = auth.current_token_data().ok_or_else(|| {
            RefreshTokenError::Transient(std::io::Error::other(
                "Codex OAuth token data is not available.",
            ))
        })?;
        if token_data.refresh_token.is_empty() {
            return Err(RefreshTokenError::Permanent(RefreshTokenFailedError::new(
                RefreshTokenFailedReason::Other,
                REFRESH_TOKEN_UNKNOWN_MESSAGE,
            )));
        }

        let result = request_token_refresh(
            token_data.refresh_token,
            &create_client(),
            &self.refresh_token_url,
        )
        .await;
        let refresh = match result {
            Ok(refresh) => refresh,
            Err(RefreshTokenError::Permanent(error)) => {
                if let Ok(mut failure) = self.permanent_refresh_failure.write() {
                    *failure = Some((auth.get_account_id(), error.clone()));
                }
                return Err(RefreshTokenError::Permanent(error));
            }
            Err(error) => return Err(error),
        };
        persist_refreshed_tokens(
            &self.storage(),
            refresh.id_token,
            refresh.access_token,
            refresh.refresh_token,
        )?;
        self.reload().await;
        Ok(())
    }

    pub(crate) async fn logout_with_revoke(&self) -> std::io::Result<bool> {
        let storage = self.storage();
        let stored = storage.load()?;
        if let Err(err) = revoke_auth_tokens(stored.as_ref()).await {
            tracing::warn!("failed to revoke Codex OAuth token during logout: {err}");
        }
        let removed =
            delete_all_codex_oauth_stores(&self.codex_home, self.auth_credentials_store_mode)?;
        self.set_auth(None);
        Ok(removed)
    }

    fn storage(&self) -> Arc<dyn AuthStorageBackend> {
        create_codex_oauth_storage(self.codex_home.clone(), self.auth_credentials_store_mode)
    }

    fn set_auth(&self, new_auth: Option<CodexAuth>) -> bool {
        let Ok(mut auth) = self.auth.write() else {
            return false;
        };
        let changed = auth.as_ref() != new_auth.as_ref();
        if changed && let Ok(mut failure) = self.permanent_refresh_failure.write() {
            *failure = None;
        }
        *auth = new_auth;
        changed
    }
}

pub fn load_codex_oauth_auth(
    codex_home: &Path,
    mode: AuthCredentialsStoreMode,
) -> std::io::Result<Option<AuthDotJson>> {
    create_codex_oauth_storage(codex_home.to_path_buf(), mode).load()
}

pub fn save_codex_oauth_auth(
    codex_home: &Path,
    auth: &AuthDotJson,
    mode: AuthCredentialsStoreMode,
) -> std::io::Result<()> {
    create_codex_oauth_storage(codex_home.to_path_buf(), mode).save(auth)
}

pub async fn revoke_superseded_codex_oauth_auth(
    previous: Option<&AuthDotJson>,
    replacement: &AuthDotJson,
) {
    if should_revoke(previous, replacement)
        && let Err(err) = revoke_auth_tokens(previous).await
    {
        tracing::warn!("failed to revoke superseded Codex OAuth token: {err}");
    }
}

fn load_codex_oauth(
    codex_home: &Path,
    mode: AuthCredentialsStoreMode,
) -> std::io::Result<Option<CodexAuth>> {
    let Some(auth) = load_codex_oauth_auth(codex_home, mode)? else {
        return Ok(None);
    };
    if auth.auth_mode.as_deref() != Some("chatgpt") {
        return Err(std::io::Error::other(
            "Codex OAuth storage contains an unsupported authentication mode.",
        ));
    }
    let tokens = auth
        .tokens
        .ok_or_else(|| std::io::Error::other("Codex OAuth storage is missing token data."))?;
    Ok(Some(CodexAuth::from_chatgpt_auth(ChatgptAuth::new(
        tokens,
        auth.last_refresh,
    ))))
}

fn persist_refreshed_tokens(
    storage: &Arc<dyn AuthStorageBackend>,
    id_token: Option<String>,
    access_token: Option<String>,
    refresh_token: Option<String>,
) -> Result<(), RefreshTokenError> {
    let mut auth = storage
        .load()
        .map_err(RefreshTokenError::Transient)?
        .ok_or_else(|| {
            RefreshTokenError::Transient(std::io::Error::other(
                "Codex OAuth token data is not available.",
            ))
        })?;
    let tokens = auth.tokens.get_or_insert_with(TokenData::default);
    if let Some(id_token) = id_token {
        tokens.id_token = parse_chatgpt_jwt_claims(&id_token)
            .map_err(std::io::Error::other)
            .map_err(RefreshTokenError::Transient)?;
    }
    if let Some(access_token) = access_token {
        tokens.access_token = access_token;
    }
    if let Some(refresh_token) = refresh_token {
        tokens.refresh_token = refresh_token;
    }
    auth.last_refresh = Some(Utc::now());
    storage.save(&auth).map_err(RefreshTokenError::Transient)
}

fn delete_all_codex_oauth_stores(
    codex_home: &Path,
    _mode: AuthCredentialsStoreMode,
) -> std::io::Result<bool> {
    let mut removed = false;
    let mut first_error = None;
    for mode in [
        AuthCredentialsStoreMode::Ephemeral,
        AuthCredentialsStoreMode::File,
        AuthCredentialsStoreMode::Keyring,
    ] {
        match create_codex_oauth_storage(codex_home.to_path_buf(), mode).delete() {
            Ok(mode_removed) => removed |= mode_removed,
            Err(err) if first_error.is_none() => first_error = Some(err),
            Err(_) => {}
        }
    }
    match first_error {
        Some(err) => Err(err),
        None => Ok(removed),
    }
}

fn should_refresh_proactively(auth: &CodexAuth) -> bool {
    let Some(tokens) = auth.current_token_data() else {
        return false;
    };
    parse_jwt_expiration(&tokens.access_token)
        .ok()
        .flatten()
        .is_some_and(|expires_at| {
            expires_at <= Utc::now() + ChronoDuration::minutes(ACCESS_TOKEN_REFRESH_WINDOW_MINUTES)
        })
}

#[derive(Serialize)]
struct RefreshRequest {
    client_id: &'static str,
    grant_type: &'static str,
    refresh_token: String,
}

#[derive(Deserialize)]
struct RefreshResponse {
    id_token: Option<String>,
    access_token: Option<String>,
    refresh_token: Option<String>,
}

async fn request_token_refresh(
    refresh_token: String,
    client: &CodexHttpClient,
    refresh_token_url: &str,
) -> Result<RefreshResponse, RefreshTokenError> {
    let response = client
        .post(refresh_token_url)
        .header("Content-Type", "application/json")
        .json(&RefreshRequest {
            client_id: CLIENT_ID,
            grant_type: "refresh_token",
            refresh_token,
        })
        .send()
        .await
        .map_err(|err| RefreshTokenError::Transient(std::io::Error::other(err)))?;
    let status = response.status();
    if status.is_success() {
        return response
            .json::<RefreshResponse>()
            .await
            .map_err(|err| RefreshTokenError::Transient(std::io::Error::other(err)));
    }

    let body = response.text().await.unwrap_or_default();
    let code = extract_error_code(&body);
    let is_invalid_grant = status == StatusCode::BAD_REQUEST
        && code
            .as_deref()
            .is_some_and(|code| code.eq_ignore_ascii_case("invalid_grant"));
    let failed = classify_refresh_failure(&body);
    if status == StatusCode::UNAUTHORIZED
        || failed.reason != RefreshTokenFailedReason::Other
        || is_invalid_grant
    {
        return Err(RefreshTokenError::Permanent(failed));
    }
    Err(RefreshTokenError::Transient(std::io::Error::other(
        format!("failed to refresh Codex OAuth token: {status}"),
    )))
}

fn classify_refresh_failure(body: &str) -> RefreshTokenFailedError {
    let code = extract_error_code(body).map(|code| code.to_ascii_lowercase());
    let reason = match code.as_deref() {
        Some("refresh_token_expired") => RefreshTokenFailedReason::Expired,
        Some("refresh_token_reused") => RefreshTokenFailedReason::Exhausted,
        Some("refresh_token_invalidated") => RefreshTokenFailedReason::Revoked,
        _ => RefreshTokenFailedReason::Other,
    };
    let message = match reason {
        RefreshTokenFailedReason::Expired => REFRESH_TOKEN_EXPIRED_MESSAGE,
        RefreshTokenFailedReason::Exhausted => REFRESH_TOKEN_REUSED_MESSAGE,
        RefreshTokenFailedReason::Revoked => REFRESH_TOKEN_INVALIDATED_MESSAGE,
        RefreshTokenFailedReason::Other => REFRESH_TOKEN_UNKNOWN_MESSAGE,
    };
    RefreshTokenFailedError::new(reason, message)
}

fn extract_error_code(body: &str) -> Option<String> {
    let Value::Object(map) = serde_json::from_str::<Value>(body).ok()? else {
        return None;
    };
    match map.get("error") {
        Some(Value::Object(error)) => error
            .get("code")
            .and_then(Value::as_str)
            .map(str::to_string),
        Some(Value::String(code)) => Some(code.clone()),
        _ => map.get("code").and_then(Value::as_str).map(str::to_string),
    }
}

#[derive(Clone, Copy)]
enum RevokeTokenKind {
    Access,
    Refresh,
}

#[derive(Serialize)]
struct RevokeTokenRequest<'a> {
    token: &'a str,
    token_type_hint: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_id: Option<&'static str>,
}

async fn revoke_auth_tokens(auth: Option<&AuthDotJson>) -> std::io::Result<()> {
    let Some((token, kind)) = auth.and_then(revocable_token) else {
        return Ok(());
    };
    let (token_type_hint, client_id) = match kind {
        RevokeTokenKind::Access => ("access_token", None),
        RevokeTokenKind::Refresh => ("refresh_token", Some(CLIENT_ID)),
    };
    let response = create_client()
        .post(revoke_token_endpoint().as_str())
        .header("Content-Type", "application/json")
        .timeout(REVOKE_HTTP_TIMEOUT)
        .json(&RevokeTokenRequest {
            token,
            token_type_hint,
            client_id,
        })
        .send()
        .await
        .map_err(std::io::Error::other)?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "failed to revoke Codex OAuth token: {}",
            response.status()
        )))
    }
}

fn should_revoke(previous: Option<&AuthDotJson>, replacement: &AuthDotJson) -> bool {
    let Some((token, kind)) = previous.and_then(revocable_token) else {
        return false;
    };
    let Some(tokens) = replacement.tokens.as_ref() else {
        return true;
    };
    match kind {
        RevokeTokenKind::Access => tokens.access_token != token,
        RevokeTokenKind::Refresh => tokens.refresh_token != token,
    }
}

fn revocable_token(auth: &AuthDotJson) -> Option<(&str, RevokeTokenKind)> {
    if auth.auth_mode.as_deref() != Some("chatgpt") {
        return None;
    }
    let tokens = auth.tokens.as_ref()?;
    if !tokens.refresh_token.is_empty() {
        Some((tokens.refresh_token.as_str(), RevokeTokenKind::Refresh))
    } else if !tokens.access_token.is_empty() {
        Some((tokens.access_token.as_str(), RevokeTokenKind::Access))
    } else {
        None
    }
}

fn refresh_token_endpoint() -> String {
    std::env::var(REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR)
        .unwrap_or_else(|_| REFRESH_TOKEN_URL.to_string())
}

fn revoke_token_endpoint() -> String {
    if let Ok(endpoint) = std::env::var(REVOKE_TOKEN_URL_OVERRIDE_ENV_VAR) {
        return endpoint;
    }
    if let Ok(refresh_endpoint) = std::env::var(REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR)
        && let Ok(mut url) = url::Url::parse(&refresh_endpoint)
    {
        url.set_path("/oauth/revoke");
        url.set_query(None);
        return url.to_string();
    }
    REVOKE_TOKEN_URL.to_string()
}

#[cfg(test)]
#[path = "codex_oauth_tests.rs"]
mod tests;
