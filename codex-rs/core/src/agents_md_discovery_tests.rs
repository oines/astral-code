use super::*;
use codex_app_server_protocol::ConfigLayerSource;
use codex_config::ConfigLayerEntry;
use codex_config::ConfigLayerStack;
use codex_config::ConfigRequirements;
use codex_config::ConfigRequirementsToml;
use codex_exec_server::CopyOptions;
use codex_exec_server::CreateDirectoryOptions;
use codex_exec_server::FileMetadata;
use codex_exec_server::FileSystemResult;
use codex_exec_server::FileSystemSandboxContext;
use codex_exec_server::GlobSearchRequest;
use codex_exec_server::GlobSearchResponse;
use codex_exec_server::GrepSearchRequest;
use codex_exec_server::GrepSearchResponse;
use codex_exec_server::ReadDirectoryEntry;
use codex_exec_server::RemoveOptions;
use pretty_assertions::assert_eq;
use std::io;
use std::sync::Mutex;
use tokio::sync::Notify;
use tokio::sync::Semaphore;

#[derive(Clone, Copy)]
enum InjectedFailure {
    Metadata(io::ErrorKind),
    MetadataBlocked,
    MetadataBlockedByFilenamePrefix(&'static str),
    MetadataPending,
    Read(io::ErrorKind),
}

struct FailingFileSystem {
    path: AbsolutePathBuf,
    failure: InjectedFailure,
    metadata_calls: Arc<MetadataCallCounts>,
}

struct MetadataCallCounts {
    paths: Mutex<Vec<PathUri>>,
    started: Notify,
    release: Semaphore,
}

impl Default for MetadataCallCounts {
    fn default() -> Self {
        Self {
            paths: Mutex::new(Vec::new()),
            started: Notify::new(),
            release: Semaphore::new(0),
        }
    }
}

#[async_trait::async_trait]
impl ExecutorFileSystem for FailingFileSystem {
    async fn canonicalize(
        &self,
        path: &PathUri,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<PathUri> {
        LOCAL_FS.canonicalize(path, sandbox).await
    }

    async fn read_file(
        &self,
        path: &PathUri,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<Vec<u8>> {
        if path.to_abs_path()? == self.path
            && let InjectedFailure::Read(kind) = self.failure
        {
            return Err(io::Error::new(kind, "injected read failure"));
        }
        LOCAL_FS.read_file(path, sandbox).await
    }

    async fn write_file(
        &self,
        path: &PathUri,
        contents: Vec<u8>,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<()> {
        LOCAL_FS.write_file(path, contents, sandbox).await
    }

    async fn create_directory(
        &self,
        path: &PathUri,
        options: CreateDirectoryOptions,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<()> {
        LOCAL_FS.create_directory(path, options, sandbox).await
    }

    async fn get_metadata(
        &self,
        path: &PathUri,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<FileMetadata> {
        let path_abs = path.to_abs_path()?;
        self.metadata_calls
            .paths
            .lock()
            .expect("metadata paths lock")
            .push(path.clone());
        self.metadata_calls.started.notify_one();
        match self.failure {
            InjectedFailure::Metadata(kind) if path_abs == self.path => {
                Err(io::Error::new(kind, "injected metadata failure"))
            }
            InjectedFailure::MetadataBlocked if path_abs == self.path => {
                self.metadata_calls
                    .release
                    .acquire()
                    .await
                    .expect("metadata release semaphore")
                    .forget();
                LOCAL_FS.get_metadata(path, sandbox).await
            }
            InjectedFailure::MetadataBlockedByFilenamePrefix(prefix)
                if path_abs
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(prefix)) =>
            {
                self.metadata_calls
                    .release
                    .acquire()
                    .await
                    .expect("metadata release semaphore")
                    .forget();
                LOCAL_FS.get_metadata(path, sandbox).await
            }
            InjectedFailure::MetadataPending if path_abs == self.path => {
                std::future::pending().await
            }
            InjectedFailure::Metadata(_)
            | InjectedFailure::MetadataBlocked
            | InjectedFailure::MetadataBlockedByFilenamePrefix(_)
            | InjectedFailure::MetadataPending
            | InjectedFailure::Read(_) => LOCAL_FS.get_metadata(path, sandbox).await,
        }
    }

    async fn read_directory(
        &self,
        path: &PathUri,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<Vec<ReadDirectoryEntry>> {
        LOCAL_FS.read_directory(path, sandbox).await
    }

    async fn remove(
        &self,
        path: &PathUri,
        options: RemoveOptions,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<()> {
        LOCAL_FS.remove(path, options, sandbox).await
    }

    async fn copy(
        &self,
        source_path: &PathUri,
        destination_path: &PathUri,
        options: CopyOptions,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<()> {
        LOCAL_FS
            .copy(source_path, destination_path, options, sandbox)
            .await
    }

    async fn glob_search(
        &self,
        request: GlobSearchRequest,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<GlobSearchResponse> {
        LOCAL_FS.glob_search(request, sandbox).await
    }

    async fn grep_search(
        &self,
        request: GrepSearchRequest,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<GrepSearchResponse> {
        LOCAL_FS.grep_search(request, sandbox).await
    }
}

#[tokio::test]
async fn total_byte_limit_truncates_later_project_docs() {
    let repo = tempfile::tempdir().expect("tempdir");
    fs::write(repo.path().join(".git"), "").unwrap();
    fs::write(repo.path().join("AGENTS.md"), "root").unwrap();
    let nested = repo.path().join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("AGENTS.md"), "abcdef").unwrap();

    let mut config = make_config(&repo, /*limit*/ 7, /*instructions*/ None).await;
    config.cwd = nested.abs();

    let loaded = load_agents_md(&config).await.expect("project instructions");
    let expected = LoadedAgentsMd {
        entries: vec![
            InstructionEntry {
                contents: "root".to_string(),
                provenance: project_provenance(
                    repo.path().join("AGENTS.md").abs(),
                    config.cwd.clone(),
                ),
            },
            InstructionEntry {
                contents: "abc".to_string(),
                provenance: project_provenance(config.cwd.join("AGENTS.md"), config.cwd.clone()),
            },
        ],
    };

    assert_eq!(loaded, expected);
    assert_eq!(loaded.text(), "root\n\nabc");
}

#[tokio::test]
async fn read_agents_md_propagates_metadata_errors() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = make_config(&tmp, /*limit*/ 4096, /*instructions*/ None).await;
    let fs = FailingFileSystem {
        path: config.cwd.join(".git"),
        failure: InjectedFailure::Metadata(io::ErrorKind::PermissionDenied),
        metadata_calls: Arc::default(),
    };
    let cwd = PathUri::from_abs_path(&config.cwd);

    let err =
        super::super::read_agents_md(&config, &fs, "local", &cwd, config.project_doc_max_bytes)
            .await
            .expect_err("metadata error");

    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[tokio::test]
async fn read_agents_md_propagates_read_errors() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(tmp.path().join("AGENTS.md"), "project doc").unwrap();
    let config = make_config(&tmp, /*limit*/ 4096, /*instructions*/ None).await;
    let fs = FailingFileSystem {
        path: config.cwd.join("AGENTS.md"),
        failure: InjectedFailure::Read(io::ErrorKind::PermissionDenied),
        metadata_calls: Arc::default(),
    };
    let cwd = PathUri::from_abs_path(&config.cwd);

    let err =
        super::super::read_agents_md(&config, &fs, "local", &cwd, config.project_doc_max_bytes)
            .await
            .expect_err("read error");

    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[tokio::test]
async fn read_agents_md_ignores_files_removed_after_discovery() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(tmp.path().join("AGENTS.md"), "project doc").unwrap();
    let config = make_config(&tmp, /*limit*/ 4096, /*instructions*/ None).await;
    let fs = FailingFileSystem {
        path: config.cwd.join("AGENTS.md"),
        failure: InjectedFailure::Read(io::ErrorKind::NotFound),
        metadata_calls: Arc::default(),
    };
    let cwd = PathUri::from_abs_path(&config.cwd);

    let loaded =
        super::super::read_agents_md(&config, &fs, "local", &cwd, config.project_doc_max_bytes)
            .await
            .expect("removed file is recoverable");

    assert_eq!(loaded, None);
}

#[tokio::test]
async fn marker_search_does_not_wait_for_a_higher_ancestor() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(tmp.path().join(".git"), "").unwrap();
    fs::write(tmp.path().join("AGENTS.md"), "project doc").unwrap();
    let nested = tmp.path().join("nested");
    fs::create_dir(&nested).unwrap();

    let mut config = make_config(&tmp, /*limit*/ 4096, /*instructions*/ None).await;
    config.cwd = nested.abs();
    let fs = FailingFileSystem {
        path: tmp
            .path()
            .parent()
            .expect("tempdir parent")
            .join(".git")
            .abs(),
        failure: InjectedFailure::MetadataPending,
        metadata_calls: Arc::default(),
    };
    let cwd = PathUri::from_abs_path(&config.cwd);

    let paths = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        super::super::agents_md_paths(&config, &cwd, &fs),
    )
    .await
    .expect("nearest marker should complete")
    .expect("AGENTS.md discovery");

    assert_eq!(
        paths,
        vec![PathUri::from_abs_path(
            &tmp.path().join(DEFAULT_AGENTS_MD_FILENAME).abs()
        )]
    );
}

#[tokio::test]
async fn project_root_marker_search_limits_concurrent_probes_and_preserves_order() {
    const CONCURRENCY_LIMIT: usize = 256;

    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(tmp.path().join("AGENTS.md"), "project doc").unwrap();
    let nested = tmp.path().join("nested");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("AGENTS.md"), "nested project doc").unwrap();

    let markers = (0..=CONCURRENCY_LIMIT)
        .map(|index| format!(".project-root-{index}"))
        .collect::<Vec<_>>();
    fs::write(
        tmp.path()
            .join(markers.last().expect("last project root marker")),
        "",
    )
    .unwrap();
    let marker_refs = markers.iter().map(String::as_str).collect::<Vec<_>>();
    let mut config = make_config_with_project_root_markers(
        &tmp,
        /*limit*/ 4096,
        /*instructions*/ None,
        &marker_refs,
    )
    .await;
    config.cwd = nested.abs();
    let cwd = PathUri::from_abs_path(&config.cwd);
    let expected_initial_probes = markers
        .iter()
        .map(|marker| cwd.join(marker).expect("project root marker path"))
        .collect::<Vec<_>>();
    let max_probe_count = markers.len() * config.cwd.ancestors().count();
    let metadata_calls = Arc::new(MetadataCallCounts::default());
    let fs = FailingFileSystem {
        path: config.cwd.join("unused"),
        failure: InjectedFailure::MetadataBlockedByFilenamePrefix(".project-root-"),
        metadata_calls: Arc::clone(&metadata_calls),
    };

    let assertions = async {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let started = metadata_calls.started.notified();
                if metadata_calls
                    .paths
                    .lock()
                    .expect("metadata paths lock")
                    .len()
                    >= CONCURRENCY_LIMIT
                {
                    break;
                }
                started.await;
            }
        })
        .await
        .expect("initial marker window should start");
        assert_eq!(
            *metadata_calls.paths.lock().expect("metadata paths lock"),
            expected_initial_probes[..CONCURRENCY_LIMIT]
        );

        metadata_calls.release.add_permits(1);
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let started = metadata_calls.started.notified();
                if metadata_calls
                    .paths
                    .lock()
                    .expect("metadata paths lock")
                    .len()
                    > CONCURRENCY_LIMIT
                {
                    break;
                }
                started.await;
            }
        })
        .await
        .expect("next marker probe should start");
        assert_eq!(
            *metadata_calls.paths.lock().expect("metadata paths lock"),
            expected_initial_probes
        );

        metadata_calls.release.add_permits(max_probe_count);
    };
    let (paths, ()) = tokio::join!(
        super::super::agents_md_paths(&config, &cwd, &fs),
        assertions
    );
    let paths = paths.expect("AGENTS.md discovery");

    assert_eq!(
        paths,
        vec![
            PathUri::from_abs_path(&tmp.path().join(DEFAULT_AGENTS_MD_FILENAME).abs()),
            PathUri::from_abs_path(&nested.join(DEFAULT_AGENTS_MD_FILENAME).abs()),
        ]
    );
}

#[tokio::test]
async fn agents_md_search_starts_all_directory_probes() {
    const NESTING_DEPTH: usize = 9;

    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(tmp.path().join(".git"), "").unwrap();
    fs::write(tmp.path().join("AGENTS.md"), "project doc").unwrap();
    let mut nested = tmp.path().to_path_buf();
    for depth in 0..NESTING_DEPTH {
        nested.push(format!("nested-{depth}"));
    }
    fs::create_dir_all(&nested).unwrap();

    let mut config = make_config(&tmp, /*limit*/ 4096, /*instructions*/ None).await;
    config.cwd = nested.abs();
    let cwd = PathUri::from_abs_path(&config.cwd);
    let mut search_dirs = config
        .cwd
        .ancestors()
        .take(NESTING_DEPTH + 1)
        .collect::<Vec<_>>();
    search_dirs.reverse();
    let expected_probes = search_dirs
        .into_iter()
        .map(|directory| PathUri::from_abs_path(&directory.join(LOCAL_AGENTS_MD_FILENAME)))
        .collect::<Vec<_>>();
    let metadata_calls = Arc::new(MetadataCallCounts::default());
    let fs = FailingFileSystem {
        path: tmp.path().join(LOCAL_AGENTS_MD_FILENAME).abs(),
        failure: InjectedFailure::MetadataBlocked,
        metadata_calls: Arc::clone(&metadata_calls),
    };

    let search =
        tokio::spawn(async move { super::super::agents_md_paths(&config, &cwd, &fs).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let started = metadata_calls.started.notified();
            if expected_probes.iter().all(|candidate| {
                metadata_calls
                    .paths
                    .lock()
                    .expect("metadata paths lock")
                    .contains(candidate)
            }) {
                break;
            }
            started.await;
        }
    })
    .await
    .expect("all directory probes should start");

    let mut actual_probes = metadata_calls
        .paths
        .lock()
        .expect("metadata paths lock")
        .iter()
        .filter(|path| expected_probes.contains(path))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    actual_probes.sort();
    let mut expected_probes = expected_probes
        .into_iter()
        .map(|path| path.to_string())
        .collect::<Vec<_>>();
    expected_probes.sort();
    assert_eq!(actual_probes, expected_probes);

    metadata_calls.release.add_permits(1);
    let paths = tokio::time::timeout(std::time::Duration::from_secs(5), search)
        .await
        .expect("AGENTS.md search should complete")
        .expect("AGENTS.md search task")
        .expect("AGENTS.md discovery");

    assert_eq!(
        paths,
        vec![PathUri::from_abs_path(
            &tmp.path().join(DEFAULT_AGENTS_MD_FILENAME).abs()
        )]
    );
}

#[tokio::test]
async fn empty_project_root_markers_only_probe_cwd_candidates() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(tmp.path().join("AGENTS.md"), "parent doc").unwrap();
    let nested = tmp.path().join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("AGENTS.md"), "cwd doc").unwrap();

    let mut config = make_config_with_project_root_markers(
        &tmp,
        /*limit*/ 4096,
        /*instructions*/ None,
        &[],
    )
    .await;
    config.cwd = nested.abs();
    let metadata_calls = Arc::new(MetadataCallCounts::default());
    let fs = FailingFileSystem {
        path: config.cwd.join("unused"),
        failure: InjectedFailure::Read(io::ErrorKind::PermissionDenied),
        metadata_calls: Arc::clone(&metadata_calls),
    };
    let cwd = PathUri::from_abs_path(&config.cwd);

    let paths = super::super::agents_md_paths(&config, &cwd, &fs)
        .await
        .expect("AGENTS.md discovery");
    let override_path = cwd.join(LOCAL_AGENTS_MD_FILENAME).expect("override path");
    let agents_path = cwd.join(DEFAULT_AGENTS_MD_FILENAME).expect("agents path");

    assert_eq!(paths, vec![agents_path.clone()]);
    assert_eq!(
        metadata_calls
            .paths
            .lock()
            .expect("metadata paths lock")
            .clone(),
        vec![override_path, agents_path]
    );
}

#[tokio::test]
async fn project_layers_do_not_override_project_root_markers() {
    let root = tempfile::tempdir().expect("tempdir");
    fs::write(root.path().join(".git"), "").unwrap();
    fs::write(root.path().join("AGENTS.md"), "root doc").unwrap();
    let nested = root.path().join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("AGENTS.md"), "nested doc").unwrap();

    let mut config = make_config(&root, /*limit*/ 4096, /*instructions*/ None).await;
    config.cwd = nested.abs();
    let project_layer = |dot_codex_folder: AbsolutePathBuf, marker: &str| {
        ConfigLayerEntry::new(
            ConfigLayerSource::Project { dot_codex_folder },
            TomlValue::Table(
                [(
                    "project_root_markers".to_string(),
                    TomlValue::Array(vec![TomlValue::String(marker.to_string())]),
                )]
                .into_iter()
                .collect(),
            ),
        )
    };
    config.config_layer_stack = ConfigLayerStack::new(
        vec![
            project_layer(root.path().join(".codex").abs(), ".ignored-root-marker"),
            project_layer(config.cwd.join(".codex"), ".ignored-nested-marker"),
        ],
        ConfigRequirements::default(),
        ConfigRequirementsToml::default(),
    )
    .expect("valid project layer ordering");

    let discovery = agents_md_paths(&config).await.expect("discover paths");

    assert_eq!(
        discovery,
        vec![
            PathUri::from_abs_path(&root.path().join("AGENTS.md").abs()),
            PathUri::from_abs_path(&config.cwd.join("AGENTS.md")),
        ]
    );
}
