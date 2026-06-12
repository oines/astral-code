use super::curated_plugins_repo_path;
use super::has_local_curated_plugins_snapshot;
use super::read_curated_plugins_sha;
use super::sync_curated_plugins_repo;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

const TEST_CURATED_PLUGIN_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

#[test]
fn curated_plugins_repo_path_uses_astral_home_tmp_dir() {
    let tmp = tempdir().expect("tempdir");
    assert_eq!(
        curated_plugins_repo_path(tmp.path()),
        tmp.path().join(".tmp/plugins")
    );
}

#[test]
fn read_curated_plugins_sha_reads_trimmed_sha_file() {
    let tmp = tempdir().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join(".tmp")).expect("create tmp");
    std::fs::write(tmp.path().join(".tmp/plugins.sha"), "abc123\n").expect("write sha");

    assert_eq!(
        read_curated_plugins_sha(tmp.path()).as_deref(),
        Some("abc123")
    );
}

#[test]
fn has_local_curated_plugins_snapshot_requires_manifest_and_sha() {
    let tmp = tempdir().expect("tempdir");
    assert!(!has_local_curated_plugins_snapshot(tmp.path()));

    let manifest_path = curated_plugins_repo_path(tmp.path()).join(".agents/plugins");
    std::fs::create_dir_all(&manifest_path).expect("create manifest parent");
    std::fs::write(manifest_path.join("marketplace.json"), "{}").expect("write manifest");
    assert!(!has_local_curated_plugins_snapshot(tmp.path()));

    std::fs::write(tmp.path().join(".tmp/plugins.sha"), TEST_CURATED_PLUGIN_SHA)
        .expect("write sha");
    assert!(has_local_curated_plugins_snapshot(tmp.path()));
}

#[test]
fn sync_curated_plugins_repo_returns_existing_local_snapshot_without_network() {
    let tmp = tempdir().expect("tempdir");
    let manifest_path = curated_plugins_repo_path(tmp.path()).join(".agents/plugins");
    std::fs::create_dir_all(&manifest_path).expect("create manifest parent");
    std::fs::write(manifest_path.join("marketplace.json"), "{}").expect("write manifest");
    std::fs::write(
        tmp.path().join(".tmp/plugins.sha"),
        format!("{TEST_CURATED_PLUGIN_SHA}\n"),
    )
    .expect("write sha");

    assert_eq!(
        sync_curated_plugins_repo(tmp.path()).as_deref(),
        Ok(TEST_CURATED_PLUGIN_SHA)
    );
}

#[test]
fn sync_curated_plugins_repo_fails_when_local_snapshot_is_missing() {
    let tmp = tempdir().expect("tempdir");
    let err = sync_curated_plugins_repo(tmp.path()).expect_err("startup sync should be disabled");

    assert_eq!(
        err,
        "Astral curated plugin startup sync is disabled and no local curated plugins snapshot is available"
    );
}
