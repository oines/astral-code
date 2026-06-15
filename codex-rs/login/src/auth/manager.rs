use async_trait::async_trait;
#[cfg(test)]
use serial_test::serial;
use std::env;
use std::fmt::Debug;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;
use tokio::sync::Semaphore;
use tokio::sync::watch;

use codex_app_server_protocol::AuthMode;
use codex_app_server_protocol::AuthMode as ApiAuthMode;
use codex_protocol::config_types::ModelProviderAuthInfo;

use super::external_bearer::BearerTokenRefresher;
pub use crate::auth::storage::AuthDotJson;
use crate::auth::storage::create_auth_storage;
use codex_config::types::AuthCredentialsStoreMode;
use codex_protocol::auth::RefreshTokenFailedError;
use codex_protocol::auth::RefreshTokenFailedReason;
use thiserror::Error;

/// Authentication mechanism used by the current user.
#[derive(Debug, Clone)]
pub enum CodexAuth {
    ApiKey(ApiKeyAuth),
}

impl PartialEq for CodexAuth {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::ApiKey(a), Self::ApiKey(b)) => a.api_key == b.api_key,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApiKeyAuth {
    api_key: String,
}

const UNSUPPORTED_LEGACY_HOSTED_AUTH_MESSAGE: &str =
    "Stored upstream hosted credentials are not supported by Astral. Use API key auth instead.";

#[derive(Debug, Error)]
pub enum RefreshTokenError {
    #[error("{0}")]
    Permanent(#[from] RefreshTokenFailedError),
    #[error(transparent)]
    Transient(#[from] std::io::Error),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalAuthTokens {
    pub access_token: String,
}

impl ExternalAuthTokens {
    pub fn access_token_only(access_token: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalAuthRefreshReason {
    Unauthorized,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalAuthRefreshContext {
    pub reason: ExternalAuthRefreshReason,
    pub previous_account_id: Option<String>,
}

#[async_trait]
/// Pluggable auth provider used by `AuthManager` for externally managed auth flows.
///
/// Implementations may either resolve auth eagerly via `resolve()` or provide refreshed
/// credentials on demand via `refresh()`.
pub trait ExternalAuth: Send + Sync {
    /// Indicates which top-level auth mode this external provider supplies.
    fn auth_mode(&self) -> AuthMode;

    /// Returns cached or immediately available auth, if this provider can resolve it synchronously
    /// from the caller's perspective.
    async fn resolve(&self) -> std::io::Result<Option<ExternalAuthTokens>> {
        Ok(None)
    }

    /// Refreshes auth in response to a manager-driven refresh attempt.
    async fn refresh(
        &self,
        context: ExternalAuthRefreshContext,
    ) -> std::io::Result<ExternalAuthTokens>;
}

impl RefreshTokenError {
    pub fn failed_reason(&self) -> Option<RefreshTokenFailedReason> {
        match self {
            Self::Permanent(error) => Some(error.reason),
            Self::Transient(_) => None,
        }
    }
}

impl From<RefreshTokenError> for std::io::Error {
    fn from(err: RefreshTokenError) -> Self {
        match err {
            RefreshTokenError::Permanent(failed) => std::io::Error::other(failed),
            RefreshTokenError::Transient(inner) => inner,
        }
    }
}

impl CodexAuth {
    async fn from_auth_dot_json(
        _codex_home: &Path,
        auth_dot_json: AuthDotJson,
        _auth_credentials_store_mode: AuthCredentialsStoreMode,
    ) -> std::io::Result<Self> {
        let auth_mode = auth_dot_json.resolved_mode();
        match auth_mode {
            StoredAuthMode::ApiKey => {
                let Some(api_key) = auth_dot_json.api_key.as_deref() else {
                    return Err(std::io::Error::other("API key auth is missing a key."));
                };
                Ok(Self::from_api_key(api_key))
            }
            StoredAuthMode::LegacyHosted => Err(std::io::Error::other(
                UNSUPPORTED_LEGACY_HOSTED_AUTH_MESSAGE.to_string(),
            )),
        }
    }

    pub async fn from_auth_storage(
        codex_home: &Path,
        auth_credentials_store_mode: AuthCredentialsStoreMode,
    ) -> std::io::Result<Option<Self>> {
        load_auth(
            codex_home,
            /*enable_astral_api_key_env*/ false,
            auth_credentials_store_mode,
        )
        .await
    }

    pub fn auth_mode(&self) -> AuthMode {
        match self {
            Self::ApiKey(_) => AuthMode::ApiKey,
        }
    }

    pub fn api_auth_mode(&self) -> ApiAuthMode {
        match self {
            Self::ApiKey(_) => ApiAuthMode::ApiKey,
        }
    }

    pub fn is_api_key_auth(&self) -> bool {
        self.auth_mode() == AuthMode::ApiKey
    }

    /// Returns `None` if `auth_mode() != AuthMode::ApiKey`.
    pub fn api_key(&self) -> Option<&str> {
        match self {
            Self::ApiKey(auth) => Some(auth.api_key.as_str()),
        }
    }

    /// Returns the token string used for bearer authentication.
    pub fn get_token(&self) -> Result<String, std::io::Error> {
        match self {
            Self::ApiKey(auth) => Ok(auth.api_key.clone()),
        }
    }

    /// Consider this private to integration tests.
    pub fn create_dummy_api_key_auth_for_testing() -> Self {
        Self::from_api_key("test-api-key")
    }

    pub fn from_api_key(api_key: &str) -> Self {
        Self::ApiKey(ApiKeyAuth {
            api_key: api_key.to_owned(),
        })
    }
}

pub const ASTRAL_API_KEY_ENV_VAR: &str = "ASTRAL_API_KEY";

pub fn read_astral_api_key_from_env() -> Option<String> {
    read_non_empty_env_var(ASTRAL_API_KEY_ENV_VAR)
}

fn read_non_empty_env_var(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Delete the auth.json file inside `codex_home` if it exists. Returns `Ok(true)`
/// if a file was removed, `Ok(false)` if no auth file was present.
pub fn logout(
    codex_home: &Path,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
) -> std::io::Result<bool> {
    let storage = create_auth_storage(codex_home.to_path_buf(), auth_credentials_store_mode);
    storage.delete()
}

pub async fn logout_with_revoke(
    codex_home: &Path,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
) -> std::io::Result<bool> {
    AuthManager::new(
        codex_home.to_path_buf(),
        /*enable_astral_api_key_env*/ false,
        auth_credentials_store_mode,
    )
    .await
    .logout()
    .await
}

/// Persist the provided auth payload using the specified backend.
pub fn save_auth(
    codex_home: &Path,
    auth: &AuthDotJson,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
) -> std::io::Result<()> {
    let storage = create_auth_storage(codex_home.to_path_buf(), auth_credentials_store_mode);
    storage.save(auth)
}

/// Load the raw stored auth payload without applying environment overrides.
///
/// Returns `None` when no credentials are stored. Prefer `AuthManager` for
/// ordinary production reads; this helper is for tests and write-side
/// maintenance that must inspect the exact payload in storage.
pub fn load_auth_dot_json(
    codex_home: &Path,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
) -> std::io::Result<Option<AuthDotJson>> {
    let storage = create_auth_storage(codex_home.to_path_buf(), auth_credentials_store_mode);
    storage.load()
}

fn logout_all_stores(
    codex_home: &Path,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
) -> std::io::Result<bool> {
    if auth_credentials_store_mode == AuthCredentialsStoreMode::Ephemeral {
        return logout(codex_home, AuthCredentialsStoreMode::Ephemeral);
    }
    let removed_ephemeral = logout(codex_home, AuthCredentialsStoreMode::Ephemeral)?;
    let removed_managed = logout(codex_home, auth_credentials_store_mode)?;
    Ok(removed_ephemeral || removed_managed)
}

async fn load_auth(
    codex_home: &Path,
    enable_astral_api_key_env: bool,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
) -> std::io::Result<Option<CodexAuth>> {
    // API key via env var takes precedence over any other auth method.
    if enable_astral_api_key_env && let Some(api_key) = read_astral_api_key_from_env() {
        return Ok(Some(CodexAuth::from_api_key(api_key.as_str())));
    }

    // If the caller explicitly requested ephemeral auth, there is no persisted fallback.
    if auth_credentials_store_mode == AuthCredentialsStoreMode::Ephemeral {
        return Ok(None);
    }

    // Fall back to the configured persistent store (file/keyring/auto) for managed auth.
    let storage = create_auth_storage(codex_home.to_path_buf(), auth_credentials_store_mode);
    let auth_dot_json = match storage.load()? {
        Some(auth) => auth,
        None => return Ok(None),
    };

    let auth =
        CodexAuth::from_auth_dot_json(codex_home, auth_dot_json, auth_credentials_store_mode)
            .await?;
    Ok(Some(auth))
}

const API_KEY_AUTH_MODE: &str = "apikey";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StoredAuthMode {
    ApiKey,
    LegacyHosted,
}

impl AuthDotJson {
    fn resolved_mode(&self) -> StoredAuthMode {
        if let Some(mode) = self.auth_mode.as_deref() {
            return match mode {
                API_KEY_AUTH_MODE | "api_key" | "apiKey" => StoredAuthMode::ApiKey,
                _ => StoredAuthMode::LegacyHosted,
            };
        }
        if self.api_key.is_some() {
            return StoredAuthMode::ApiKey;
        }
        StoredAuthMode::ApiKey
    }
}

/// Internal cached auth state.
#[derive(Clone)]
struct CachedAuth {
    auth: Option<CodexAuth>,
}

impl Debug for CachedAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedAuth")
            .field(
                "auth_mode",
                &self.auth.as_ref().map(CodexAuth::api_auth_mode),
            )
            .finish()
    }
}

enum UnauthorizedRecoveryStep {
    ExternalRefresh,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnauthorizedRecoveryMode {
    Managed,
    External,
}

// UnauthorizedRecovery is a state machine that handles an attempt to refresh
// external bearer authentication when requests to the API fail with 401 status
// code. Astral does not support managed legacy-hosted/OAuth token refresh.
pub struct UnauthorizedRecovery {
    manager: Arc<AuthManager>,
    step: UnauthorizedRecoveryStep,
    mode: UnauthorizedRecoveryMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnauthorizedRecoveryStepResult {
    auth_state_changed: Option<bool>,
}

impl UnauthorizedRecoveryStepResult {
    pub fn auth_state_changed(&self) -> Option<bool> {
        self.auth_state_changed
    }
}

impl UnauthorizedRecovery {
    fn new(manager: Arc<AuthManager>) -> Self {
        let mode = if manager.has_external_api_key_auth() {
            UnauthorizedRecoveryMode::External
        } else {
            UnauthorizedRecoveryMode::Managed
        };
        let step = match mode {
            UnauthorizedRecoveryMode::Managed => UnauthorizedRecoveryStep::Done,
            UnauthorizedRecoveryMode::External => UnauthorizedRecoveryStep::ExternalRefresh,
        };
        Self {
            manager,
            step,
            mode,
        }
    }

    pub fn has_next(&self) -> bool {
        match self.mode {
            UnauthorizedRecoveryMode::External => {
                self.manager.has_external_api_key_auth()
                    && !matches!(self.step, UnauthorizedRecoveryStep::Done)
            }
            UnauthorizedRecoveryMode::Managed => false,
        }
    }

    pub fn unavailable_reason(&self) -> &'static str {
        match self.mode {
            UnauthorizedRecoveryMode::External => {
                if !self.manager.has_external_auth() {
                    return "no_external_auth";
                }
                if !self.manager.has_external_api_key_auth() {
                    return "not_refreshable_auth";
                }
                if matches!(self.step, UnauthorizedRecoveryStep::Done) {
                    return "recovery_exhausted";
                }
                "ready"
            }
            UnauthorizedRecoveryMode::Managed => {
                if matches!(self.step, UnauthorizedRecoveryStep::Done) {
                    return "recovery_exhausted";
                }
                "not_refreshable_auth"
            }
        }
    }

    pub fn mode_name(&self) -> &'static str {
        match self.mode {
            UnauthorizedRecoveryMode::Managed => "managed",
            UnauthorizedRecoveryMode::External => "external",
        }
    }

    pub fn step_name(&self) -> &'static str {
        match self.step {
            UnauthorizedRecoveryStep::ExternalRefresh => "external_refresh",
            UnauthorizedRecoveryStep::Done => "done",
        }
    }

    pub async fn next(&mut self) -> Result<UnauthorizedRecoveryStepResult, RefreshTokenError> {
        if !self.has_next() {
            return Err(RefreshTokenError::Permanent(RefreshTokenFailedError::new(
                RefreshTokenFailedReason::Other,
                "No more recovery steps available.",
            )));
        }

        match self.step {
            UnauthorizedRecoveryStep::ExternalRefresh => {
                self.manager
                    .refresh_external_auth(ExternalAuthRefreshReason::Unauthorized)
                    .await?;
                self.step = UnauthorizedRecoveryStep::Done;
                Ok(UnauthorizedRecoveryStepResult {
                    auth_state_changed: Some(true),
                })
            }
            UnauthorizedRecoveryStep::Done => Ok(UnauthorizedRecoveryStepResult {
                auth_state_changed: None,
            }),
        }
    }
}

/// Central manager providing a single source of truth for auth.json derived
/// authentication data. It loads once (or on preference change) and then
/// hands out cloned `CodexAuth` values so the rest of the program has a
/// consistent snapshot.
///
/// External modifications to `auth.json` will NOT be observed until
/// `reload()` is called explicitly. This matches the design goal of avoiding
/// different parts of the program seeing inconsistent auth data mid‑run.
pub struct AuthManager {
    codex_home: PathBuf,
    inner: RwLock<CachedAuth>,
    auth_change_tx: watch::Sender<u64>,
    enable_astral_api_key_env: bool,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
    refresh_lock: Semaphore,
    external_auth: RwLock<Option<Arc<dyn ExternalAuth>>>,
}

/// Configuration view required to construct a shared [`AuthManager`].
///
/// Implementations should return the auth-related config values for the
/// already-resolved runtime configuration. The primary implementation is
/// `codex_core::config::Config`, but this trait keeps `codex-login` independent
/// from `codex-core`.
pub trait AuthManagerConfig {
    /// Returns the Codex home directory used for auth storage.
    fn codex_home(&self) -> PathBuf;

    /// Returns the CLI auth credential storage mode for auth loading.
    fn cli_auth_credentials_store_mode(&self) -> AuthCredentialsStoreMode;
}

impl Debug for AuthManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthManager")
            .field("codex_home", &self.codex_home)
            .field("inner", &self.inner)
            .field("enable_astral_api_key_env", &self.enable_astral_api_key_env)
            .field(
                "auth_credentials_store_mode",
                &self.auth_credentials_store_mode,
            )
            .field("has_external_auth", &self.has_external_auth())
            .finish_non_exhaustive()
    }
}

impl AuthManager {
    /// Create a new manager loading the initial auth using the provided
    /// preferred auth method. Errors loading auth are swallowed; `auth()` will
    /// simply return `None` in that case so callers can treat it as an
    /// unauthenticated state.
    pub async fn new(
        codex_home: PathBuf,
        enable_astral_api_key_env: bool,
        auth_credentials_store_mode: AuthCredentialsStoreMode,
    ) -> Self {
        let managed_auth = load_auth(
            &codex_home,
            enable_astral_api_key_env,
            auth_credentials_store_mode,
        )
        .await
        .ok()
        .flatten();
        let (auth_change_tx, _auth_change_rx) = watch::channel(0);
        Self {
            codex_home,
            inner: RwLock::new(CachedAuth { auth: managed_auth }),
            auth_change_tx,
            enable_astral_api_key_env,
            auth_credentials_store_mode,
            refresh_lock: Semaphore::new(/*permits*/ 1),
            external_auth: RwLock::new(None),
        }
    }

    /// Create an AuthManager with a specific CodexAuth, for testing only.
    pub fn from_auth_for_testing(auth: CodexAuth) -> Arc<Self> {
        let cached = CachedAuth { auth: Some(auth) };
        let (auth_change_tx, _auth_change_rx) = watch::channel(0);

        Arc::new(Self {
            codex_home: PathBuf::from("non-existent"),
            inner: RwLock::new(cached),
            auth_change_tx,
            enable_astral_api_key_env: false,
            auth_credentials_store_mode: AuthCredentialsStoreMode::File,
            refresh_lock: Semaphore::new(/*permits*/ 1),
            external_auth: RwLock::new(None),
        })
    }

    /// Create an AuthManager with a specific CodexAuth and codex home, for testing only.
    pub fn from_auth_for_testing_with_home(auth: CodexAuth, codex_home: PathBuf) -> Arc<Self> {
        let cached = CachedAuth { auth: Some(auth) };
        let (auth_change_tx, _auth_change_rx) = watch::channel(0);
        Arc::new(Self {
            codex_home,
            inner: RwLock::new(cached),
            auth_change_tx,
            enable_astral_api_key_env: false,
            auth_credentials_store_mode: AuthCredentialsStoreMode::File,
            refresh_lock: Semaphore::new(/*permits*/ 1),
            external_auth: RwLock::new(None),
        })
    }

    pub fn external_bearer_only(config: ModelProviderAuthInfo) -> Arc<Self> {
        let (auth_change_tx, _auth_change_rx) = watch::channel(0);
        Arc::new(Self {
            codex_home: PathBuf::from("non-existent"),
            inner: RwLock::new(CachedAuth { auth: None }),
            auth_change_tx,
            enable_astral_api_key_env: false,
            auth_credentials_store_mode: AuthCredentialsStoreMode::File,
            refresh_lock: Semaphore::new(/*permits*/ 1),
            external_auth: RwLock::new(Some(
                Arc::new(BearerTokenRefresher::new(config)) as Arc<dyn ExternalAuth>
            )),
        })
    }

    /// Current cached auth (clone) without attempting a refresh.
    pub fn auth_cached(&self) -> Option<CodexAuth> {
        self.inner.read().ok().and_then(|c| c.auth.clone())
    }

    /// Subscribes to cached auth changes that can affect request recovery.
    pub fn auth_change_receiver(&self) -> watch::Receiver<u64> {
        self.auth_change_tx.subscribe()
    }

    pub fn refresh_failure_for_auth(&self, _auth: &CodexAuth) -> Option<RefreshTokenFailedError> {
        None
    }

    /// Current cached auth (clone). May be `None` if not logged in or load failed.
    pub async fn auth(&self) -> Option<CodexAuth> {
        if self.has_external_api_key_auth() {
            return self.resolve_external_api_key_auth().await;
        }

        self.auth_cached()
    }

    /// Force a reload of the auth information from auth.json. Returns
    /// whether the auth value changed.
    pub async fn reload(&self) -> bool {
        tracing::info!("Reloading auth");
        let new_auth = self.load_auth_from_storage().await;
        self.set_cached_auth(new_auth)
    }

    fn auths_equal_for_refresh(a: Option<&CodexAuth>, b: Option<&CodexAuth>) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => match (a.api_auth_mode(), b.api_auth_mode()) {
                (ApiAuthMode::ApiKey, ApiAuthMode::ApiKey) => a.api_key() == b.api_key(),
            },
            _ => false,
        }
    }

    fn auths_equal(a: Option<&CodexAuth>, b: Option<&CodexAuth>) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }

    async fn load_auth_from_storage(&self) -> Option<CodexAuth> {
        load_auth(
            &self.codex_home,
            self.enable_astral_api_key_env,
            self.auth_credentials_store_mode,
        )
        .await
        .ok()
        .flatten()
    }

    fn set_cached_auth(&self, new_auth: Option<CodexAuth>) -> bool {
        if let Ok(mut guard) = self.inner.write() {
            let previous = guard.auth.as_ref();
            let changed = !AuthManager::auths_equal(previous, new_auth.as_ref());
            let auth_changed_for_refresh =
                !Self::auths_equal_for_refresh(previous, new_auth.as_ref());
            tracing::info!("Reloaded auth, changed: {changed}");
            guard.auth = new_auth;
            if auth_changed_for_refresh {
                self.auth_change_tx.send_modify(|revision| *revision += 1);
            }
            changed
        } else {
            false
        }
    }

    pub fn set_external_auth(&self, external_auth: Arc<dyn ExternalAuth>) {
        if let Ok(mut guard) = self.external_auth.write() {
            *guard = Some(external_auth);
        }
    }

    pub fn clear_external_auth(&self) {
        if let Ok(mut guard) = self.external_auth.write() {
            *guard = None;
        }
    }

    pub fn has_external_auth(&self) -> bool {
        self.external_auth().is_some()
    }

    pub fn astral_api_key_env_enabled(&self) -> bool {
        self.enable_astral_api_key_env
    }

    /// Convenience constructor returning an `Arc` wrapper.
    pub async fn shared(
        codex_home: PathBuf,
        enable_astral_api_key_env: bool,
        auth_credentials_store_mode: AuthCredentialsStoreMode,
    ) -> Arc<Self> {
        Arc::new(
            Self::new(
                codex_home,
                enable_astral_api_key_env,
                auth_credentials_store_mode,
            )
            .await,
        )
    }

    /// Convenience constructor returning an `Arc` wrapper from resolved config.
    pub async fn shared_from_config(
        config: &impl AuthManagerConfig,
        enable_astral_api_key_env: bool,
    ) -> Arc<Self> {
        Self::shared(
            config.codex_home(),
            enable_astral_api_key_env,
            config.cli_auth_credentials_store_mode(),
        )
        .await
    }

    pub fn unauthorized_recovery(self: &Arc<Self>) -> UnauthorizedRecovery {
        UnauthorizedRecovery::new(Arc::clone(self))
    }

    fn external_auth(&self) -> Option<Arc<dyn ExternalAuth>> {
        self.external_auth
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().cloned())
    }

    fn external_auth_mode(&self) -> Option<AuthMode> {
        self.external_auth()
            .as_ref()
            .map(|external_auth| external_auth.auth_mode())
    }

    fn has_external_api_key_auth(&self) -> bool {
        self.external_auth_mode() == Some(AuthMode::ApiKey)
    }

    async fn resolve_external_api_key_auth(&self) -> Option<CodexAuth> {
        if !self.has_external_api_key_auth() {
            return None;
        }

        let external_auth = self.external_auth()?;

        match external_auth.resolve().await {
            Ok(Some(tokens)) => Some(CodexAuth::from_api_key(&tokens.access_token)),
            Ok(None) => None,
            Err(err) => {
                tracing::error!("Failed to resolve external API key auth: {err}");
                None
            }
        }
    }

    /// Refresh external bearer auth if a provider supplies it. Astral does not
    /// support managed legacy-hosted/OAuth token refresh.
    pub async fn refresh_token(&self) -> Result<(), RefreshTokenError> {
        let _refresh_guard = self.refresh_lock.acquire().await.map_err(|_| {
            RefreshTokenError::Transient(std::io::Error::other("auth refresh lock closed"))
        })?;
        if self.has_external_api_key_auth() {
            self.refresh_external_auth(ExternalAuthRefreshReason::Unauthorized)
                .await?;
        }
        Ok(())
    }

    /// Log out by deleting the on‑disk auth.json (if present). Returns Ok(true)
    /// if a file was removed, Ok(false) if no auth file existed. On success,
    /// reloads the in‑memory auth cache so callers immediately observe the
    /// unauthenticated state.
    pub async fn logout(&self) -> std::io::Result<bool> {
        let removed = logout_all_stores(&self.codex_home, self.auth_credentials_store_mode)?;
        // Always reload to clear any cached auth (even if file absent).
        self.reload().await;
        Ok(removed)
    }

    /// Compatibility wrapper for older logout call sites. Astral does not
    /// support token-backed hosted auth, so logout is local-only and never
    /// contacts a legacy OAuth revoke endpoint.
    pub async fn logout_with_revoke(&self) -> std::io::Result<bool> {
        self.logout().await
    }

    pub fn get_api_auth_mode(&self) -> Option<ApiAuthMode> {
        if self.has_external_api_key_auth() {
            return Some(ApiAuthMode::ApiKey);
        }
        self.auth_cached().as_ref().map(CodexAuth::api_auth_mode)
    }

    pub fn auth_mode(&self) -> Option<AuthMode> {
        if self.has_external_api_key_auth() {
            return Some(AuthMode::ApiKey);
        }
        self.auth_cached().as_ref().map(CodexAuth::auth_mode)
    }

    async fn refresh_external_auth(
        &self,
        reason: ExternalAuthRefreshReason,
    ) -> Result<(), RefreshTokenError> {
        let Some(external_auth) = self.external_auth() else {
            return Err(RefreshTokenError::Transient(std::io::Error::other(
                "external auth is not configured",
            )));
        };
        let context = ExternalAuthRefreshContext {
            reason,
            previous_account_id: None,
        };

        let _refreshed = external_auth
            .refresh(context)
            .await
            .map_err(RefreshTokenError::Transient)?;
        if external_auth.auth_mode() != AuthMode::ApiKey {
            return Err(RefreshTokenError::Transient(std::io::Error::other(
                "only external API key auth refresh is supported",
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
