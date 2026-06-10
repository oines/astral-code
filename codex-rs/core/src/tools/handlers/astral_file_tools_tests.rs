use pretty_assertions::assert_eq;
use serde_json::json;

use codex_exec_server::FileSystemSandboxContext;
use codex_exec_server::LOCAL_FS;
use codex_protocol::models::PermissionProfile;
use core_test_support::PathExt;

use super::GrepArgs;
use super::add_line_numbers;
use super::edit_file;
use super::is_blocked_device_path;
use super::push_content_matches;
use super::split_lines_preserving_newline;

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
fn grep_content_output_can_include_line_numbers() {
    let text = "alpha\nneedle\nomega\n";
    let lines = split_lines_preserving_newline(text);
    let mut output = Vec::new();

    push_content_matches(
        &mut output,
        "src/lib.rs",
        &lines,
        &[1],
        /*line_numbers*/ true,
        /*context_before*/ 0,
        /*context_after*/ 0,
    );

    assert_eq!(output, vec!["src/lib.rs:2:needle"]);
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
