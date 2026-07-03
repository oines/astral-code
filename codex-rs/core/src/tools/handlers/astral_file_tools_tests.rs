use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::time::Duration;

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
use codex_exec_server::LOCAL_ENVIRONMENT_ID;
use codex_exec_server::LOCAL_FS;
use codex_exec_server::ReadDirectoryEntry;
use codex_exec_server::RemoveOptions;
use codex_protocol::models::AdditionalPermissionProfile;
use codex_protocol::models::FileSystemPermissions;
use codex_protocol::models::PermissionProfile;
use codex_protocol::permissions::FileSystemAccessMode;
use codex_protocol::permissions::FileSystemPath;
use codex_protocol::permissions::FileSystemSandboxEntry;
use codex_protocol::permissions::FileSystemSandboxPolicy;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::FileChange;
use codex_utils_absolute_path::AbsolutePathBuf;
use core_test_support::PathExt;

use super::AstralFileToolTextOutput;
use super::DEFAULT_READ_MAX_BYTES_WITHOUT_LIMIT;
use super::DEFAULT_READ_MAX_OUTPUT_TOKENS;
use super::EMPTY_FILE_REMINDER;
use super::FILE_HAS_NOT_BEEN_READ_ERROR;
use super::FILE_MODIFIED_SINCE_READ_ERROR;
use super::FILE_UNCHANGED_STUB;
use super::FileReadStateStore;
use super::GrepArgs;
use super::ReadArgs;
use super::add_line_numbers;
use super::edit_file;
use super::file_environment_id;
use super::file_tool_exec_approval_requirement;
use super::file_tool_permission_targets;
use super::glob_files;
use super::grep_files;
use super::is_blocked_device_path;
use super::read_file;
use super::read_state_key;
use super::split_lines_preserving_newline;
use super::write_file;
use crate::function_tool::FunctionCallError;
use crate::tools::handlers::AstralFileToolKind;
use crate::tools::sandboxing::ExecApprovalRequirement;

#[derive(Default)]
struct RecordingFileSystem {
    files: Mutex<HashMap<String, Vec<u8>>>,
    directories: Mutex<HashSet<String>>,
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

    async fn insert_directory(&self, path: &AbsolutePathBuf) {
        self.directories.lock().await.insert(path_key(path));
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

fn read_args(file_path: &str) -> ReadArgs {
    ReadArgs {
        file_path: file_path.to_string(),
        offset: None,
        limit: None,
    }
}

fn model_error(error: FunctionCallError) -> String {
    let FunctionCallError::RespondToModel(message) = error else {
        panic!("expected model-facing error");
    };
    message
}

fn single_file_change<'a>(
    output: &'a AstralFileToolTextOutput,
    path: &AbsolutePathBuf,
) -> &'a FileChange {
    output
        .file_changes
        .as_ref()
        .and_then(|changes| changes.get(&path.to_path_buf()))
        .unwrap_or_else(|| panic!("expected file change for {}", path.display()))
}

struct FileToolFixture {
    _temp_dir: tempfile::TempDir,
    cwd: AbsolutePathBuf,
    path: AbsolutePathBuf,
    fs: RecordingFileSystem,
    sandbox: FileSystemSandboxContext,
    read_state: FileReadStateStore,
}

impl FileToolFixture {
    async fn with_file(file_name: &str, contents: impl Into<Vec<u8>>) -> Self {
        let fixture = Self::without_file(file_name);
        fixture.fs.insert_file(&fixture.path, contents).await;
        fixture
    }

    fn without_file(file_name: &str) -> Self {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let cwd = temp_dir.path().abs();
        let path = cwd.join(file_name);
        Self {
            _temp_dir: temp_dir,
            cwd,
            path,
            fs: RecordingFileSystem::default(),
            sandbox: FileSystemSandboxContext::from_permission_profile(PermissionProfile::Disabled),
            read_state: FileReadStateStore::default(),
        }
    }

    async fn read_full(&self, file_name: &str) {
        read_file(
            read_args(file_name),
            &self.fs,
            &self.sandbox,
            &self.cwd,
            LOCAL_ENVIRONMENT_ID,
            &self.read_state,
        )
        .await
        .expect("read succeeds");
    }
}

async fn write_remote(
    fixture: &FileToolFixture,
    content: &str,
) -> Result<AstralFileToolTextOutput, FunctionCallError> {
    write_file(
        json!({ "file_path": "remote.txt", "content": content }).to_string(),
        &fixture.fs,
        &fixture.sandbox,
        &fixture.cwd,
        LOCAL_ENVIRONMENT_ID,
        &fixture.read_state,
    )
    .await
}

async fn edit_remote(
    fixture: &FileToolFixture,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<AstralFileToolTextOutput, FunctionCallError> {
    edit_file(
        json!({
            "file_path": "remote.txt",
            "old_string": old_string,
            "new_string": new_string,
            "replace_all": replace_all,
        })
        .to_string(),
        &fixture.fs,
        &fixture.sandbox,
        &fixture.cwd,
        LOCAL_ENVIRONMENT_ID,
        &fixture.read_state,
    )
    .await
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
        path: &AbsolutePathBuf,
        create_directory_options: CreateDirectoryOptions,
        _sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<()> {
        self.calls.lock().await.push(format!(
            "create_directory:{}:{}",
            path.display(),
            create_directory_options.recursive
        ));
        self.insert_directory(path).await;
        Ok(())
    }

    async fn get_metadata(
        &self,
        path: &AbsolutePathBuf,
        _sandbox: Option<&FileSystemSandboxContext>,
    ) -> FileSystemResult<FileMetadata> {
        if self.directories.lock().await.contains(&path_key(path)) {
            Ok(FileMetadata {
                is_directory: true,
                is_file: false,
                is_symlink: false,
                created_at_ms: 0,
                modified_at_ms: 0,
            })
        } else if self.file_contents(path).await.is_some() {
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
                num_files: 0,
                num_matches: None,
                applied_limit: None,
                applied_offset: None,
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
async fn read_state_key_uses_resolved_environment_id() {
    let fixture = FileToolFixture::with_file("remote.txt", b"contents\n").await;

    let key = read_state_key(&fixture.fs, &fixture.sandbox, "remote-env", &fixture.path)
        .await
        .expect("state key");

    assert_eq!(key.environment_id, "remote-env");
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

    assert_eq!(output, "lib.rs:2:needle");
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
async fn read_file_formats_text_like_cat_n() {
    let fixture = FileToolFixture::with_file("sample.txt", b"alpha\nbeta\n").await;

    let output = read_file(
        read_args("sample.txt"),
        &fixture.fs,
        &fixture.sandbox,
        &fixture.cwd,
        LOCAL_ENVIRONMENT_ID,
        &fixture.read_state,
    )
    .await
    .expect("read succeeds");

    assert_eq!(output, "1\talpha\n2\tbeta\n");
}

#[tokio::test]
async fn read_file_reports_empty_and_offset_past_end_like_claude() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    let empty_path = cwd.join("empty.txt");
    let short_path = cwd.join("short.txt");
    let fs = RecordingFileSystem::default();
    fs.insert_file(&empty_path, b"").await;
    fs.insert_file(&short_path, b"one\ntwo\n").await;
    let sandbox = FileSystemSandboxContext::from_permission_profile(PermissionProfile::Disabled);
    let read_state = FileReadStateStore::default();

    let empty = read_file(
        read_args("empty.txt"),
        &fs,
        &sandbox,
        &cwd,
        LOCAL_ENVIRONMENT_ID,
        &read_state,
    )
    .await
    .expect("read empty succeeds");
    let past_end = read_file(
        ReadArgs {
            offset: Some(5),
            ..read_args("short.txt")
        },
        &fs,
        &sandbox,
        &cwd,
        LOCAL_ENVIRONMENT_ID,
        &read_state,
    )
    .await
    .expect("read past end succeeds");

    assert_eq!(empty, EMPTY_FILE_REMINDER);
    assert_eq!(
        past_end,
        "<system-reminder>Warning: the file exists but is shorter than the provided offset (5). The file has 2 lines.</system-reminder>"
    );
}

#[tokio::test]
async fn read_file_repeated_unchanged_full_read_returns_stub() {
    let fixture = FileToolFixture::with_file("sample.txt", b"alpha\nbeta\n").await;

    let first = read_file(
        read_args("sample.txt"),
        &fixture.fs,
        &fixture.sandbox,
        &fixture.cwd,
        LOCAL_ENVIRONMENT_ID,
        &fixture.read_state,
    )
    .await
    .expect("first read succeeds");
    let second = read_file(
        read_args("sample.txt"),
        &fixture.fs,
        &fixture.sandbox,
        &fixture.cwd,
        LOCAL_ENVIRONMENT_ID,
        &fixture.read_state,
    )
    .await
    .expect("second read succeeds");

    assert_eq!(first, "1\talpha\n2\tbeta\n");
    assert_eq!(second, FILE_UNCHANGED_STUB);
}

#[tokio::test]
async fn read_file_partial_output_omits_astral_footer() {
    let fixture = FileToolFixture::with_file("sample.txt", b"one\ntwo\nthree\n").await;

    let output = read_file(
        ReadArgs {
            limit: Some(2),
            ..read_args("sample.txt")
        },
        &fixture.fs,
        &fixture.sandbox,
        &fixture.cwd,
        LOCAL_ENVIRONMENT_ID,
        &fixture.read_state,
    )
    .await
    .expect("read succeeds");

    assert_eq!(output, "1\tone\n2\ttwo\n");
    assert!(!output.contains("[Showing lines"));
}

#[tokio::test]
async fn read_without_limit_rejects_large_file() {
    let fixture = FileToolFixture::with_file(
        "large.txt",
        vec![b'a'; DEFAULT_READ_MAX_BYTES_WITHOUT_LIMIT + 1],
    )
    .await;

    let error = read_file(
        read_args("large.txt"),
        &fixture.fs,
        &fixture.sandbox,
        &fixture.cwd,
        LOCAL_ENVIRONMENT_ID,
        &fixture.read_state,
    )
    .await
    .expect_err("large unbounded read should fail");

    assert_eq!(
        model_error(error),
        format!(
            "File content ({}) exceeds maximum allowed size ({}). Use offset and limit parameters to read specific portions of the file.",
            DEFAULT_READ_MAX_BYTES_WITHOUT_LIMIT + 1,
            DEFAULT_READ_MAX_BYTES_WITHOUT_LIMIT
        )
    );
}

#[tokio::test]
async fn read_rejects_output_over_token_limit() {
    let contents = "token ".repeat(DEFAULT_READ_MAX_OUTPUT_TOKENS + 1_000);
    let fixture = FileToolFixture::with_file("long.txt", contents).await;

    let error = read_file(
        ReadArgs {
            limit: Some(1),
            ..read_args("long.txt")
        },
        &fixture.fs,
        &fixture.sandbox,
        &fixture.cwd,
        LOCAL_ENVIRONMENT_ID,
        &fixture.read_state,
    )
    .await
    .expect_err("token-heavy output should fail");

    let message = model_error(error);
    assert!(message.contains("exceeds maximum allowed tokens (25000)"));
    assert!(message.contains("Use offset and limit parameters"));
}

#[tokio::test]
async fn edit_empty_old_string_creates_missing_file() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    let sandbox = FileSystemSandboxContext::from_permission_profile(PermissionProfile::Disabled);
    let read_state = FileReadStateStore::default();
    let arguments = json!({
        "file_path": "created.txt",
        "old_string": "",
        "new_string": "created content\n"
    })
    .to_string();

    let output = edit_file(
        arguments,
        LOCAL_FS.as_ref(),
        &sandbox,
        &cwd,
        LOCAL_ENVIRONMENT_ID,
        &read_state,
    )
    .await
    .expect("edit succeeds");

    assert_eq!(
        output.text,
        format!(
            "The file {} has been updated successfully.",
            cwd.join("created.txt").display()
        )
    );
    assert_eq!(
        single_file_change(&output, &cwd.join("created.txt")),
        &FileChange::Add {
            content: "created content\n".to_string()
        }
    );
    assert_eq!(
        std::fs::read_to_string(temp_dir.path().join("created.txt")).expect("created file"),
        "created content\n"
    );
}

#[tokio::test]
async fn edit_empty_old_string_rejects_existing_nonempty_file() {
    let fixture = FileToolFixture::with_file("remote.txt", b"already here\n").await;

    let error = edit_file(
        json!({
            "file_path": "remote.txt",
            "old_string": "",
            "new_string": "created\n"
        })
        .to_string(),
        &fixture.fs,
        &fixture.sandbox,
        &fixture.cwd,
        LOCAL_ENVIRONMENT_ID,
        &fixture.read_state,
    )
    .await
    .expect_err("creating existing file should fail");

    assert_eq!(
        model_error(error),
        "Cannot create new file - file already exists."
    );
}

#[tokio::test]
async fn write_uses_executor_file_system() {
    let fixture = FileToolFixture::without_file("remote.txt");
    let arguments = json!({
        "file_path": "remote.txt",
        "content": "written through backend\n"
    })
    .to_string();

    let output = write_file(
        arguments,
        &fixture.fs,
        &fixture.sandbox,
        &fixture.cwd,
        LOCAL_ENVIRONMENT_ID,
        &fixture.read_state,
    )
    .await
    .expect("write succeeds");

    assert_eq!(
        output.text,
        format!("File created successfully at: {}", fixture.path.display())
    );
    assert_eq!(
        single_file_change(&output, &fixture.path),
        &FileChange::Add {
            content: "written through backend\n".to_string()
        }
    );
    assert_eq!(
        fixture
            .fs
            .file_contents(&fixture.path)
            .await
            .expect("recorded file"),
        b"written through backend\n"
    );
    assert_eq!(
        fixture.fs.calls().await,
        vec![
            format!("create_directory:{}:true", fixture.cwd.display()),
            format!("write_file:{}", fixture.path.display())
        ]
    );
    assert!(!fixture.path.exists());
}

#[tokio::test]
async fn write_creates_missing_parent_directory() {
    let fixture = FileToolFixture::without_file("nested/remote.txt");
    let arguments = json!({
        "file_path": "nested/remote.txt",
        "content": "nested content\n"
    })
    .to_string();

    let output = write_file(
        arguments,
        &fixture.fs,
        &fixture.sandbox,
        &fixture.cwd,
        LOCAL_ENVIRONMENT_ID,
        &fixture.read_state,
    )
    .await
    .expect("write succeeds");

    assert_eq!(
        output.text,
        format!("File created successfully at: {}", fixture.path.display())
    );
    assert_eq!(
        fixture
            .fs
            .file_contents(&fixture.path)
            .await
            .expect("recorded file"),
        b"nested content\n"
    );
    assert_eq!(
        fixture.fs.calls().await,
        vec![
            format!(
                "create_directory:{}:true",
                fixture.cwd.join("nested").display()
            ),
            format!("write_file:{}", fixture.path.display())
        ]
    );
}

#[tokio::test]
async fn write_existing_file_requires_full_read() {
    let fixture = FileToolFixture::with_file("remote.txt", b"before\n").await;

    let error = write_remote(&fixture, "after\n")
        .await
        .expect_err("unread write should fail");

    assert_eq!(model_error(error), FILE_HAS_NOT_BEEN_READ_ERROR);
    assert_eq!(
        fixture
            .fs
            .file_contents(&fixture.path)
            .await
            .expect("recorded file"),
        b"before\n"
    );
}

#[tokio::test]
async fn write_existing_file_succeeds_after_full_read() {
    let fixture = FileToolFixture::with_file("remote.txt", b"before\n").await;
    fixture.read_full("remote.txt").await;
    let output = write_remote(&fixture, "after\n")
        .await
        .expect("write succeeds");

    assert_eq!(
        output.text,
        format!(
            "The file {} has been updated successfully.",
            fixture.path.display()
        )
    );
    let FileChange::Update {
        unified_diff,
        move_path,
    } = single_file_change(&output, &fixture.path)
    else {
        panic!("expected update change");
    };
    assert_eq!(move_path, &None);
    assert!(unified_diff.contains("-before"));
    assert!(unified_diff.contains("+after"));
    assert_eq!(
        fixture
            .fs
            .file_contents(&fixture.path)
            .await
            .expect("recorded file"),
        b"after\n"
    );
}

#[tokio::test]
async fn write_existing_file_succeeds_after_limited_read() {
    let fixture = FileToolFixture::with_file("remote.txt", b"before\nsecond\n").await;

    read_file(
        ReadArgs {
            limit: Some(1),
            ..read_args("remote.txt")
        },
        &fixture.fs,
        &fixture.sandbox,
        &fixture.cwd,
        LOCAL_ENVIRONMENT_ID,
        &fixture.read_state,
    )
    .await
    .expect("limited read succeeds");
    let output = write_remote(&fixture, "after\n")
        .await
        .expect("write succeeds");

    assert_eq!(
        output.text,
        format!(
            "The file {} has been updated successfully.",
            fixture.path.display()
        )
    );
    assert_eq!(
        fixture
            .fs
            .file_contents(&fixture.path)
            .await
            .expect("recorded file"),
        b"after\n"
    );
}

#[tokio::test]
async fn write_existing_file_rejects_external_modification_after_read() {
    let fixture = FileToolFixture::with_file("remote.txt", b"before\n").await;
    fixture.read_full("remote.txt").await;
    fixture
        .fs
        .insert_file(&fixture.path, b"user changed\n")
        .await;
    let error = write_remote(&fixture, "after\n")
        .await
        .expect_err("stale write should fail");

    assert_eq!(model_error(error), FILE_MODIFIED_SINCE_READ_ERROR);
    assert_eq!(
        fixture
            .fs
            .file_contents(&fixture.path)
            .await
            .expect("recorded file"),
        b"user changed\n"
    );
}

#[tokio::test]
async fn edit_uses_executor_file_system() {
    let fixture = FileToolFixture::with_file("remote.txt", b"before\n").await;

    fixture.read_full("remote.txt").await;
    let output = edit_remote(&fixture, "before", "after", false)
        .await
        .expect("edit succeeds");

    assert_eq!(
        output.text,
        format!(
            "The file {} has been updated successfully.",
            fixture.path.display()
        )
    );
    let FileChange::Update {
        unified_diff,
        move_path,
    } = single_file_change(&output, &fixture.path)
    else {
        panic!("expected update change");
    };
    assert_eq!(move_path, &None);
    assert!(unified_diff.contains("-before"));
    assert!(unified_diff.contains("+after"));
    assert_eq!(
        fixture
            .fs
            .file_contents(&fixture.path)
            .await
            .expect("recorded file"),
        b"after\n"
    );
    assert_eq!(
        fixture.fs.calls().await,
        vec![
            format!("read_file:{}", fixture.path.display()),
            format!("read_file:{}", fixture.path.display()),
            format!("create_directory:{}:true", fixture.cwd.display()),
            format!("write_file:{}", fixture.path.display())
        ]
    );
    assert!(!fixture.path.exists());
}

#[tokio::test]
async fn edit_empty_old_string_creates_missing_parent_directory() {
    let fixture = FileToolFixture::without_file("nested/created.txt");
    let output = edit_file(
        json!({
            "file_path": "nested/created.txt",
            "old_string": "",
            "new_string": "created\n"
        })
        .to_string(),
        &fixture.fs,
        &fixture.sandbox,
        &fixture.cwd,
        LOCAL_ENVIRONMENT_ID,
        &fixture.read_state,
    )
    .await
    .expect("edit create succeeds");

    assert_eq!(
        output.text,
        format!(
            "The file {} has been updated successfully.",
            fixture.path.display()
        )
    );
    assert_eq!(
        fixture
            .fs
            .file_contents(&fixture.path)
            .await
            .expect("recorded file"),
        b"created\n"
    );
    assert_eq!(
        fixture.fs.calls().await,
        vec![
            format!(
                "create_directory:{}:true",
                fixture.cwd.join("nested").display()
            ),
            format!("write_file:{}", fixture.path.display())
        ]
    );
}

#[tokio::test]
async fn edit_requires_read_but_allows_limited_read_for_existing_file() {
    let fixture = FileToolFixture::with_file("remote.txt", b"before\nsecond\n").await;

    let unread_error = edit_remote(&fixture, "before", "after", false)
        .await
        .expect_err("unread edit should fail");
    read_file(
        ReadArgs {
            limit: Some(1),
            ..read_args("remote.txt")
        },
        &fixture.fs,
        &fixture.sandbox,
        &fixture.cwd,
        LOCAL_ENVIRONMENT_ID,
        &fixture.read_state,
    )
    .await
    .expect("limited read succeeds");
    let output = edit_remote(&fixture, "before", "after", false)
        .await
        .expect("edit succeeds");

    assert_eq!(model_error(unread_error), FILE_HAS_NOT_BEEN_READ_ERROR);
    assert_eq!(
        output.text,
        format!(
            "The file {} has been updated successfully.",
            fixture.path.display()
        )
    );
    assert_eq!(
        fixture
            .fs
            .file_contents(&fixture.path)
            .await
            .expect("recorded file"),
        b"after\nsecond\n"
    );
}

#[tokio::test]
async fn edit_reports_claude_style_validation_errors() {
    let fixture = FileToolFixture::with_file("remote.txt", b"alpha\nbeta\nbeta\n").await;
    fixture.read_full("remote.txt").await;
    let same = edit_remote(&fixture, "alpha", "alpha", false)
        .await
        .expect_err("same edit should fail");
    let missing = edit_remote(&fixture, "gamma", "delta", false)
        .await
        .expect_err("missing old_string should fail");
    let multiple = edit_remote(&fixture, "beta", "delta", false)
        .await
        .expect_err("multi match should fail");

    assert_eq!(
        model_error(same),
        "No changes to make: old_string and new_string are exactly the same."
    );
    assert_eq!(
        model_error(missing),
        "String to replace not found in file.\nString: gamma"
    );
    assert_eq!(
        model_error(multiple),
        "Found 2 matches of the string to replace, but replace_all is false. To replace all occurrences, set replace_all to true. To replace only one occurrence, please provide more context to uniquely identify the instance.\nString: beta"
    );
}

#[tokio::test]
async fn edit_replace_all_uses_claude_success_message() {
    let fixture = FileToolFixture::with_file("remote.txt", b"beta\nbeta\n").await;
    fixture.read_full("remote.txt").await;
    let output = edit_remote(&fixture, "beta", "delta", true)
        .await
        .expect("replace_all succeeds");

    assert_eq!(
        output.text,
        format!(
            "The file {} has been updated. All occurrences were successfully replaced.",
            fixture.path.display()
        )
    );
    assert_eq!(
        fixture
            .fs
            .file_contents(&fixture.path)
            .await
            .expect("recorded file"),
        b"delta\ndelta\n"
    );
}

#[tokio::test]
async fn edit_rejects_external_modification_after_read() {
    let fixture = FileToolFixture::with_file("remote.txt", b"before\n").await;
    fixture.read_full("remote.txt").await;
    fixture
        .fs
        .insert_file(&fixture.path, b"user changed\n")
        .await;
    let error = edit_remote(&fixture, "before", "after", false)
        .await
        .expect_err("stale edit should fail");

    assert_eq!(model_error(error), FILE_MODIFIED_SINCE_READ_ERROR);
    assert_eq!(
        fixture
            .fs
            .file_contents(&fixture.path)
            .await
            .expect("recorded file"),
        b"user changed\n"
    );
}

#[tokio::test]
async fn grep_excludes_vcs_directories_but_not_generated_directories() {
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

    let mut lines = output.lines();
    assert_eq!(lines.next(), Some("Found 2 files"));
    let mut files = lines.collect::<Vec<_>>();
    files.sort();
    assert_eq!(files, vec!["src/lib.rs", "target/debug/generated.rs"]);
}

#[tokio::test]
async fn glob_uses_executor_search_backend() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    let fs = RecordingFileSystem::default();
    fs.insert_directory(&cwd.join("src")).await;
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

    assert_eq!(output, "src/lib.rs");
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
    fs.insert_directory(&cwd.join("src")).await;
    fs.set_grep_response(GrepSearchResponse {
        lines: vec!["lib.rs:1:needle".to_string()],
        num_files: 0,
        num_matches: None,
        applied_limit: None,
        applied_offset: None,
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

    assert_eq!(output, "src/lib.rs:1:needle");
    assert_eq!(
        fs.calls().await,
        vec![format!("grep_search:{}:needle", cwd.join("src").display())]
    );
}

#[tokio::test]
async fn glob_formats_empty_and_truncated_results_like_claude() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    let fs = RecordingFileSystem::default();
    let sandbox = FileSystemSandboxContext::from_permission_profile(PermissionProfile::Disabled);

    let empty = glob_files(
        json!({ "pattern": "*.rs" }).to_string(),
        &fs,
        &sandbox,
        &cwd,
    )
    .await
    .expect("glob succeeds");
    assert_eq!(empty, "No files found");

    fs.set_glob_response(GlobSearchResponse {
        matches: vec![GlobSearchMatch {
            path: cwd.join("src/lib.rs"),
            modified_at_ms: 42,
        }],
        truncated: true,
    })
    .await;
    let truncated = glob_files(
        json!({ "pattern": "*.rs" }).to_string(),
        &fs,
        &sandbox,
        &cwd,
    )
    .await
    .expect("glob succeeds");
    assert_eq!(
        truncated,
        "src/lib.rs\n(Results are truncated. Consider using a more specific path or pattern.)"
    );
}

#[tokio::test]
async fn grep_formats_files_count_content_and_pagination_like_claude() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    let fs = RecordingFileSystem::default();
    let sandbox = FileSystemSandboxContext::from_permission_profile(PermissionProfile::Disabled);

    fs.set_grep_response(GrepSearchResponse {
        lines: vec!["src/lib.rs".to_string(), "src/main.rs".to_string()],
        num_files: 2,
        num_matches: None,
        applied_limit: Some(2),
        applied_offset: Some(5),
        truncated: true,
    })
    .await;
    let files = grep_files(
        json!({ "pattern": "needle", "output_mode": "files_with_matches" }).to_string(),
        &fs,
        &sandbox,
        &cwd,
    )
    .await
    .expect("grep succeeds");
    assert_eq!(
        files,
        "Found 2 files limit: 2, offset: 5\nsrc/lib.rs\nsrc/main.rs"
    );

    fs.set_grep_response(GrepSearchResponse {
        lines: vec!["src/lib.rs:3".to_string()],
        num_files: 1,
        num_matches: Some(3),
        applied_limit: None,
        applied_offset: None,
        truncated: false,
    })
    .await;
    let count = grep_files(
        json!({ "pattern": "needle", "output_mode": "count" }).to_string(),
        &fs,
        &sandbox,
        &cwd,
    )
    .await
    .expect("grep succeeds");
    assert_eq!(
        count,
        "src/lib.rs:3\n\nFound 3 total occurrences across 1 file."
    );

    fs.set_grep_response(GrepSearchResponse {
        lines: Vec::new(),
        num_files: 0,
        num_matches: None,
        applied_limit: Some(250),
        applied_offset: Some(10),
        truncated: true,
    })
    .await;
    let content = grep_files(
        json!({ "pattern": "needle", "output_mode": "content" }).to_string(),
        &fs,
        &sandbox,
        &cwd,
    )
    .await
    .expect("grep succeeds");
    assert_eq!(
        content,
        "No matches found\n\n[Showing results with pagination = limit: 250, offset: 10]"
    );
}

#[tokio::test]
async fn grep_omitted_path_searches_cwd() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    let fs = RecordingFileSystem::default();
    let sandbox = FileSystemSandboxContext::from_permission_profile(PermissionProfile::Disabled);

    let _ = grep_files(
        json!({ "pattern": "needle" }).to_string(),
        &fs,
        &sandbox,
        &cwd,
    )
    .await
    .expect("grep succeeds");

    assert_eq!(
        fs.calls().await,
        vec![format!("grep_search:{}:needle", cwd.display())]
    );
}

#[tokio::test]
async fn glob_pattern_without_slash_recurses() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    std::fs::create_dir_all(temp_dir.path().join("nested")).expect("create nested");
    std::fs::write(temp_dir.path().join("root.toml"), "").expect("write root");
    std::thread::sleep(Duration::from_millis(20));
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

    assert_eq!(output, "root.toml\nnested/child.toml");
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

    assert_eq!(output, "src/lib.rs");
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
async fn grep_glob_pattern_without_slash_recurses() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    std::fs::create_dir_all(temp_dir.path().join("nested")).expect("create nested");
    std::fs::write(temp_dir.path().join("root.md"), "needle\n").expect("write root");
    std::thread::sleep(Duration::from_millis(20));
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

    assert_eq!(output, "Found 2 files\nnested/child.md\nroot.md");
}

#[test]
fn read_permission_targets_skip_when_policy_already_allows_read() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    let policy = FileSystemSandboxPolicy::read_only();

    let plan = file_tool_permission_targets(
        AstralFileToolKind::Read,
        &json!({ "file_path": "notes.txt" }).to_string(),
        &policy,
        &cwd,
    )
    .expect("permission targets");

    assert_eq!(
        plan.approval_command,
        vec![
            "Read".to_string(),
            cwd.join("notes.txt").display().to_string()
        ]
    );
    assert_eq!(plan.additional_permissions, None);
}

#[test]
fn read_permission_targets_request_read_when_policy_denies_path() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    let path = cwd.join("secret.txt");
    let policy = FileSystemSandboxPolicy::restricted(vec![]);

    let plan = file_tool_permission_targets(
        AstralFileToolKind::Read,
        &json!({ "file_path": "secret.txt" }).to_string(),
        &policy,
        &cwd,
    )
    .expect("permission targets");

    assert_eq!(
        plan.additional_permissions,
        Some(AdditionalPermissionProfile {
            file_system: Some(FileSystemPermissions::from_read_write_roots(
                Some(vec![path]),
                Some(vec![]),
            )),
            ..Default::default()
        })
    );
}

#[test]
fn write_permission_targets_skip_when_parent_is_writable() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().abs();
    let policy = FileSystemSandboxPolicy::restricted(vec![FileSystemSandboxEntry {
        path: FileSystemPath::Path { path: cwd.clone() },
        access: FileSystemAccessMode::Write,
    }]);

    let plan = file_tool_permission_targets(
        AstralFileToolKind::Write,
        &json!({ "file_path": "notes.txt", "content": "hello" }).to_string(),
        &policy,
        &cwd,
    )
    .expect("permission targets");

    assert_eq!(plan.additional_permissions, None);
}

#[test]
fn write_permission_targets_request_parent_write_when_policy_denies_path() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let cwd = temp_dir.path().join("workspace").abs();
    let outside_dir = temp_dir.path().join("outside").abs();
    let outside_file = outside_dir.join("notes.txt");
    let policy = FileSystemSandboxPolicy::restricted(vec![FileSystemSandboxEntry {
        path: FileSystemPath::Path { path: cwd.clone() },
        access: FileSystemAccessMode::Write,
    }]);

    let plan = file_tool_permission_targets(
        AstralFileToolKind::Write,
        &json!({ "file_path": outside_file.to_string_lossy(), "content": "hello" }).to_string(),
        &policy,
        &cwd,
    )
    .expect("permission targets");

    assert_eq!(
        plan.additional_permissions,
        Some(AdditionalPermissionProfile {
            file_system: Some(FileSystemPermissions::from_read_write_roots(
                Some(vec![]),
                Some(vec![outside_dir]),
            )),
            ..Default::default()
        })
    );
}

#[test]
fn permission_prompt_policy_rejects_when_approval_is_disabled() {
    let permissions = AdditionalPermissionProfile {
        file_system: Some(FileSystemPermissions::from_read_write_roots(
            Some(vec![std::env::temp_dir().abs()]),
            Some(vec![]),
        )),
        ..Default::default()
    };

    let requirement =
        file_tool_exec_approval_requirement(AskForApproval::Never, Some(&permissions), false);

    assert_eq!(
        requirement,
        ExecApprovalRequirement::Forbidden {
            reason: "approval policy disallowed filesystem permission prompt".to_string(),
        }
    );
}

#[test]
fn permission_prompt_policy_skips_when_permissions_are_preapproved() {
    let permissions = AdditionalPermissionProfile {
        file_system: Some(FileSystemPermissions::from_read_write_roots(
            Some(vec![std::env::temp_dir().abs()]),
            Some(vec![]),
        )),
        ..Default::default()
    };

    let requirement =
        file_tool_exec_approval_requirement(AskForApproval::Never, Some(&permissions), true);

    assert_eq!(
        requirement,
        ExecApprovalRequirement::Skip {
            bypass_sandbox: false,
            proposed_execpolicy_amendment: None,
        }
    );
}
