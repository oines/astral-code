use std::fs::File;
use std::path::Path;
use std::path::PathBuf;

use codex_otel::CURATED_PLUGINS_STARTUP_SYNC_FINAL_METRIC;
use codex_otel::CURATED_PLUGINS_STARTUP_SYNC_METRIC;

const CURATED_PLUGINS_RELATIVE_DIR: &str = ".tmp/plugins";
const CURATED_PLUGINS_SHA_FILE: &str = ".tmp/plugins.sha";
const CURATED_PLUGINS_SYNC_LOCK_FILE: &str = ".tmp/plugins.sync.lock";

pub fn curated_plugins_repo_path(codex_home: &Path) -> PathBuf {
    codex_home.join(CURATED_PLUGINS_RELATIVE_DIR)
}

pub fn read_curated_plugins_sha(codex_home: &Path) -> Option<String> {
    read_sha_file(curated_plugins_sha_path(codex_home).as_path())
}

fn curated_plugins_sha_path(codex_home: &Path) -> PathBuf {
    codex_home.join(CURATED_PLUGINS_SHA_FILE)
}

pub fn sync_curated_plugins_repo(codex_home: &Path) -> Result<String, String> {
    let _file_guard = lock_curated_plugins_startup_sync(codex_home)?;

    if has_local_curated_plugins_snapshot(codex_home)
        && let Some(local_sha) = read_curated_plugins_sha(codex_home)
    {
        emit_curated_plugins_startup_sync_metric("disabled", "local_snapshot");
        emit_curated_plugins_startup_sync_final_metric("disabled", "local_snapshot");
        return Ok(local_sha);
    }

    emit_curated_plugins_startup_sync_metric("disabled", "failure");
    emit_curated_plugins_startup_sync_final_metric("disabled", "failure");
    Err(
        "Astral curated plugin startup sync is disabled and no local curated plugins snapshot is available"
            .to_string(),
    )
}

fn lock_curated_plugins_startup_sync(codex_home: &Path) -> Result<File, String> {
    let lock_path = codex_home.join(CURATED_PLUGINS_SYNC_LOCK_FILE);
    std::fs::create_dir_all(codex_home.join(".tmp"))
        .map_err(|err| format!("failed to create curated plugins sync directory: {err}"))?;
    let lock_file = File::options()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|err| format!("failed to open curated plugins sync lock: {err}"))?;
    lock_file
        .lock()
        .map_err(|err| format!("failed to lock curated plugins sync: {err}"))?;
    Ok(lock_file)
}

pub fn has_local_curated_plugins_snapshot(codex_home: &Path) -> bool {
    curated_plugins_repo_path(codex_home)
        .join(".agents/plugins/marketplace.json")
        .is_file()
        && codex_home.join(CURATED_PLUGINS_SHA_FILE).is_file()
}

fn emit_curated_plugins_startup_sync_metric(transport: &'static str, status: &'static str) {
    emit_curated_plugins_startup_sync_counter(
        CURATED_PLUGINS_STARTUP_SYNC_METRIC,
        transport,
        status,
    );
}

fn emit_curated_plugins_startup_sync_final_metric(transport: &'static str, status: &'static str) {
    emit_curated_plugins_startup_sync_counter(
        CURATED_PLUGINS_STARTUP_SYNC_FINAL_METRIC,
        transport,
        status,
    );
}

fn emit_curated_plugins_startup_sync_counter(
    metric_name: &str,
    transport: &'static str,
    status: &'static str,
) {
    let Some(metrics) = codex_otel::global() else {
        return;
    };
    let tags = [("transport", transport), ("status", status)];
    let _ = metrics.counter(metric_name, /*inc*/ 1, &tags);
}

fn read_sha_file(sha_path: &Path) -> Option<String> {
    std::fs::read_to_string(sha_path)
        .ok()
        .map(|sha| sha.trim().to_string())
        .filter(|sha| !sha.is_empty())
}

#[cfg(test)]
#[path = "startup_sync_tests.rs"]
mod tests;
