use std::time::Duration;

use codex_exec_server::FileSystemSandboxContext;
use codex_exec_server::LOCAL_ENVIRONMENT_ID;
use codex_exec_server::LOCAL_FS;
use codex_protocol::models::PermissionProfile;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::EMPTY_FILE_REMINDER;
use super::FileReadStateStore;
use super::ReadArgs;
use super::glob_files;
use super::grep_files;
use super::read_file;

#[tokio::test]
async fn claude_code_read_golden_line_numbers_and_reminders() {
    let fixture = FileGoldenFixture::new();
    fixture.write("sample.txt", "alpha\nbeta\ngamma\n");
    fixture.write("empty.txt", "");
    fixture.write("short.txt", "one\ntwo\n");
    let read_state = FileReadStateStore::default();

    let partial = read_file(
        ReadArgs {
            file_path: "sample.txt".to_string(),
            offset: Some(2),
            limit: Some(2),
        },
        LOCAL_FS.as_ref(),
        &fixture.sandbox,
        &fixture.cwd,
        LOCAL_ENVIRONMENT_ID,
        &read_state,
    )
    .await
    .expect("read succeeds");
    let empty = read_file(
        read_args("empty.txt"),
        LOCAL_FS.as_ref(),
        &fixture.sandbox,
        &fixture.cwd,
        LOCAL_ENVIRONMENT_ID,
        &read_state,
    )
    .await
    .expect("empty read succeeds");
    let past_end = read_file(
        ReadArgs {
            offset: Some(9),
            ..read_args("short.txt")
        },
        LOCAL_FS.as_ref(),
        &fixture.sandbox,
        &fixture.cwd,
        LOCAL_ENVIRONMENT_ID,
        &read_state,
    )
    .await
    .expect("past-end read succeeds");

    assert_eq!(partial, "2\tbeta\n3\tgamma\n");
    assert_eq!(empty, EMPTY_FILE_REMINDER);
    assert_eq!(
        past_end,
        "<system-reminder>Warning: the file exists but is shorter than the provided offset (9). The file has 2 lines.</system-reminder>"
    );
}

#[tokio::test]
async fn claude_code_glob_golden_mtime_order_and_relative_paths() {
    let fixture = FileGoldenFixture::new();
    fixture.create_dir("src/nested");
    fixture.write("src/old.toml", "");
    std::thread::sleep(Duration::from_millis(20));
    fixture.write("src/new.toml", "");
    std::thread::sleep(Duration::from_millis(20));
    fixture.write("src/nested/child.toml", "");

    let output = glob_files(
        json!({ "pattern": "*.toml", "path": "src" }).to_string(),
        LOCAL_FS.as_ref(),
        &fixture.sandbox,
        &fixture.cwd,
    )
    .await
    .expect("glob succeeds");

    assert_eq!(output, "src/old.toml\nsrc/new.toml\nsrc/nested/child.toml");
}

#[tokio::test]
async fn claude_code_glob_grep_golden_hidden_and_no_ignore_defaults() {
    let fixture = FileGoldenFixture::new();
    fixture.create_dir(".git");
    fixture.write(".gitignore", "ignored.rs\nignored.txt\n");
    fixture.write("ignored.rs", "");
    fixture.write("ignored.txt", "needle\n");
    std::thread::sleep(Duration::from_millis(20));
    fixture.write(".hidden.rs", "");
    fixture.write(".hidden.txt", "needle\n");

    let glob_output = glob_files(
        json!({ "pattern": "*.rs" }).to_string(),
        LOCAL_FS.as_ref(),
        &fixture.sandbox,
        &fixture.cwd,
    )
    .await
    .expect("glob succeeds");
    let grep_output = grep_files(
        json!({ "pattern": "needle" }).to_string(),
        LOCAL_FS.as_ref(),
        &fixture.sandbox,
        &fixture.cwd,
    )
    .await
    .expect("grep succeeds");

    assert_eq!(glob_output, "ignored.rs\n.hidden.rs");
    assert_eq!(grep_output, "Found 1 file\n.hidden.txt");
}

#[tokio::test]
async fn claude_code_glob_golden_take_100_and_truncation_message() {
    let fixture = FileGoldenFixture::new();
    fixture.create_dir("many");
    for index in 0..101 {
        fixture.write(&format!("many/file-{index:03}.txt"), "");
    }

    let output = glob_files(
        json!({ "pattern": "many/*.txt" }).to_string(),
        LOCAL_FS.as_ref(),
        &fixture.sandbox,
        &fixture.cwd,
    )
    .await
    .expect("glob succeeds");
    let lines = output.lines().collect::<Vec<_>>();

    assert_eq!(lines.len(), 101);
    assert_eq!(
        lines.last(),
        Some(&"(Results are truncated. Consider using a more specific path or pattern.)")
    );
}

#[tokio::test]
async fn claude_code_grep_golden_long_line_omission_and_relative_path() {
    let fixture = FileGoldenFixture::new();
    fixture.create_dir("src");
    fixture.write("src/lib.rs", &format!("needle{}\n", "x".repeat(600)));

    let output = grep_files(
        json!({
            "pattern": "needle",
            "path": "src",
            "output_mode": "content"
        })
        .to_string(),
        LOCAL_FS.as_ref(),
        &fixture.sandbox,
        &fixture.cwd,
    )
    .await
    .expect("grep succeeds");

    assert_eq!(output, "src/lib.rs:1:[Omitted long matching line]");
}

struct FileGoldenFixture {
    _temp_dir: tempfile::TempDir,
    cwd: AbsolutePathBuf,
    sandbox: FileSystemSandboxContext,
}

impl FileGoldenFixture {
    fn new() -> Self {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let cwd = AbsolutePathBuf::from_absolute_path(temp_dir.path()).expect("absolute temp dir");
        Self {
            _temp_dir: temp_dir,
            cwd,
            sandbox: FileSystemSandboxContext::from_permission_profile(PermissionProfile::Disabled),
        }
    }

    fn create_dir(&self, relative_path: &str) {
        std::fs::create_dir_all(self.cwd.as_path().join(relative_path)).expect("create directory");
    }

    fn write(&self, relative_path: &str, contents: &str) {
        let path = self.cwd.as_path().join(relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent directory");
        }
        std::fs::write(path, contents).expect("write fixture file");
    }
}

fn read_args(file_path: &str) -> ReadArgs {
    ReadArgs {
        file_path: file_path.to_string(),
        offset: None,
        limit: None,
    }
}
