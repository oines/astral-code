use codex_file_system::GlobSearchMatch;
use codex_file_system::GlobSearchRequest;
use codex_file_system::GlobSearchResponse;
use codex_file_system::GrepOutputMode;
use codex_file_system::GrepSearchRequest;
use codex_file_system::GrepSearchResponse;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

#[test]
fn search_types_serde_round_trip() -> anyhow::Result<()> {
    let root = AbsolutePathBuf::from_absolute_path(std::env::current_dir()?.join("src"))?;
    let glob_request = GlobSearchRequest {
        root: root.clone(),
        pattern: "**/*.rs".to_string(),
        max_results: 100,
    };
    let glob_response = GlobSearchResponse {
        matches: vec![GlobSearchMatch {
            path: root.join("lib.rs"),
            modified_at_ms: 123,
        }],
        truncated: true,
    };
    let grep_request = GrepSearchRequest {
        root,
        pattern: "needle".to_string(),
        glob: Some("**/*.rs".to_string()),
        file_type: Some("rust".to_string()),
        output_mode: GrepOutputMode::Content,
        context_before: 1,
        context_after: 2,
        line_numbers: true,
        ignore_case: true,
        head_limit: 250,
        offset: 3,
        multiline: true,
    };
    let grep_response = GrepSearchResponse {
        lines: vec!["lib.rs:4:needle".to_string()],
        num_files: 0,
        num_matches: None,
        applied_limit: None,
        applied_offset: None,
        truncated: false,
    };

    assert_eq!(round_trip(&glob_request)?, glob_request);
    assert_eq!(round_trip(&glob_response)?, glob_response);
    assert_eq!(round_trip(&grep_request)?, grep_request);
    assert_eq!(round_trip(&grep_response)?, grep_response);
    Ok(())
}

fn round_trip<T>(value: &T) -> serde_json::Result<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    serde_json::from_value(serde_json::to_value(value)?)
}
