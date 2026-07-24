use std::io;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_file_system::GlobSearchMatch;
use codex_file_system::GlobSearchRequest;
use codex_file_system::GlobSearchResponse;
use codex_file_system::GrepOutputMode;
use codex_file_system::GrepSearchRequest;
use codex_file_system::GrepSearchResponse;
use codex_utils_path_uri::PathUri;
use grep_regex::RegexMatcher;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::BinaryDetection;
use grep_searcher::Searcher;
use grep_searcher::SearcherBuilder;
use grep_searcher::Sink;
use grep_searcher::SinkContext;
use grep_searcher::SinkMatch;
use ignore::DirEntry;
use ignore::WalkBuilder;
use ignore::WalkState;
use ignore::overrides::Override;
use ignore::overrides::OverrideBuilder;
use ignore::types::Types;
use ignore::types::TypesBuilder;

const GREP_EXCLUDED_DIRECTORY_GLOBS: &[&str] = &["!.git", "!.svn", "!.hg", "!.bzr", "!.jj", "!.sl"];
const MAX_COLUMNS: usize = 500;
const MAX_GLOB_SCAN_ENTRIES: usize = 100_000;
const MAX_GLOB_SCAN_DURATION: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
struct SearchCandidate {
    path: PathBuf,
    relative_slash_path: String,
}

#[derive(Debug)]
struct GlobCandidate {
    path: PathUri,
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

#[derive(Clone, Copy, Debug)]
struct WalkConfig {
    include_hidden: bool,
    respect_ignore_files: bool,
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
    let native_root = request.root.to_abs_path()?;
    let (root, pattern) = glob_root_and_pattern(native_root.as_path(), &request.pattern);
    let override_matcher = build_overrides(root.as_path(), &[pattern])?;
    let candidates = glob_candidates(root.as_path(), &override_matcher)?;
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

fn glob_candidates(root: &Path, override_matcher: &Override) -> io::Result<Vec<GlobCandidate>> {
    let metadata = match std::fs::metadata(root) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    if metadata.is_file() {
        if !path_matches_overrides(override_matcher, root, false) {
            return Ok(Vec::new());
        }
        let path = codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(root)?;
        return Ok(vec![GlobCandidate {
            path: PathUri::from_abs_path(&path),
            relative_slash_path: root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            modified_at_ms: metadata.modified().ok().map_or(0, system_time_to_unix_ms),
        }]);
    }
    if !metadata.is_dir() {
        return Ok(Vec::new());
    }

    let candidates = Arc::new(Mutex::new(Vec::new()));
    let scanned_entries = Arc::new(AtomicUsize::new(0));
    let entry_limit_exceeded = Arc::new(AtomicBool::new(false));
    let time_limit_exceeded = Arc::new(AtomicBool::new(false));
    let started_at = Instant::now();
    let mut builder = walk_builder(root, glob_walk_config());
    builder.overrides(override_matcher.clone());
    builder.build_parallel().run(|| {
        let candidates = Arc::clone(&candidates);
        let scanned_entries = Arc::clone(&scanned_entries);
        let entry_limit_exceeded = Arc::clone(&entry_limit_exceeded);
        let time_limit_exceeded = Arc::clone(&time_limit_exceeded);
        Box::new(move |result| {
            let Ok(entry) = result else {
                return WalkState::Continue;
            };
            let scanned = scanned_entries.fetch_add(1, Ordering::Relaxed) + 1;
            if scanned > MAX_GLOB_SCAN_ENTRIES {
                entry_limit_exceeded.store(true, Ordering::Relaxed);
                return WalkState::Quit;
            }
            if started_at.elapsed() > MAX_GLOB_SCAN_DURATION {
                time_limit_exceeded.store(true, Ordering::Relaxed);
                return WalkState::Quit;
            }
            if !entry_is_file(&entry) {
                return WalkState::Continue;
            }
            let relative_slash_path = relative_slash_path(entry.path(), root);
            let Ok(path) =
                codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(entry.path())
            else {
                return WalkState::Continue;
            };
            let modified_at_ms = entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .map_or(0, system_time_to_unix_ms);
            let Ok(mut candidates) = candidates.lock() else {
                return WalkState::Quit;
            };
            candidates.push(GlobCandidate {
                path: PathUri::from_abs_path(&path),
                relative_slash_path,
                modified_at_ms,
            });
            WalkState::Continue
        })
    });

    if entry_limit_exceeded.load(Ordering::Relaxed) || time_limit_exceeded.load(Ordering::Relaxed) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Glob search scanned too many paths. Use a more specific path or pattern.",
        ));
    }

    let mut candidates = into_inner_vec(candidates, "glob candidates lock")?;
    candidates.sort_by(|left, right| {
        left.modified_at_ms
            .cmp(&right.modified_at_ms)
            .then_with(|| left.relative_slash_path.cmp(&right.relative_slash_path))
    });
    Ok(candidates)
}

fn grep_search_blocking(request: GrepSearchRequest) -> io::Result<GrepSearchResponse> {
    let matcher = grep_matcher(&request)?;
    let overrides = grep_overrides(&request)?;
    let type_matcher = grep_type_matcher(request.file_type.as_deref())?;
    let output = match request.output_mode {
        GrepOutputMode::FilesWithMatches => {
            grep_files_with_matches(&request, &matcher, &overrides, type_matcher.as_ref())?
        }
        GrepOutputMode::Count | GrepOutputMode::Content => {
            grep_count_or_content(&request, &matcher, &overrides, type_matcher.as_ref())?
        }
    };

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

fn grep_matcher(request: &GrepSearchRequest) -> io::Result<RegexMatcher> {
    let mut builder = RegexMatcherBuilder::new();
    builder
        .case_insensitive(request.ignore_case)
        .multi_line(request.multiline)
        .dot_matches_new_line(request.multiline);
    builder
        .build(&request.pattern)
        .map_err(invalid_pattern_error)
}

fn grep_overrides(request: &GrepSearchRequest) -> io::Result<Override> {
    let mut patterns = GREP_EXCLUDED_DIRECTORY_GLOBS
        .iter()
        .map(|pattern| (*pattern).to_string())
        .collect::<Vec<_>>();
    if let Some(glob) = request.glob.as_deref() {
        patterns.extend(split_grep_globs(glob).into_iter().map(|pattern| {
            if let Some(exclusion) = pattern.strip_prefix('!') {
                format!("!{}", normalize_pattern(exclusion))
            } else {
                normalize_pattern(&pattern)
            }
        }));
    }
    let root = request.root.to_abs_path()?;
    build_overrides(root.as_path(), &patterns)
}

fn grep_type_matcher(file_type: Option<&str>) -> io::Result<Option<Types>> {
    let Some(file_type) = file_type else {
        return Ok(None);
    };
    let mut builder = TypesBuilder::new();
    builder.add_defaults();
    builder.select(file_type);
    builder.build().map(Some).map_err(invalid_pattern_error)
}

fn grep_files_with_matches(
    request: &GrepSearchRequest,
    matcher: &RegexMatcher,
    overrides: &Override,
    type_matcher: Option<&Types>,
) -> io::Result<Vec<String>> {
    let root = request.root.to_abs_path()?;
    let metadata = std::fs::metadata(root.as_path())?;
    if metadata.is_file() {
        let mut searcher = files_with_matches_searcher(request.multiline);
        if !path_has_match(&mut searcher, matcher, root.as_path()) {
            return Ok(Vec::new());
        }
        return Ok(vec![String::new()]);
    }
    if !metadata.is_dir() {
        return Ok(Vec::new());
    }

    let matches = Arc::new(Mutex::new(Vec::new()));
    let builder = grep_walk_builder(root.as_path(), overrides, type_matcher);
    builder.build_parallel().run(|| {
        let matches = Arc::clone(&matches);
        let root = root.clone();
        let mut searcher = files_with_matches_searcher(request.multiline);
        Box::new(move |result| {
            let Ok(entry) = result else {
                return WalkState::Continue;
            };
            if !entry_is_file(&entry) {
                return WalkState::Continue;
            }
            if !path_has_match(&mut searcher, matcher, entry.path()) {
                return WalkState::Continue;
            }
            let relative_slash_path = relative_slash_path(entry.path(), root.as_path());
            let modified_at_ms = entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .map_or(0, system_time_to_unix_ms);
            let Ok(mut matches) = matches.lock() else {
                return WalkState::Quit;
            };
            matches.push((relative_slash_path, modified_at_ms));
            WalkState::Continue
        })
    });

    let mut matches = into_inner_vec(matches, "grep matches lock")?;
    matches.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    Ok(matches
        .into_iter()
        .map(|(relative_slash_path, _modified_at_ms)| relative_slash_path)
        .collect())
}

fn grep_count_or_content(
    request: &GrepSearchRequest,
    matcher: &RegexMatcher,
    overrides: &Override,
    type_matcher: Option<&Types>,
) -> io::Result<Vec<String>> {
    let root = request.root.to_abs_path()?;
    let candidates = grep_candidates(root.as_path(), overrides, type_matcher)?;
    let mut output = Vec::new();
    let mut searcher = count_or_content_searcher(request);
    for candidate in candidates {
        match request.output_mode {
            GrepOutputMode::Count => {
                let mut sink = CountSink::default();
                if searcher
                    .search_path(matcher, candidate.path.as_path(), &mut sink)
                    .is_err()
                    || sink.count == 0
                {
                    continue;
                }
                output.push(format!("{}:{}", candidate.relative_slash_path, sink.count));
            }
            GrepOutputMode::Content => {
                let mut sink =
                    ContentSink::new(candidate.relative_slash_path.clone(), request.line_numbers);
                if searcher
                    .search_path(matcher, candidate.path.as_path(), &mut sink)
                    .is_err()
                    || sink.lines.is_empty()
                {
                    continue;
                }
                output.extend(sink.lines);
            }
            GrepOutputMode::FilesWithMatches => {}
        }
    }
    Ok(output)
}

fn grep_candidates(
    root: &Path,
    overrides: &Override,
    type_matcher: Option<&Types>,
) -> io::Result<Vec<SearchCandidate>> {
    let metadata = std::fs::metadata(root)?;
    if metadata.is_file() {
        return Ok(vec![SearchCandidate {
            path: root.to_path_buf(),
            relative_slash_path: String::new(),
        }]);
    }
    if !metadata.is_dir() {
        return Ok(Vec::new());
    }

    let mut candidates = Vec::new();
    let builder = grep_walk_builder(root, overrides, type_matcher);
    for entry in builder.build() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !entry_is_file(&entry) {
            continue;
        }
        candidates.push(SearchCandidate {
            path: entry.path().to_path_buf(),
            relative_slash_path: relative_slash_path(entry.path(), root),
        });
    }
    Ok(candidates)
}

fn files_with_matches_searcher(multiline: bool) -> Searcher {
    let mut builder = SearcherBuilder::new();
    builder
        .binary_detection(BinaryDetection::quit(0))
        .multi_line(multiline)
        .line_number(false)
        .max_matches(Some(1));
    builder.build()
}

fn count_or_content_searcher(request: &GrepSearchRequest) -> Searcher {
    let mut builder = SearcherBuilder::new();
    builder
        .binary_detection(BinaryDetection::quit(0))
        .multi_line(request.multiline)
        .line_number(request.output_mode == GrepOutputMode::Content && request.line_numbers);
    if request.output_mode == GrepOutputMode::Content {
        builder
            .before_context(request.context_before)
            .after_context(request.context_after);
    }
    builder.build()
}

fn path_has_match(searcher: &mut Searcher, matcher: &RegexMatcher, path: &Path) -> bool {
    let mut sink = HasMatchSink::default();
    searcher.search_path(matcher, path, &mut sink).is_ok() && sink.matched
}

#[derive(Default)]
struct HasMatchSink {
    matched: bool,
}

impl Sink for HasMatchSink {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, _mat: &SinkMatch<'_>) -> io::Result<bool> {
        self.matched = true;
        Ok(false)
    }
}

#[derive(Default)]
struct CountSink {
    count: usize,
}

impl Sink for CountSink {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, _mat: &SinkMatch<'_>) -> io::Result<bool> {
        self.count += 1;
        Ok(true)
    }
}

struct ContentSink {
    path: String,
    line_numbers: bool,
    lines: Vec<String>,
}

impl ContentSink {
    fn new(path: String, line_numbers: bool) -> Self {
        Self {
            path,
            line_numbers,
            lines: Vec::new(),
        }
    }

    fn push_match(&mut self, mat: &SinkMatch<'_>) {
        let mut line_number = mat.line_number();
        for line in mat.lines() {
            self.push_line(':', line_number, line, "matching");
            line_number = line_number.map(|line_number| line_number + 1);
        }
    }

    fn push_context(&mut self, context: &SinkContext<'_>) {
        self.push_line('-', context.line_number(), context.bytes(), "context");
    }

    fn push_line(&mut self, separator: char, line_number: Option<u64>, line: &[u8], kind: &str) {
        let line = format_line_bytes(line, kind);
        if self.line_numbers
            && let Some(line_number) = line_number
        {
            self.lines.push(format!(
                "{}{separator}{line_number}{separator}{line}",
                self.path
            ));
            return;
        }
        self.lines.push(format!("{}{separator}{line}", self.path));
    }
}

impl Sink for ContentSink {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> io::Result<bool> {
        self.push_match(mat);
        Ok(true)
    }

    fn context(&mut self, _searcher: &Searcher, context: &SinkContext<'_>) -> io::Result<bool> {
        self.push_context(context);
        Ok(true)
    }

    fn context_break(&mut self, _searcher: &Searcher) -> io::Result<bool> {
        self.lines.push("--".to_string());
        Ok(true)
    }
}

fn format_line_bytes(line: &[u8], kind: &str) -> String {
    let line = trim_line_terminator(line);
    if line.len() > MAX_COLUMNS {
        return format!("[Omitted long {kind} line]");
    }
    String::from_utf8_lossy(line).into_owned()
}

fn trim_line_terminator(mut line: &[u8]) -> &[u8] {
    if let Some(stripped) = line.strip_suffix(b"\n") {
        line = stripped;
    }
    if let Some(stripped) = line.strip_suffix(b"\r") {
        line = stripped;
    }
    line
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

fn into_inner_vec<T>(items: Arc<Mutex<Vec<T>>>, lock_name: &'static str) -> io::Result<Vec<T>> {
    match Arc::try_unwrap(items) {
        Ok(items) => items
            .into_inner()
            .map_err(|_err| io::Error::other(format!("{lock_name} poisoned"))),
        Err(items) => {
            let mut items = items
                .lock()
                .map_err(|_err| io::Error::other(format!("{lock_name} poisoned")))?;
            Ok(std::mem::take(&mut *items))
        }
    }
}

fn glob_walk_config() -> WalkConfig {
    WalkConfig {
        include_hidden: env_truthy_or_default("CLAUDE_CODE_GLOB_HIDDEN", true),
        respect_ignore_files: !env_truthy_or_default("CLAUDE_CODE_GLOB_NO_IGNORE", true),
    }
}

fn grep_walk_config() -> WalkConfig {
    WalkConfig {
        include_hidden: true,
        respect_ignore_files: true,
    }
}

fn walk_builder(root: &Path, config: WalkConfig) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder
        .follow_links(false)
        .hidden(!config.include_hidden)
        .ignore(config.respect_ignore_files)
        .git_ignore(config.respect_ignore_files)
        .git_global(config.respect_ignore_files)
        .git_exclude(config.respect_ignore_files)
        .parents(config.respect_ignore_files);
    builder
}

fn grep_walk_builder(
    root: &Path,
    overrides: &Override,
    type_matcher: Option<&Types>,
) -> WalkBuilder {
    let mut builder = walk_builder(root, grep_walk_config());
    builder.overrides(overrides.clone());
    if let Some(type_matcher) = type_matcher {
        builder.types(type_matcher.clone());
    }
    builder
}

fn build_overrides(root: &Path, patterns: &[String]) -> io::Result<Override> {
    let mut builder = OverrideBuilder::new(root);
    for pattern in patterns {
        builder.add(pattern).map_err(invalid_pattern_error)?;
    }
    builder.build().map_err(invalid_pattern_error)
}

fn path_matches_overrides(overrides: &Override, path: &Path, is_dir: bool) -> bool {
    !overrides.matched(path, is_dir).is_ignore()
}

fn entry_is_file(entry: &DirEntry) -> bool {
    entry
        .file_type()
        .is_some_and(|file_type| file_type.is_file())
}

fn split_grep_globs(glob: &str) -> Vec<String> {
    let mut glob_patterns = Vec::new();
    for raw_pattern in glob.split_whitespace() {
        if raw_pattern.contains('{') && raw_pattern.contains('}') {
            glob_patterns.push(raw_pattern.to_string());
        } else {
            glob_patterns.extend(
                raw_pattern
                    .split(',')
                    .filter(|pattern| !pattern.is_empty())
                    .map(str::to_string),
            );
        }
    }
    glob_patterns
}

fn glob_root_and_pattern(root: &Path, pattern: &str) -> (PathBuf, String) {
    let pattern = normalize_pattern(pattern);
    if Path::new(&pattern).is_absolute()
        && let Some((base_dir, relative_pattern)) = absolute_glob_base(&pattern)
    {
        return (base_dir, relative_pattern);
    }
    if let Some((base_dir, relative_pattern)) = relative_glob_base(&pattern) {
        return (root.join(base_dir), relative_pattern);
    }

    (root.to_path_buf(), pattern)
}

fn relative_glob_base(pattern: &str) -> Option<(PathBuf, String)> {
    if Path::new(pattern).is_absolute() {
        return None;
    }
    let glob_index = pattern.find(['*', '?', '[', '{']).unwrap_or(pattern.len());
    let static_prefix = &pattern[..glob_index];
    let last_separator = static_prefix.rfind('/')?;
    let base_dir = PathBuf::from(&static_prefix[..last_separator]);
    if base_dir.as_os_str().is_empty() {
        return None;
    }
    let mut relative_pattern = pattern[last_separator + 1..].to_string();
    if !relative_pattern.contains('/') && !relative_pattern.contains("**") {
        relative_pattern = format!("/{relative_pattern}");
    }
    Some((base_dir, relative_pattern))
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
    let mut relative_pattern = pattern[last_separator + 1..].to_string();
    if !relative_pattern.contains('/') && !relative_pattern.contains("**") {
        relative_pattern = format!("/{relative_pattern}");
    }
    Some((base_dir, relative_pattern))
}

fn normalize_pattern(pattern: &str) -> String {
    pattern
        .strip_prefix("./")
        .unwrap_or(pattern)
        .replace('\\', "/")
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

fn env_truthy_or_default(name: &str, default: bool) -> bool {
    let Some(value) = std::env::var_os(name) else {
        return default;
    };
    let value = value.to_string_lossy();
    if value.is_empty() {
        return default;
    }
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
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
