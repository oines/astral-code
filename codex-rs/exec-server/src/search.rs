use std::io;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_file_system::GlobSearchMatch;
use codex_file_system::GlobSearchRequest;
use codex_file_system::GlobSearchResponse;
use codex_file_system::GrepOutputMode;
use codex_file_system::GrepSearchRequest;
use codex_file_system::GrepSearchResponse;
use globset::GlobBuilder;
use globset::GlobSet;
use globset::GlobSetBuilder;
use ignore::WalkBuilder;
use regex_lite::Regex;
use regex_lite::RegexBuilder;

const SEARCH_PRUNED_DIRECTORY_NAMES: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    ".bzr",
    ".jj",
    ".cache",
    ".next",
    ".turbo",
    "build",
    "dist",
    "node_modules",
    "target",
];

#[derive(Debug)]
struct GlobCandidate {
    path: codex_utils_absolute_path::AbsolutePathBuf,
    relative_slash_path: String,
    modified_at_ms: i64,
}

#[derive(Debug)]
struct GrepCandidate {
    path: PathBuf,
    relative_slash_path: String,
    modified_at_ms: i64,
}

#[derive(Debug)]
struct LimitedLines {
    lines: Vec<String>,
    truncated: bool,
    applied_limit: Option<usize>,
    applied_offset: Option<usize>,
}

pub(crate) async fn glob_search(request: GlobSearchRequest) -> io::Result<GlobSearchResponse> {
    tokio::task::spawn_blocking(move || glob_search_blocking(request))
        .await
        .map_err(|err| io::Error::other(format!("filesystem search task failed: {err}")))?
}

pub(crate) async fn grep_search(request: GrepSearchRequest) -> io::Result<GrepSearchResponse> {
    tokio::task::spawn_blocking(move || grep_search_blocking(request))
        .await
        .map_err(|err| io::Error::other(format!("filesystem search task failed: {err}")))?
}

fn glob_search_blocking(request: GlobSearchRequest) -> io::Result<GlobSearchResponse> {
    let (root, pattern) = glob_root_and_pattern(request.root.as_path(), &request.pattern);
    let matcher = compile_glob_set(&pattern)?;
    let candidates = glob_candidates(root.as_path(), &pattern, &matcher)?;
    let max_results = request.max_results;
    let truncated = candidates.len() > max_results;
    let matches = candidates
        .into_iter()
        .take(max_results)
        .map(|candidate| GlobSearchMatch {
            path: candidate.path,
            modified_at_ms: candidate.modified_at_ms,
        })
        .collect();

    Ok(GlobSearchResponse { matches, truncated })
}

fn glob_candidates(
    root: &Path,
    pattern: &str,
    matcher: &GlobSet,
) -> io::Result<Vec<GlobCandidate>> {
    let prefix = glob_literal_directory_prefix(pattern);
    let Some(start_dir) = resolve_glob_start_dir(root, &prefix)? else {
        return Ok(Vec::new());
    };
    let match_basename = !pattern.contains('/');
    let mut candidates = Vec::new();
    for entry in search_walker(start_dir.as_path()) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let file_type = entry.file_type();
        if !file_type.is_some_and(|file_type| file_type.is_file()) {
            continue;
        }
        let relative_slash_path = relative_slash_path(entry.path(), root);
        if !glob_path_matches(matcher, match_basename, &relative_slash_path, entry.path()) {
            continue;
        }
        candidates.push(GlobCandidate {
            path: codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(entry.path())?,
            modified_at_ms: std::fs::metadata(entry.path())
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .map_or(0, system_time_to_unix_ms),
            relative_slash_path,
        });
    }

    candidates.sort_by(|left, right| {
        left.modified_at_ms
            .cmp(&right.modified_at_ms)
            .then_with(|| left.relative_slash_path.cmp(&right.relative_slash_path))
    });
    Ok(candidates)
}

fn grep_search_blocking(request: GrepSearchRequest) -> io::Result<GrepSearchResponse> {
    let mut regex_builder = RegexBuilder::new(&request.pattern);
    regex_builder
        .case_insensitive(request.ignore_case)
        .dot_matches_new_line(request.multiline);
    let regex = regex_builder.build().map_err(invalid_pattern_error)?;
    let candidates = grep_candidates(&request)?;
    let mut output = Vec::new();
    let mut files_with_matches = Vec::new();

    for candidate in candidates {
        if !type_filter_matches(candidate.path.as_path(), request.file_type.as_deref()) {
            continue;
        }

        let bytes = match std::fs::read(candidate.path.as_path()) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        if bytes.contains(&0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        let lines = split_lines_preserving_newline(&text);
        let matched_lines = matching_line_indexes(&regex, &lines, request.multiline, &text);
        if matched_lines.is_empty() {
            continue;
        }

        match request.output_mode {
            GrepOutputMode::FilesWithMatches => {
                files_with_matches.push((candidate.relative_slash_path, candidate.modified_at_ms));
            }
            GrepOutputMode::Count => {
                output.push(format!(
                    "{}:{}",
                    candidate.relative_slash_path,
                    matched_lines.len()
                ));
            }
            GrepOutputMode::Content => push_content_matches(
                &mut output,
                &candidate.relative_slash_path,
                &lines,
                &matched_lines,
                request.line_numbers,
                request.context_before,
                request.context_after,
            ),
        }
    }

    if request.output_mode == GrepOutputMode::FilesWithMatches {
        files_with_matches
            .sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        output = files_with_matches
            .into_iter()
            .map(|(path, _modified_at_ms)| path)
            .collect();
    }

    let LimitedLines {
        lines,
        truncated,
        applied_limit,
        applied_offset,
    } = apply_head_limit(output, request.head_limit, request.offset);
    let (num_files, num_matches) = grep_result_counts(request.output_mode, &lines);
    Ok(GrepSearchResponse {
        lines,
        num_files,
        num_matches,
        applied_limit,
        applied_offset,
        truncated,
    })
}

fn apply_head_limit(
    lines: Vec<String>,
    head_limit: usize,
    requested_offset: usize,
) -> LimitedLines {
    let offset = requested_offset.min(lines.len());
    let applied_offset = (requested_offset > 0).then_some(requested_offset);
    if head_limit == 0 {
        return LimitedLines {
            lines: lines.into_iter().skip(offset).collect(),
            truncated: false,
            applied_limit: None,
            applied_offset,
        };
    }

    let truncated = lines.len().saturating_sub(offset) > head_limit;
    LimitedLines {
        lines: lines.into_iter().skip(offset).take(head_limit).collect(),
        truncated,
        applied_limit: truncated.then_some(head_limit),
        applied_offset,
    }
}

fn grep_result_counts(output_mode: GrepOutputMode, lines: &[String]) -> (usize, Option<usize>) {
    match output_mode {
        GrepOutputMode::FilesWithMatches => (lines.len(), None),
        GrepOutputMode::Content => (0, None),
        GrepOutputMode::Count => {
            let num_matches = lines
                .iter()
                .filter_map(|line| line.rsplit_once(':'))
                .filter_map(|(_path, count)| count.parse::<usize>().ok())
                .sum();
            (lines.len(), Some(num_matches))
        }
    }
}

fn grep_candidates(request: &GrepSearchRequest) -> io::Result<Vec<GrepCandidate>> {
    let metadata = std::fs::metadata(request.root.as_path())?;
    if metadata.is_file() {
        return Ok(vec![GrepCandidate {
            path: request.root.to_path_buf(),
            relative_slash_path: String::new(),
            modified_at_ms: metadata.modified().ok().map_or(0, system_time_to_unix_ms),
        }]);
    }
    if !metadata.is_dir() {
        return Ok(Vec::new());
    }

    let glob = request.glob.as_deref().map(normalize_pattern);
    let matcher = glob.as_deref().map(compile_glob_set).transpose()?;
    let match_basename = glob.as_deref().is_some_and(|glob| !glob.contains('/'));
    let prefix = glob
        .as_deref()
        .map(glob_literal_directory_prefix)
        .unwrap_or_default();
    let Some(start_dir) = resolve_glob_start_dir(request.root.as_path(), &prefix)? else {
        return Ok(Vec::new());
    };

    let mut candidates = Vec::new();
    for entry in search_walker(start_dir.as_path()) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let file_type = entry.file_type();
        if !file_type.is_some_and(|file_type| file_type.is_file()) {
            continue;
        }
        let relative_slash_path = relative_slash_path(entry.path(), request.root.as_path());
        if let Some(matcher) = &matcher
            && !glob_path_matches(matcher, match_basename, &relative_slash_path, entry.path())
        {
            continue;
        }
        candidates.push(GrepCandidate {
            path: entry.path().to_path_buf(),
            relative_slash_path,
            modified_at_ms: std::fs::metadata(entry.path())
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .map_or(0, system_time_to_unix_ms),
        });
    }
    Ok(candidates)
}

fn search_walker(root: &Path) -> ignore::Walk {
    let mut builder = WalkBuilder::new(root);
    builder
        .follow_links(false)
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .filter_entry(|entry| entry.depth() == 0 || !is_pruned_directory(entry.path()));
    builder.build()
}

fn is_pruned_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| SEARCH_PRUNED_DIRECTORY_NAMES.contains(&name))
}

fn glob_root_and_pattern(root: &Path, pattern: &str) -> (PathBuf, String) {
    if Path::new(pattern).is_absolute()
        && let Some((base_dir, relative_pattern)) = absolute_glob_base(pattern)
    {
        return (base_dir, normalize_pattern(&relative_pattern));
    }

    (root.to_path_buf(), normalize_pattern(pattern))
}

fn absolute_glob_base(pattern: &str) -> Option<(PathBuf, String)> {
    let glob_index = pattern.find(['*', '?', '[', '{']).unwrap_or(pattern.len());
    let static_prefix = &pattern[..glob_index];
    let last_separator = static_prefix
        .rfind('/')
        .into_iter()
        .chain(static_prefix.rfind('\\'))
        .max()?;
    let base_end = if last_separator == 0
        || static_prefix
            .as_bytes()
            .get(last_separator.saturating_sub(1))
            .is_some_and(|byte| *byte == b':')
    {
        last_separator + 1
    } else {
        last_separator
    };
    let base_dir = PathBuf::from(&pattern[..base_end]);
    let relative_pattern = pattern[last_separator + 1..].to_string();
    Some((base_dir, relative_pattern))
}

fn normalize_pattern(pattern: &str) -> String {
    pattern
        .strip_prefix("./")
        .unwrap_or(pattern)
        .replace('\\', "/")
}

fn compile_glob_set(pattern: &str) -> io::Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in expanded_glob_patterns(pattern) {
        let glob = GlobBuilder::new(&pattern)
            .literal_separator(true)
            .build()
            .map_err(invalid_pattern_error)?;
        builder.add(glob);
    }
    builder.build().map_err(invalid_pattern_error)
}

fn expanded_glob_patterns(pattern: &str) -> Vec<String> {
    let mut patterns = vec![pattern.to_string()];
    let mut index = 0;
    while let Some(relative_pos) = pattern[index..].find("**/") {
        let pos = index + relative_pos;
        let mut alternative = String::new();
        alternative.push_str(&pattern[..pos]);
        alternative.push_str(&pattern[pos + 3..]);
        patterns.push(alternative);
        index = pos + 3;
    }
    patterns.sort();
    patterns.dedup();
    patterns
}

fn glob_literal_directory_prefix(pattern: &str) -> Vec<&str> {
    let segments = pattern.split('/').collect::<Vec<_>>();
    let mut prefix = Vec::new();
    for segment in segments.iter().take(segments.len().saturating_sub(1)) {
        if *segment == "**" || has_glob_meta(segment) {
            break;
        }
        prefix.push(*segment);
    }
    prefix
}

fn has_glob_meta(segment: &str) -> bool {
    segment.contains('*') || segment.contains('?') || segment.contains('[') || segment.contains('{')
}

fn resolve_glob_start_dir(root: &Path, prefix: &[&str]) -> io::Result<Option<PathBuf>> {
    let mut dir = root.to_path_buf();
    for segment in prefix {
        dir.push(segment);
    }
    match std::fs::metadata(dir.as_path()) {
        Ok(metadata) if metadata.is_dir() => Ok(Some(dir)),
        Ok(_) => Ok(None),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn glob_path_matches(
    matcher: &GlobSet,
    match_basename: bool,
    relative_slash_path: &str,
    path: &Path,
) -> bool {
    if !match_basename {
        return matcher.is_match(relative_slash_path);
    }

    path.file_name()
        .and_then(|file_name| file_name.to_str())
        .is_some_and(|file_name| matcher.is_match(file_name))
}

fn relative_slash_path(path: &Path, root: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .filter_map(normal_component_text)
        .collect::<Vec<_>>()
        .join("/")
}

fn normal_component_text(component: Component<'_>) -> Option<String> {
    match component {
        Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
        Component::CurDir | Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
            None
        }
    }
}

fn type_filter_matches(path: &Path, file_type: Option<&str>) -> bool {
    let Some(file_type) = file_type else {
        return true;
    };
    let extension = match file_type {
        "rust" => "rs",
        "python" | "py" => "py",
        "javascript" | "js" => "js",
        "typescript" | "ts" => "ts",
        "tsx" => "tsx",
        "go" => "go",
        "java" => "java",
        "markdown" | "md" => "md",
        "json" => "json",
        other => other,
    };
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

fn split_lines_preserving_newline(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split_inclusive('\n').collect()
}

fn matching_line_indexes(
    regex: &Regex,
    lines: &[&str],
    multiline: bool,
    full_text: &str,
) -> Vec<usize> {
    if multiline && regex.is_match(full_text) && !lines.is_empty() {
        return (0..lines.len()).collect();
    }

    lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| regex.is_match(line).then_some(index))
        .collect()
}

fn push_content_matches(
    output: &mut Vec<String>,
    display: &str,
    lines: &[&str],
    matched_lines: &[usize],
    line_numbers: bool,
    context_before: usize,
    context_after: usize,
) {
    let mut last_pushed = None;
    for line_index in matched_lines {
        let start = line_index.saturating_sub(context_before);
        let end = (line_index + context_after + 1).min(lines.len());
        for (index, line) in lines.iter().enumerate().take(end).skip(start) {
            if last_pushed.is_some_and(|last| index <= last) {
                continue;
            }
            last_pushed = Some(index);
            let line = line.trim_end_matches(['\r', '\n']);
            if line_numbers {
                output.push(format!("{display}:{}:{line}", index + 1));
            } else {
                output.push(format!("{display}:{line}"));
            }
        }
    }
}

fn invalid_pattern_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error.to_string())
}

fn system_time_to_unix_ms(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
