use super::RemotePluginCatalogError;
use super::RemotePluginServiceConfig;
use crate::store::PLUGINS_CACHE_DIR;
use crate::store::PluginStoreError;
use codex_login::CodexAuth;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use tracing::info;
use tracing::warn;

static REMOTE_INSTALLED_PLUGIN_BUNDLE_SYNC_IN_FLIGHT: OnceLock<
    Mutex<HashSet<RemoteInstalledPluginBundleSyncKey>>,
> = OnceLock::new();
static REMOTE_PLUGIN_CACHE_MUTATIONS_IN_FLIGHT: OnceLock<
    Mutex<HashMap<RemotePluginCacheMutationKey, usize>>,
> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoteInstalledPluginBundleSyncOutcome {
    pub installed_plugin_ids: Vec<String>,
    pub removed_cache_plugin_ids: Vec<String>,
    pub failed_remote_plugin_ids: Vec<String>,
}

impl RemoteInstalledPluginBundleSyncOutcome {
    pub fn changed_local_cache(&self) -> bool {
        !self.installed_plugin_ids.is_empty() || !self.removed_cache_plugin_ids.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RemoteInstalledPluginBundleSyncError {
    #[error("{0}")]
    Catalog(#[from] RemotePluginCatalogError),

    #[error("{0}")]
    Store(#[from] PluginStoreError),

    #[error("failed to join stale remote plugin cache cleanup task: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[error("failed to remove stale remote plugin cache entries: {0}")]
    CacheRemove(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RemoteInstalledPluginBundleSyncKey {
    plugin_cache_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RemotePluginCacheMutationKey {
    plugin_cache_root: PathBuf,
    marketplace_name: String,
    plugin_name: String,
}

pub struct RemotePluginCacheMutationGuard {
    key: RemotePluginCacheMutationKey,
}

pub(crate) fn maybe_start_remote_installed_plugin_bundle_sync(
    codex_home: PathBuf,
    config: RemotePluginServiceConfig,
    auth: Option<CodexAuth>,
    on_local_cache_changed: Option<Arc<dyn Fn() + Send + Sync + 'static>>,
) {
    let Some(auth) = auth else {
        return;
    };
    let key = RemoteInstalledPluginBundleSyncKey {
        plugin_cache_root: remote_plugin_cache_root(&codex_home),
    };
    if !mark_remote_installed_plugin_bundle_sync_in_flight(key.clone()) {
        return;
    }

    tokio::spawn(async move {
        let result =
            sync_remote_installed_plugin_bundles_once(codex_home, &config, Some(&auth)).await;
        match result {
            Ok(outcome) => {
                if outcome.changed_local_cache()
                    && let Some(on_local_cache_changed) = on_local_cache_changed
                {
                    on_local_cache_changed();
                }
                info!(
                    installed_plugin_ids = ?outcome.installed_plugin_ids,
                    removed_cache_plugin_ids = ?outcome.removed_cache_plugin_ids,
                    failed_remote_plugin_ids = ?outcome.failed_remote_plugin_ids,
                    "completed remote installed plugin bundle sync"
                );
            }
            Err(err) => {
                warn!(
                    error = %err,
                    "remote installed plugin bundle sync failed"
                );
            }
        }
        clear_remote_installed_plugin_bundle_sync_in_flight(&key);
    });
}

pub async fn sync_remote_installed_plugin_bundles_once(
    _codex_home: PathBuf,
    _config: &RemotePluginServiceConfig,
    _auth: Option<&CodexAuth>,
) -> Result<RemoteInstalledPluginBundleSyncOutcome, RemoteInstalledPluginBundleSyncError> {
    Err(super::remote_plugin_control_plane_disabled_error().into())
}

pub fn mark_remote_plugin_cache_mutation_in_flight(
    codex_home: &Path,
    marketplace_name: &str,
    plugin_name: &str,
) -> RemotePluginCacheMutationGuard {
    let key = RemotePluginCacheMutationKey {
        plugin_cache_root: remote_plugin_cache_root(codex_home),
        marketplace_name: marketplace_name.to_string(),
        plugin_name: plugin_name.to_string(),
    };
    let mutations =
        REMOTE_PLUGIN_CACHE_MUTATIONS_IN_FLIGHT.get_or_init(|| Mutex::new(HashMap::new()));
    let mut mutations = match mutations.lock() {
        Ok(mutations) => mutations,
        Err(err) => err.into_inner(),
    };
    *mutations.entry(key.clone()).or_default() += 1;
    RemotePluginCacheMutationGuard { key }
}

impl Drop for RemotePluginCacheMutationGuard {
    fn drop(&mut self) {
        let Some(mutations) = REMOTE_PLUGIN_CACHE_MUTATIONS_IN_FLIGHT.get() else {
            return;
        };
        let mut mutations = match mutations.lock() {
            Ok(mutations) => mutations,
            Err(err) => err.into_inner(),
        };
        if let Some(count) = mutations.get_mut(&self.key) {
            *count -= 1;
            if *count == 0 {
                mutations.remove(&self.key);
            }
        }
    }
}

fn remote_plugin_cache_root(codex_home: &Path) -> PathBuf {
    codex_home.join(PLUGINS_CACHE_DIR)
}

fn mark_remote_installed_plugin_bundle_sync_in_flight(
    key: RemoteInstalledPluginBundleSyncKey,
) -> bool {
    let syncs =
        REMOTE_INSTALLED_PLUGIN_BUNDLE_SYNC_IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()));
    let mut syncs = match syncs.lock() {
        Ok(syncs) => syncs,
        Err(err) => err.into_inner(),
    };
    syncs.insert(key)
}

fn clear_remote_installed_plugin_bundle_sync_in_flight(key: &RemoteInstalledPluginBundleSyncKey) {
    let Some(syncs) = REMOTE_INSTALLED_PLUGIN_BUNDLE_SYNC_IN_FLIGHT.get() else {
        return;
    };
    let mut syncs = match syncs.lock() {
        Ok(syncs) => syncs,
        Err(err) => err.into_inner(),
    };
    syncs.remove(key);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_installed_plugin_sync_in_flight_dedupes_by_cache_root() {
        let codex_home = tempfile::tempdir().expect("create codex home");
        let key = RemoteInstalledPluginBundleSyncKey {
            plugin_cache_root: remote_plugin_cache_root(codex_home.path()),
        };

        assert!(mark_remote_installed_plugin_bundle_sync_in_flight(
            key.clone()
        ));
        assert!(!mark_remote_installed_plugin_bundle_sync_in_flight(
            key.clone()
        ));

        clear_remote_installed_plugin_bundle_sync_in_flight(&key);
        assert!(mark_remote_installed_plugin_bundle_sync_in_flight(
            key.clone()
        ));
        clear_remote_installed_plugin_bundle_sync_in_flight(&key);
    }
}
