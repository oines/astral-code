use std::collections::HashMap;
use std::io;
use std::path::Path;

use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::sync::Mutex;

use codex_exec_server::CopyOptions;
use codex_exec_server::CreateDirectoryOptions;
use codex_exec_server::ExecutorFileSystem;
use codex_exec_server::FileMetadata;
use codex_exec_server::FileSystemResult;
use codex_exec_server::FileSystemSandboxContext;
use codex_exec_server::GlobSearchMatch;
use codex_exec_server::GlobSearchRequest;
use codex_exec_server::GlobSearchResponse;
use codex_exec_server::GrepSearchRequest;
use codex_exec_server::GrepSearchResponse;
use codex_exec_server::LOCAL_FS;
use codex_exec_server::ReadDirectoryEntry;
use codex_exec_server::RemoveOptions;
use codex_protocol::models::PermissionProfile;
use codex_utils_absolute_path::AbsolutePathBuf;
use core_test_support::PathExt;

use super::GrepArgs;
use super::add_line_numbers;
use super::edit_file;
use super::file_environment_id;
use super::glob_files;
use super::grep_files;
use super::is_blocked_device_path;
use super::split_lines_preserving_newline;
use super::write_file;

#[derive(Default)]
struct RecordingFileSystem {
    files: Mutex<HashMap<String, Vec<u8>>>,
    calls: Mutex<Vec<String>>,
    glob_response: Mutex<Option<GlobSearchResponse>>,
    grep_response: Mutex<Option<GrepSearchResponse>>,
}

impl RecordingFileSystem {
    async fn insert_file(&self, path: &AbsolutePathBuf, contents: impl Into<Vec<u8>>) {
        self.files
            .lock()
            .await
            .insert(path_key(path), contents.into());
    }

    async fn file_contents(&self, path: &AbsolutePathBuf) -> Option<Vec<u8>> {
        self.files.lock().await.get(&path_key(path)).cloned()
    }

    async fn calls(&self) -> Vec<String> {
        self.calls.lock().await.clone()
    }

    async fn set_glob_response(&self, response: GlobSearchResponse) {
        *self.glob_response.lock().await = Some(response);
    }

    async fn set_grep_response(&self, response: GrepSearchResponse) {
        *self.grep_response.lock().await = Some(response);
    }

    async fn record(&self, method: &str, path: &AbsolutePathBuf) {
        self.calls
            .lock()
            .await
            .push(format!("{method}:{}", path.display()));
    }
}

fn path_key(path: &AbsolutePathBuf) -> String {
    path.to_string_lossy().into_owned()
}

#[async_trait::async_trait]
impl ExecutorFileSystem for RecordingFileSystem {
    async fn canonicalize(
        &self,
        path: &AbsolutePathBuf,
        _sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<AbsolutePathBuf> {
        Ok(path.clone())
    }

    async fn join(
        &self,
        base_path: &AbsolutePathBuf,
        path: &Path,
    ) -> FileSystemResult<AbsolutePathBuf> {
        Ok(base_path.join(path))
    }

    async fn parent(&self, path: &AbsolutePathBuf) -> FileSystemResult<Option<AbsolutePathBuf>> {
        Ok(path.parent())
    }

    async fn read_file(
        &self,
        path: &AbsolutePathBuf,
        _sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<Vec<u8>> {
        self.record("read_file", path).await;
        self.file_contents(path).await.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} not found", path.display()),
            )
        })
    }

    async fn write_file(
        &self,
        path: &AbsolutePathBuf,
        contents: Vec<u8>,
        _sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<()> {
        self.record("write_file", path).await;
        self.insert_file(path, contents).await;
        Ok(())
    }

    async fn create_directory(
        &self,
        _path: &AbsolutePathBuf,
        _create_directory_options: CreateDirectoryOptions,
        _sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<()> {
        Ok(())
    }

    async fn get_metadata(
        &self,
        path: &AbsolutePathBuf,
        _sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<FileMetadata> {
        if self.file_contents(path).await.is_some() {
            Ok(FileMetadata {
                is_directory: false,
                is_file: true,
                is_symlink: false,
                created_at_ms: 0,
                modified_at_ms: 0,
            })
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} not found", path.display()),
            ))
        }
    }

    async fn read_directory(
        &self,
        _path: &AbsolutePathBuf,
        _sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<Vec<ReadDirectoryEntry>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "read_directory unsupported by recording filesystem",
        ))
    }

    async fn remove(
        &self,
        _path: &AbsolutePathBuf,
        _remove_options: RemoveOptions,
        _sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "remove unsupported by recording filesystem",
        ))
    }

    async fn copy(
        &self,
        _source_path: &AbsolutePathBuf,
        _destination_path: &AbsolutePathBuf,
        _copy_options: CopyOptions,
        _sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "copy unsupported by recording filesystem",
        ))
    }

    async fn glob_search(
        &self,
        request: GlobSearchRequest,
        _sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<GlobSearchResponse> {
        self.calls.lock().await.push(format!(
            "glob_search:{}:{}",
            request.root.display(),
            request.pattern
        ));
        Ok(self
            .glob_response
            .lock()
            .await
            .clone()
            .unwrap_or(GlobSearchResponse {
                matches: Vec::new(),
                truncated: false,
            }))
    }

    async fn grep_search(
        &self,
        request: GrepSearchRequest,
        _sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<GrepSearchResponse> {
        self.calls.lock().await.push(format!(
            "grep_search:{}:{}",
            request.root.display(),
            request.pattern
        ));
        Ok(self
            .grep_response
            .lock()
            .await
            .clone()
            .unwrap_or(GrepSearchResponse {
                lines: Vec::new(),
                truncated: false,
            }))
    }
}

#[test]
fn read_output_uses_compact_line_number_prefixes() {
    let text = "first\n  second\nthird";
    let lines = split_lines_preserving_newline(text);

    assert_eq!(
        add_line_numbers(&lines[1..], /*start_line*/ 2),
        "2\t  second\n3\tthird"
    );
}

#[test]
fn grep_line_numbers_flag_is_optional() {
    let args: GrepArgs =
        serde_json::from_value(json!({ "pattern": "needle" })).expect("valid Grep args");

    assert_eq!(args.line_numbers, None);
}

#[test]
fn file_tools_accept_snake_and_camel_environment_ids() -> anyhow::Result<()> {
    assert_eq!(
        file_environment_id(&json!({ "environment_id": "remote" }).to_string())?,
        Some("remote".to_string())
    );
    assert_eq!(
        file_environment_id(&json!({ "environmentId": "secondary" }).to_string())?,
        Some("secondary".to_string())
    );
    assert_eq!(file_environment_id(&json!({}).to_string())?, None);
    Ok(())
}

#[tokio::test]
async fn grep_content_output_can_include_line_numbers() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    std::fs::write(temp_dir.path().join("lib.rs"), "alpha\nneedle\nomega\n").expect("write source");
    let sandbox = FileSystemSandboxContext::from_permission_profile(PermissionProfile::Disabled);

    let output = grep_files(
        json!({
            "pattern": "needle",
            "path": ".",
            "output_mode": "content",
            "line_numbers": true
        })
        .to_string(),
        LOCAL_FS.as_ref(),
        &sandbox,
        &cwd,
    )
    .await
    .expect("grep succeeds");

    assert_eq!(output, "lib.rs:2:needle\n");
}

#[test]
fn read_blocks_device_paths_that_can_hang() {
    assert!(is_blocked_device_path(std::path::Path::new("/dev/random")));
    assert!(is_blocked_device_path(std::path::Path::new(
        "/proc/self/fd/0"
    )));
    assert!(!is_blocked_device_path(std::path::Path::new("/dev/null")));
}

#[tokio::test]
async fn edit_empty_old_string_creates_missing_file() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    let sandbox = FileSystemSandboxContext::from_permission_profile(PermissionProfile::Disabled);
    let arguments = json!({
        "file_path": "created.txt",
        "old_string": "",
        "new_string": "created content\n"
    })
    .to_string();

    let output = edit_file(arguments, LOCAL_FS.as_ref(), &sandbox, &cwd)
        .await
        .expect("edit succeeds");

    assert_eq!(
        output,
        "The file created.txt has been updated successfully."
    );
    assert_eq!(
        std::fs::read_to_string(temp_dir.path().join("created.txt")).expect("created file"),
        "created content\n"
    );
}

#[tokio::test]
async fn write_uses_executor_file_system() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    let path = cwd.join("remote.txt");
    let fs = RecordingFileSystem::default();
    let sandbox = FileSystemSandboxContext::from_permission_profile(PermissionProfile::Disabled);
    let arguments = json!({
        "file_path": "remote.txt",
        "content": "written through backend\n"
    })
    .to_string();

    let output = write_file(arguments, &fs, &sandbox, &cwd)
        .await
        .expect("write succeeds");

    assert_eq!(output, "Wrote remote.txt");
    assert_eq!(
        fs.file_contents(&path).await.expect("recorded file"),
        b"written through backend\n"
    );
    assert_eq!(
        fs.calls().await,
        vec![format!("write_file:{}", path.display())]
    );
    assert!(!temp_dir.path().join("remote.txt").exists());
}

#[tokio::test]
async fn edit_uses_executor_file_system() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    let path = cwd.join("remote.txt");
    let fs = RecordingFileSystem::default();
    fs.insert_file(&path, b"before\n").await;
    let sandbox = FileSystemSandboxContext::from_permission_profile(PermissionProfile::Disabled);
    let arguments = json!({
        "file_path": "remote.txt",
        "old_string": "before",
        "new_string": "after"
    })
    .to_string();

    let output = edit_file(arguments, &fs, &sandbox, &cwd)
        .await
        .expect("edit succeeds");

    assert_eq!(output, "Updated remote.txt (1 replacement)");
    assert_eq!(
        fs.file_contents(&path).await.expect("recorded file"),
        b"after\n"
    );
    assert_eq!(
        fs.calls().await,
        vec![
            format!("read_file:{}", path.display()),
            format!("write_file:{}", path.display())
        ]
    );
    assert!(!temp_dir.path().join("remote.txt").exists());
}

#[tokio::test]
async fn grep_prunes_generated_and_vcs_directories() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    std::fs::create_dir_all(temp_dir.path().join("src")).expect("create src");
    std::fs::create_dir_all(temp_dir.path().join(".git")).expect("create git dir");
    std::fs::create_dir_all(temp_dir.path().join("target/debug")).expect("create target dir");
    std::fs::write(temp_dir.path().join("src/lib.rs"), "pub fn live() {}\n").expect("write source");
    std::fs::write(temp_dir.path().join(".git/config"), "[core]\n").expect("write git config");
    std::fs::write(
        temp_dir.path().join("target/debug/generated.rs"),
        "pub fn generated() {}\n",
    )
    .expect("write generated");
    let sandbox = FileSystemSandboxContext::from_permission_profile(PermissionProfile::Disabled);

    let output = grep_files(
        json!({
            "pattern": "pub fn",
            "path": ".",
            "output_mode": "files_with_matches"
        })
        .to_string(),
        LOCAL_FS.as_ref(),
        &sandbox,
        &cwd,
    )
    .await
    .expect("grep succeeds");

    assert_eq!(output, "src/lib.rs\n");
}

#[tokio::test]
async fn glob_uses_executor_search_backend() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    let fs = RecordingFileSystem::default();
    fs.set_glob_response(GlobSearchResponse {
        matches: vec![GlobSearchMatch {
            path: cwd.join("src/lib.rs"),
            modified_at_ms: 42,
        }],
        truncated: false,
    })
    .await;
    let sandbox = FileSystemSandboxContext::from_permission_profile(PermissionProfile::Disabled);

    let output = glob_files(
        json!({ "pattern": "**/*.rs", "path": "src" }).to_string(),
        &fs,
        &sandbox,
        &cwd,
    )
    .await
    .expect("glob succeeds");

    assert_eq!(output, "src/lib.rs\n");
    assert_eq!(
        fs.calls().await,
        vec![format!("glob_search:{}:**/*.rs", cwd.join("src").display())]
    );
}

#[tokio::test]
async fn grep_uses_executor_search_backend() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    let fs = RecordingFileSystem::default();
    fs.set_grep_response(GrepSearchResponse {
        lines: vec!["lib.rs:1:needle".to_string()],
        truncated: false,
    })
    .await;
    let sandbox = FileSystemSandboxContext::from_permission_profile(PermissionProfile::Disabled);

    let output = grep_files(
        json!({
            "pattern": "needle",
            "path": "src",
            "output_mode": "content"
        })
        .to_string(),
        &fs,
        &sandbox,
        &cwd,
    )
    .await
    .expect("grep succeeds");

    assert_eq!(output, "src/lib.rs:1:needle\n");
    assert_eq!(
        fs.calls().await,
        vec![format!("grep_search:{}:needle", cwd.join("src").display())]
    );
}

#[tokio::test]
async fn glob_pattern_without_slash_does_not_recurse() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    std::fs::create_dir_all(temp_dir.path().join("nested")).expect("create nested");
    std::fs::write(temp_dir.path().join("root.toml"), "").expect("write root");
    std::fs::write(temp_dir.path().join("nested/child.toml"), "").expect("write child");
    let sandbox = FileSystemSandboxContext::from_permission_profile(PermissionProfile::Disabled);

    let output = glob_files(
        json!({ "pattern": "*.toml", "path": "." }).to_string(),
        LOCAL_FS.as_ref(),
        &sandbox,
        &cwd,
    )
    .await
    .expect("glob succeeds");

    assert_eq!(output, "root.toml\n");
}

#[tokio::test]
async fn glob_uses_literal_prefix_for_fixed_depth_patterns() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    std::fs::create_dir_all(temp_dir.path().join("src")).expect("create src");
    std::fs::create_dir_all(temp_dir.path().join("tests")).expect("create tests");
    std::fs::write(temp_dir.path().join("src/lib.rs"), "").expect("write source");
    std::fs::write(temp_dir.path().join("tests/lib.rs"), "").expect("write test source");
    let sandbox = FileSystemSandboxContext::from_permission_profile(PermissionProfile::Disabled);

    let output = glob_files(
        json!({ "pattern": "src/*.rs", "path": "." }).to_string(),
        LOCAL_FS.as_ref(),
        &sandbox,
        &cwd,
    )
    .await
    .expect("glob succeeds");

    assert_eq!(output, "src/lib.rs\n");
}

#[tokio::test]
async fn glob_double_star_recurses_under_literal_prefix() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    std::fs::create_dir_all(temp_dir.path().join("src/nested")).expect("create nested src");
    std::fs::create_dir_all(temp_dir.path().join("tests/nested")).expect("create nested tests");
    std::fs::write(temp_dir.path().join("src/lib.rs"), "").expect("write source");
    std::fs::write(temp_dir.path().join("src/nested/mod.rs"), "").expect("write nested source");
    std::fs::write(temp_dir.path().join("tests/nested/mod.rs"), "").expect("write test source");
    let sandbox = FileSystemSandboxContext::from_permission_profile(PermissionProfile::Disabled);

    let output = glob_files(
        json!({ "pattern": "src/**/*.rs", "path": "." }).to_string(),
        LOCAL_FS.as_ref(),
        &sandbox,
        &cwd,
    )
    .await
    .expect("glob succeeds");
    let mut lines = output.lines().collect::<Vec<_>>();
    lines.sort();

    assert_eq!(lines, vec!["src/lib.rs", "src/nested/mod.rs"]);
}

#[tokio::test]
async fn grep_glob_pattern_without_slash_does_not_recurse() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    std::fs::create_dir_all(temp_dir.path().join("nested")).expect("create nested");
    std::fs::write(temp_dir.path().join("root.md"), "needle\n").expect("write root");
    std::fs::write(temp_dir.path().join("nested/child.md"), "needle\n").expect("write child");
    let sandbox = FileSystemSandboxContext::from_permission_profile(PermissionProfile::Disabled);

    let output = grep_files(
        json!({
            "pattern": "needle",
            "path": ".",
            "glob": "*.md",
            "output_mode": "files_with_matches"
        })
        .to_string(),
        LOCAL_FS.as_ref(),
        &sandbox,
        &cwd,
    )
    .await
    .expect("grep succeeds");

    assert_eq!(output, "root.md\n");
}
