use std::time::Duration;

use codex_file_system::GlobSearchRequest;
use codex_file_system::GrepOutputMode;
use codex_file_system::GrepSearchRequest;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

use super::*;

#[tokio::test]
async fn glob_without_slash_recurses_and_sorts_by_oldest_mtime() -> io::Result<()> {
    let temp_dir = tempfile::TempDir::new()?;
    let root = AbsolutePathBuf::from_absolute_path(temp_dir.path())?;
    std::fs::create_dir_all(temp_dir.path().join("nested"))?;
    std::fs::write(temp_dir.path().join("old.toml"), "")?;
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(temp_dir.path().join("new.toml"), "")?;
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(temp_dir.path().join("nested/child.toml"), "")?;

    let response = glob_search(GlobSearchRequest {
        root: root.clone(),
        pattern: "*.toml".to_string(),
        max_results: 10,
    })
    .await?;

    let paths = response
        .matches
        .iter()
        .map(|matched| relative_slash_path(matched.path.as_path(), root.as_path()))
        .collect::<Vec<_>>();
    assert_eq!(paths, vec!["old.toml", "new.toml", "nested/child.toml"]);
    assert!(!response.truncated);
    Ok(())
}

#[tokio::test]
async fn glob_with_slash_matches_relative_path() -> io::Result<()> {
    let temp_dir = tempfile::TempDir::new()?;
    let root = AbsolutePathBuf::from_absolute_path(temp_dir.path())?;
    std::fs::create_dir_all(temp_dir.path().join("src/nested"))?;
    std::fs::write(temp_dir.path().join("src/lib.rs"), "")?;
    std::fs::write(temp_dir.path().join("src/nested/mod.rs"), "")?;

    let response = glob_search(GlobSearchRequest {
        root: root.clone(),
        pattern: "src/*.rs".to_string(),
        max_results: 10,
    })
    .await?;

    let paths = response
        .matches
        .iter()
        .map(|matched| relative_slash_path(matched.path.as_path(), root.as_path()))
        .collect::<Vec<_>>();
    assert_eq!(paths, vec!["src/lib.rs"]);
    Ok(())
}

#[tokio::test]
async fn glob_absolute_pattern_extracts_search_root() -> io::Result<()> {
    let temp_dir = tempfile::TempDir::new()?;
    let root = AbsolutePathBuf::from_absolute_path(temp_dir.path())?;
    std::fs::create_dir_all(temp_dir.path().join("src"))?;
    std::fs::write(temp_dir.path().join("src/lib.rs"), "")?;
    std::fs::write(temp_dir.path().join("other.rs"), "")?;
    let absolute_pattern = temp_dir
        .path()
        .join("src")
        .join("*.rs")
        .to_string_lossy()
        .into_owned();

    let response = glob_search(GlobSearchRequest {
        root: root.clone(),
        pattern: absolute_pattern,
        max_results: 10,
    })
    .await?;

    let paths = response
        .matches
        .iter()
        .map(|matched| relative_slash_path(matched.path.as_path(), root.as_path()))
        .collect::<Vec<_>>();
    assert_eq!(paths, vec!["src/lib.rs"]);
    Ok(())
}

#[tokio::test]
async fn glob_double_star_recurses_under_literal_prefix() -> io::Result<()> {
    let temp_dir = tempfile::TempDir::new()?;
    let root = AbsolutePathBuf::from_absolute_path(temp_dir.path())?;
    std::fs::create_dir_all(temp_dir.path().join("src/nested"))?;
    std::fs::create_dir_all(temp_dir.path().join("tests/nested"))?;
    std::fs::write(temp_dir.path().join("src/lib.rs"), "")?;
    std::fs::write(temp_dir.path().join("src/nested/mod.rs"), "")?;
    std::fs::write(temp_dir.path().join("tests/nested/mod.rs"), "")?;

    let response = glob_search(GlobSearchRequest {
        root: root.clone(),
        pattern: "src/**/*.rs".to_string(),
        max_results: 10,
    })
    .await?;

    let mut paths = response
        .matches
        .iter()
        .map(|matched| relative_slash_path(matched.path.as_path(), root.as_path()))
        .collect::<Vec<_>>();
    paths.sort();
    assert_eq!(paths, vec!["src/lib.rs", "src/nested/mod.rs"]);
    Ok(())
}

#[tokio::test]
async fn grep_excludes_vcs_directories_but_not_generated_directories() -> io::Result<()> {
    let temp_dir = tempfile::TempDir::new()?;
    let root = AbsolutePathBuf::from_absolute_path(temp_dir.path())?;
    std::fs::create_dir_all(temp_dir.path().join("src"))?;
    std::fs::create_dir_all(temp_dir.path().join(".git"))?;
    std::fs::create_dir_all(temp_dir.path().join("target/debug"))?;
    std::fs::write(temp_dir.path().join("src/lib.rs"), "needle\n")?;
    std::fs::write(temp_dir.path().join(".git/config"), "needle\n")?;
    std::fs::write(
        temp_dir.path().join("target/debug/generated.rs"),
        "needle\n",
    )?;

    let response = grep_search(GrepSearchRequest {
        root,
        pattern: "needle".to_string(),
        glob: None,
        file_type: None,
        output_mode: GrepOutputMode::FilesWithMatches,
        context_before: 0,
        context_after: 0,
        line_numbers: false,
        ignore_case: false,
        head_limit: 10,
        offset: 0,
        multiline: false,
    })
    .await?;

    let mut lines = response.lines;
    lines.sort();
    assert_eq!(lines, vec!["src/lib.rs", "target/debug/generated.rs"]);
    Ok(())
}

#[tokio::test]
async fn grep_files_with_matches_sorts_by_newest_mtime() -> io::Result<()> {
    let temp_dir = tempfile::TempDir::new()?;
    let root = AbsolutePathBuf::from_absolute_path(temp_dir.path())?;
    std::fs::write(temp_dir.path().join("old.txt"), "needle\n")?;
    std::thread::sleep(Duration::from_secs(1));
    std::fs::write(temp_dir.path().join("middle.txt"), "needle\n")?;
    std::thread::sleep(Duration::from_secs(1));
    std::fs::write(temp_dir.path().join("new.txt"), "needle\n")?;

    let response = grep_search(GrepSearchRequest {
        root,
        pattern: "needle".to_string(),
        glob: Some("*.txt".to_string()),
        file_type: None,
        output_mode: GrepOutputMode::FilesWithMatches,
        context_before: 0,
        context_after: 0,
        line_numbers: false,
        ignore_case: false,
        head_limit: 10,
        offset: 0,
        multiline: false,
    })
    .await?;

    assert_eq!(response.lines, vec!["new.txt", "middle.txt", "old.txt"]);
    Ok(())
}

#[tokio::test]
async fn grep_supports_count_content_context_and_glob_filters() -> io::Result<()> {
    let temp_dir = tempfile::TempDir::new()?;
    let root = AbsolutePathBuf::from_absolute_path(temp_dir.path())?;
    std::fs::create_dir_all(temp_dir.path().join("nested"))?;
    std::fs::write(temp_dir.path().join("root.md"), "alpha\nneedle\nomega\n")?;
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(temp_dir.path().join("nested/child.md"), "needle\n")?;

    let files = grep_search(GrepSearchRequest {
        root: root.clone(),
        pattern: "needle".to_string(),
        glob: Some("*.md".to_string()),
        file_type: Some("markdown".to_string()),
        output_mode: GrepOutputMode::FilesWithMatches,
        context_before: 0,
        context_after: 0,
        line_numbers: false,
        ignore_case: false,
        head_limit: 10,
        offset: 0,
        multiline: false,
    })
    .await?;
    assert_eq!(files.lines, vec!["nested/child.md", "root.md"]);
    assert_eq!(files.num_files, 2);
    assert_eq!(files.applied_limit, None);

    let count = grep_search(GrepSearchRequest {
        output_mode: GrepOutputMode::Count,
        glob: Some("root.md".to_string()),
        ..grep_request_defaults(root.clone())
    })
    .await?;
    assert_eq!(count.lines, vec!["root.md:1"]);
    assert_eq!(count.num_files, 1);
    assert_eq!(count.num_matches, Some(1));

    let content = grep_search(GrepSearchRequest {
        output_mode: GrepOutputMode::Content,
        context_before: 1,
        context_after: 1,
        line_numbers: true,
        glob: Some("root.md".to_string()),
        ..grep_request_defaults(root)
    })
    .await?;
    assert_eq!(
        content.lines,
        vec!["root.md-1-alpha", "root.md:2:needle", "root.md-3-omega"]
    );
    Ok(())
}

#[tokio::test]
async fn grep_multiline_matches_all_output_modes() -> io::Result<()> {
    let temp_dir = tempfile::TempDir::new()?;
    let root = AbsolutePathBuf::from_absolute_path(temp_dir.path())?;
    std::fs::write(temp_dir.path().join("sample.txt"), "alpha\nbeta\ngamma\n")?;

    let files = grep_search(GrepSearchRequest {
        pattern: "alpha\nbeta".to_string(),
        multiline: true,
        output_mode: GrepOutputMode::FilesWithMatches,
        ..grep_request_defaults(root.clone())
    })
    .await?;
    assert_eq!(files.lines, vec!["sample.txt"]);

    let count = grep_search(GrepSearchRequest {
        pattern: "alpha\nbeta".to_string(),
        multiline: true,
        output_mode: GrepOutputMode::Count,
        ..grep_request_defaults(root.clone())
    })
    .await?;
    assert_eq!(count.lines, vec!["sample.txt:1"]);

    let content = grep_search(GrepSearchRequest {
        pattern: "alpha\nbeta".to_string(),
        multiline: true,
        output_mode: GrepOutputMode::Content,
        line_numbers: true,
        ..grep_request_defaults(root)
    })
    .await?;
    assert_eq!(
        content.lines,
        vec!["sample.txt:1:alpha", "sample.txt:2:beta"]
    );
    Ok(())
}

#[tokio::test]
async fn glob_prunes_to_literal_prefix() -> io::Result<()> {
    let temp_dir = tempfile::TempDir::new()?;
    let root = AbsolutePathBuf::from_absolute_path(temp_dir.path())?;
    std::fs::create_dir_all(temp_dir.path().join("src/nested"))?;
    std::fs::create_dir_all(temp_dir.path().join("wide/nested"))?;
    std::fs::write(temp_dir.path().join("src/lib.rs"), "")?;
    std::fs::write(temp_dir.path().join("src/nested/mod.rs"), "")?;
    std::fs::write(temp_dir.path().join("wide/nested/mod.rs"), "")?;

    let response = glob_search(GlobSearchRequest {
        root: root.clone(),
        pattern: "src/**/*.rs".to_string(),
        max_results: 10,
    })
    .await?;

    let mut paths = response
        .matches
        .iter()
        .map(|matched| relative_slash_path(matched.path.as_path(), root.as_path()))
        .collect::<Vec<_>>();
    paths.sort();
    assert_eq!(paths, vec!["src/lib.rs", "src/nested/mod.rs"]);
    Ok(())
}

#[tokio::test]
async fn grep_head_limit_zero_is_unlimited() -> io::Result<()> {
    let temp_dir = tempfile::TempDir::new()?;
    let root = AbsolutePathBuf::from_absolute_path(temp_dir.path())?;
    std::fs::write(temp_dir.path().join("a.txt"), "needle\n")?;
    std::fs::write(temp_dir.path().join("b.txt"), "needle\n")?;
    std::fs::write(temp_dir.path().join("c.txt"), "needle\n")?;

    let response = grep_search(GrepSearchRequest {
        head_limit: 0,
        ..grep_request_defaults(root)
    })
    .await?;

    assert_eq!(response.lines.len(), 3);
    assert_eq!(response.num_files, 3);
    assert_eq!(response.applied_limit, None);
    assert!(!response.truncated);
    Ok(())
}

fn grep_request_defaults(root: AbsolutePathBuf) -> GrepSearchRequest {
    GrepSearchRequest {
        root,
        pattern: "needle".to_string(),
        glob: None,
        file_type: None,
        output_mode: GrepOutputMode::FilesWithMatches,
        context_before: 0,
        context_after: 0,
        line_numbers: false,
        ignore_case: false,
        head_limit: 10,
        offset: 0,
        multiline: false,
    }
}
