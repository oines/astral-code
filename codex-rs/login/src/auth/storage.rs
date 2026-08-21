use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::collections::HashMap;
use std::fmt::Debug;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use tracing::warn;

use codex_config::types::AuthCredentialsStoreMode;
use codex_keyring_store::DefaultKeyringStore;
use codex_keyring_store::KeyringStore;
use codex_utils_path::write_atomically;
use once_cell::sync::Lazy;

use crate::token_data::TokenData;

/// Expected structure for $ASTRAL_HOME/auth.json.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct AuthDotJson {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_mode: Option<String>,

    #[serde(rename = "ASTRAL_API_KEY")]
    pub api_key: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenData>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_refresh: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuthStorageNamespace {
    Astral,
    CodexOAuth,
}

impl AuthStorageNamespace {
    fn file_path(self, codex_home: &Path) -> PathBuf {
        match self {
            Self::Astral => codex_home.join("auth.json"),
            Self::CodexOAuth => codex_home.join("auth").join("codex.json"),
        }
    }

    fn key_prefix(self) -> &'static str {
        match self {
            Self::Astral => "cli",
            Self::CodexOAuth => "codex|cli",
        }
    }
}

#[cfg(test)]
pub(super) fn get_auth_file(codex_home: &Path) -> PathBuf {
    AuthStorageNamespace::Astral.file_path(codex_home)
}

#[cfg(test)]
pub fn get_codex_oauth_file(codex_home: &Path) -> PathBuf {
    AuthStorageNamespace::CodexOAuth.file_path(codex_home)
}

fn delete_file_if_exists(
    codex_home: &Path,
    namespace: AuthStorageNamespace,
) -> std::io::Result<bool> {
    let auth_file = namespace.file_path(codex_home);
    match std::fs::remove_file(&auth_file) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

pub(super) trait AuthStorageBackend: Debug + Send + Sync {
    fn load(&self) -> std::io::Result<Option<AuthDotJson>>;
    fn save(&self, auth: &AuthDotJson) -> std::io::Result<()>;
    fn delete(&self) -> std::io::Result<bool>;
}

#[derive(Clone, Debug)]
pub(super) struct FileAuthStorage {
    codex_home: PathBuf,
    namespace: AuthStorageNamespace,
}

impl FileAuthStorage {
    #[cfg(test)]
    fn new(codex_home: PathBuf) -> Self {
        Self::new_namespaced(codex_home, AuthStorageNamespace::Astral)
    }

    fn new_namespaced(codex_home: PathBuf, namespace: AuthStorageNamespace) -> Self {
        Self {
            codex_home,
            namespace,
        }
    }

    /// Attempt to read and parse the `auth.json` file in the given `ASTRAL_HOME` directory.
    /// Returns the full AuthDotJson structure.
    pub(super) fn try_read_auth_json(&self, auth_file: &Path) -> std::io::Result<AuthDotJson> {
        let mut file = File::open(auth_file)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let auth_dot_json: AuthDotJson = serde_json::from_str(&contents)?;

        Ok(auth_dot_json)
    }
}

impl AuthStorageBackend for FileAuthStorage {
    fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
        let auth_file = self.namespace.file_path(&self.codex_home);
        let auth_dot_json = match self.try_read_auth_json(&auth_file) {
            Ok(auth) => auth,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        Ok(Some(auth_dot_json))
    }

    fn save(&self, auth_dot_json: &AuthDotJson) -> std::io::Result<()> {
        let auth_file = self.namespace.file_path(&self.codex_home);

        if let Some(parent) = auth_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json_data = serde_json::to_string_pretty(auth_dot_json)?;
        write_atomically(&auth_file, &json_data)?;
        #[cfg(unix)]
        if self.namespace == AuthStorageNamespace::CodexOAuth {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&auth_file, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    fn delete(&self) -> std::io::Result<bool> {
        delete_file_if_exists(&self.codex_home, self.namespace)
    }
}

const KEYRING_SERVICE: &str = "Astral Auth";

// Turns the Astral home path into a stable, short key string.
fn compute_namespaced_store_key(
    codex_home: &Path,
    namespace: AuthStorageNamespace,
) -> std::io::Result<String> {
    let canonical = codex_home
        .canonicalize()
        .unwrap_or_else(|_| codex_home.to_path_buf());
    let path_str = canonical.to_string_lossy();
    let mut hasher = Sha256::new();
    hasher.update(path_str.as_bytes());
    let digest = hasher.finalize();
    let hex = format!("{digest:x}");
    let truncated = hex.get(..16).unwrap_or(&hex);
    let prefix = namespace.key_prefix();
    Ok(format!("{prefix}|{truncated}"))
}

#[cfg(test)]
fn compute_store_key(codex_home: &Path) -> std::io::Result<String> {
    compute_namespaced_store_key(codex_home, AuthStorageNamespace::Astral)
}

#[derive(Clone, Debug)]
struct KeyringAuthStorage {
    codex_home: PathBuf,
    namespace: AuthStorageNamespace,
    keyring_store: Arc<dyn KeyringStore>,
}

impl KeyringAuthStorage {
    #[cfg(test)]
    fn new(codex_home: PathBuf, keyring_store: Arc<dyn KeyringStore>) -> Self {
        Self::new_namespaced(codex_home, AuthStorageNamespace::Astral, keyring_store)
    }

    fn new_namespaced(
        codex_home: PathBuf,
        namespace: AuthStorageNamespace,
        keyring_store: Arc<dyn KeyringStore>,
    ) -> Self {
        Self {
            codex_home,
            namespace,
            keyring_store,
        }
    }

    fn load_from_keyring(&self, key: &str) -> std::io::Result<Option<AuthDotJson>> {
        match self.keyring_store.load(KEYRING_SERVICE, key) {
            Ok(Some(serialized)) => serde_json::from_str(&serialized).map(Some).map_err(|err| {
                std::io::Error::other(format!(
                    "failed to deserialize CLI auth from keyring: {err}"
                ))
            }),
            Ok(None) => Ok(None),
            Err(error) => Err(std::io::Error::other(format!(
                "failed to load CLI auth from keyring: {}",
                error.message()
            ))),
        }
    }

    fn save_to_keyring(&self, key: &str, value: &str) -> std::io::Result<()> {
        match self.keyring_store.save(KEYRING_SERVICE, key, value) {
            Ok(()) => Ok(()),
            Err(error) => {
                let message = format!("failed to write auth data to keyring: {}", error.message());
                warn!("{message}");
                Err(std::io::Error::other(message))
            }
        }
    }
}

impl AuthStorageBackend for KeyringAuthStorage {
    fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
        let key = compute_namespaced_store_key(&self.codex_home, self.namespace)?;
        self.load_from_keyring(&key)
    }

    fn save(&self, auth: &AuthDotJson) -> std::io::Result<()> {
        let key = compute_namespaced_store_key(&self.codex_home, self.namespace)?;
        // Simpler error mapping per style: prefer method reference over closure
        let serialized = serde_json::to_string(auth).map_err(std::io::Error::other)?;
        self.save_to_keyring(&key, &serialized)?;
        if let Err(err) = delete_file_if_exists(&self.codex_home, self.namespace) {
            warn!("failed to remove CLI auth fallback file: {err}");
        }
        Ok(())
    }

    fn delete(&self) -> std::io::Result<bool> {
        let key = compute_namespaced_store_key(&self.codex_home, self.namespace)?;
        let keyring_removed = self
            .keyring_store
            .delete(KEYRING_SERVICE, &key)
            .map_err(|err| {
                std::io::Error::other(format!("failed to delete auth from keyring: {err}"))
            })?;
        let file_removed = delete_file_if_exists(&self.codex_home, self.namespace)?;
        Ok(keyring_removed || file_removed)
    }
}

#[derive(Clone, Debug)]
struct AutoAuthStorage {
    keyring_storage: Arc<KeyringAuthStorage>,
    file_storage: Arc<FileAuthStorage>,
}

impl AutoAuthStorage {
    #[cfg(test)]
    fn new(codex_home: PathBuf, keyring_store: Arc<dyn KeyringStore>) -> Self {
        Self::new_namespaced(codex_home, AuthStorageNamespace::Astral, keyring_store)
    }

    fn new_namespaced(
        codex_home: PathBuf,
        namespace: AuthStorageNamespace,
        keyring_store: Arc<dyn KeyringStore>,
    ) -> Self {
        Self {
            keyring_storage: Arc::new(KeyringAuthStorage::new_namespaced(
                codex_home.clone(),
                namespace,
                keyring_store,
            )),
            file_storage: Arc::new(FileAuthStorage::new_namespaced(codex_home, namespace)),
        }
    }
}

impl AuthStorageBackend for AutoAuthStorage {
    fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
        match self.keyring_storage.load() {
            Ok(Some(auth)) => Ok(Some(auth)),
            Ok(None) => self.file_storage.load(),
            Err(err) => {
                warn!("failed to load CLI auth from keyring, falling back to file storage: {err}");
                self.file_storage.load()
            }
        }
    }

    fn save(&self, auth: &AuthDotJson) -> std::io::Result<()> {
        match self.keyring_storage.save(auth) {
            Ok(()) => Ok(()),
            Err(err) => {
                warn!("failed to save auth to keyring, falling back to file storage: {err}");
                self.file_storage.save(auth)
            }
        }
    }

    fn delete(&self) -> std::io::Result<bool> {
        // Keyring storage will delete from disk as well
        self.keyring_storage.delete()
    }
}

// A global in-memory store for mapping Astral home -> AuthDotJson.
static EPHEMERAL_AUTH_STORE: Lazy<Mutex<HashMap<String, AuthDotJson>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Debug)]
struct EphemeralAuthStorage {
    codex_home: PathBuf,
    namespace: AuthStorageNamespace,
}

impl EphemeralAuthStorage {
    fn new(codex_home: PathBuf, namespace: AuthStorageNamespace) -> Self {
        Self {
            codex_home,
            namespace,
        }
    }

    fn with_store<F, T>(&self, action: F) -> std::io::Result<T>
    where
        F: FnOnce(&mut HashMap<String, AuthDotJson>, String) -> std::io::Result<T>,
    {
        let key = compute_namespaced_store_key(&self.codex_home, self.namespace)?;
        let mut store = EPHEMERAL_AUTH_STORE
            .lock()
            .map_err(|_| std::io::Error::other("failed to lock ephemeral auth storage"))?;
        action(&mut store, key)
    }
}

impl AuthStorageBackend for EphemeralAuthStorage {
    fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
        self.with_store(|store, key| Ok(store.get(&key).cloned()))
    }

    fn save(&self, auth: &AuthDotJson) -> std::io::Result<()> {
        self.with_store(|store, key| {
            store.insert(key, auth.clone());
            Ok(())
        })
    }

    fn delete(&self) -> std::io::Result<bool> {
        self.with_store(|store, key| Ok(store.remove(&key).is_some()))
    }
}

pub(super) fn create_auth_storage(
    codex_home: PathBuf,
    mode: AuthCredentialsStoreMode,
) -> Arc<dyn AuthStorageBackend> {
    let keyring_store: Arc<dyn KeyringStore> = Arc::new(DefaultKeyringStore);
    create_namespaced_auth_storage_with_keyring_store(
        codex_home,
        mode,
        AuthStorageNamespace::Astral,
        keyring_store,
    )
}

pub(super) fn create_codex_oauth_storage(
    codex_home: PathBuf,
    mode: AuthCredentialsStoreMode,
) -> Arc<dyn AuthStorageBackend> {
    let keyring_store: Arc<dyn KeyringStore> = Arc::new(DefaultKeyringStore);
    create_namespaced_auth_storage_with_keyring_store(
        codex_home,
        mode,
        AuthStorageNamespace::CodexOAuth,
        keyring_store,
    )
}

fn create_namespaced_auth_storage_with_keyring_store(
    codex_home: PathBuf,
    mode: AuthCredentialsStoreMode,
    namespace: AuthStorageNamespace,
    keyring_store: Arc<dyn KeyringStore>,
) -> Arc<dyn AuthStorageBackend> {
    match mode {
        AuthCredentialsStoreMode::File => {
            Arc::new(FileAuthStorage::new_namespaced(codex_home, namespace))
        }
        AuthCredentialsStoreMode::Keyring => Arc::new(KeyringAuthStorage::new_namespaced(
            codex_home,
            namespace,
            keyring_store,
        )),
        AuthCredentialsStoreMode::Auto => Arc::new(AutoAuthStorage::new_namespaced(
            codex_home,
            namespace,
            keyring_store,
        )),
        AuthCredentialsStoreMode::Ephemeral => {
            Arc::new(EphemeralAuthStorage::new(codex_home, namespace))
        }
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
