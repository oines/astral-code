use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::sync::Mutex;

const PLUGIN_SHARE_LOCAL_PATHS_FILE: &str = ".tmp/plugin-share-local-paths-v1.json";
static PLUGIN_SHARE_LOCAL_PATHS_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginShareLocalPaths {
    #[serde(default)]
    local_plugin_paths_by_remote_plugin_id: BTreeMap<String, AbsolutePathBuf>,
}

pub(crate) fn load_plugin_share_local_paths(
    codex_home: &Path,
) -> io::Result<BTreeMap<String, AbsolutePathBuf>> {
    let _guard = lock_plugin_share_local_paths()?;
    read_plugin_share_local_paths(codex_home)
}

fn lock_plugin_share_local_paths() -> io::Result<std::sync::MutexGuard<'static, ()>> {
    PLUGIN_SHARE_LOCAL_PATHS_LOCK
        .lock()
        .map_err(|err| io::Error::other(format!("plugin share local path lock poisoned: {err}")))
}

fn read_plugin_share_local_paths(
    codex_home: &Path,
) -> io::Result<BTreeMap<String, AbsolutePathBuf>> {
    let path = plugin_share_local_paths_path(codex_home);
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(err) => return Err(err),
    };

    let mapping = serde_json::from_str::<PluginShareLocalPaths>(&contents).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "failed to parse plugin share local path mapping {}: {err}",
                path.display()
            ),
        )
    })?;
    Ok(mapping.local_plugin_paths_by_remote_plugin_id)
}

fn plugin_share_local_paths_path(codex_home: &Path) -> std::path::PathBuf {
    codex_home.join(PLUGIN_SHARE_LOCAL_PATHS_FILE)
}
