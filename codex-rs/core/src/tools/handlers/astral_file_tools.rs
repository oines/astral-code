use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;

use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::ViewImageHandler;
use crate::tools::handlers::parse_arguments;
use crate::tools::handlers::resolve_tool_environment;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_exec_server::ExecutorFileSystem;
use codex_exec_server::FileSystemSandboxContext;
use codex_tools::EDIT_TOOL_NAME;
use codex_tools::GLOB_TOOL_NAME;
use codex_tools::GREP_TOOL_NAME;
use codex_tools::READ_TOOL_NAME;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::WRITE_TOOL_NAME;
use codex_tools::astral_core_tool_by_name;
use codex_tools::parse_tool_input_schema_without_compaction;
use codex_utils_absolute_path::AbsolutePathBuf;
use regex_lite::Regex;
use serde::Deserialize;
use serde_json::json;

const DEFAULT_READ_LINE_LIMIT: usize = 2_000;
const DEFAULT_GREP_HEAD_LIMIT: usize = 250;
const MAX_SCAN_ENTRIES: usize = 10_000;
const MAX_RESULT_LINES: usize = 1_000;
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
const BLOCKED_DEVICE_PATHS: &[&str] = &[
    "/dev/zero",
    "/dev/random",
    "/dev/urandom",
    "/dev/full",
    "/dev/stdin",
    "/dev/tty",
    "/dev/console",
    "/dev/stdout",
    "/dev/stderr",
    "/dev/fd/0",
    "/dev/fd/1",
    "/dev/fd/2",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AstralFileToolKind {
    Read,
    Write,
    Edit,
    Glob,
    Grep,
}

pub(crate) struct AstralFileToolHandler {
    kind: AstralFileToolKind,
}

impl AstralFileToolHandler {
    pub(crate) fn new(kind: AstralFileToolKind) -> Self {
        Self { kind }
    }

    fn name(&self) -> &'static str {
        self.kind.name()
    }
}

impl AstralFileToolKind {
    fn name(self) -> &'static str {
        match self {
            Self::Read => READ_TOOL_NAME,
            Self::Write => WRITE_TOOL_NAME,
            Self::Edit => EDIT_TOOL_NAME,
            Self::Glob => GLOB_TOOL_NAME,
            Self::Grep => GREP_TOOL_NAME,
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for AstralFileToolHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(self.name())
    }

    fn spec(&self) -> ToolSpec {
        astral_file_tool_spec(self.name())
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        matches!(
            self.kind,
            AstralFileToolKind::Read | AstralFileToolKind::Glob | AstralFileToolKind::Grep
        )
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        if self.kind == AstralFileToolKind::Read {
            return handle_read_invocation(invocation).await;
        }

        let ToolInvocation {
            turn,
            payload,
            tracker,
            ..
        } = invocation;
        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(format!(
                    "{} handler received unsupported payload",
                    self.name()
                )));
            }
        };

        let environment_id = file_environment_id(&arguments)?;
        let Some(turn_environment) =
            resolve_tool_environment(turn.as_ref(), environment_id.as_deref())?
        else {
            return Err(FunctionCallError::RespondToModel(format!(
                "{} is unavailable in this session",
                self.name()
            )));
        };
        let cwd = turn_environment.cwd.clone();
        let fs = turn_environment.environment.get_filesystem();
        let sandbox = turn.file_system_sandbox_context(/*additional_permissions*/ None, &cwd);

        let text = match self.kind {
            AstralFileToolKind::Read => unreachable!("Read is handled before generic file tools"),
            AstralFileToolKind::Write => {
                let output = write_file(arguments, fs.as_ref(), &sandbox, &cwd).await?;
                tracker.lock().await.invalidate();
                output
            }
            AstralFileToolKind::Edit => {
                let output = edit_file(arguments, fs.as_ref(), &sandbox, &cwd).await?;
                tracker.lock().await.invalidate();
                output
            }
            AstralFileToolKind::Glob => glob_files(arguments, fs.as_ref(), &sandbox, &cwd).await?,
            AstralFileToolKind::Grep => grep_files(arguments, fs.as_ref(), &sandbox, &cwd).await?,
        };

        Ok(boxed_tool_output(FunctionToolOutput::from_text(
            text,
            Some(true),
        )))
    }
}

impl CoreToolRuntime for AstralFileToolHandler {}

fn astral_file_tool_spec(name: &str) -> ToolSpec {
    let tool = astral_core_tool_by_name(name).unwrap_or_else(|| {
        panic!("astral core tool `{name}` should have a schema");
    });
    let parameters = parse_tool_input_schema_without_compaction(&tool.input_schema)
        .unwrap_or_else(|err| panic!("astral core tool `{name}` schema should parse: {err}"));
    ToolSpec::Function(ResponsesApiTool {
        name: tool.name,
        description: tool.description,
        strict: false,
        defer_loading: None,
        parameters,
        output_schema: None,
    })
}

#[derive(Deserialize)]
struct ReadArgs {
    file_path: String,
    #[serde(default, rename = "environment_id", alias = "environmentId")]
    environment_id: Option<String>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    pages: Option<String>,
}

async fn read_file(
    args: ReadArgs,
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    cwd: &AbsolutePathBuf,
) -> Result<String, FunctionCallError> {
    if args.pages.is_some() {
        return Err(FunctionCallError::RespondToModel(
            "Read pages are not supported yet; read PDFs through Bash or a dedicated reader"
                .to_string(),
        ));
    }

    let path = resolve_path(cwd, &args.file_path);
    if is_blocked_device_path(&path) {
        return Err(FunctionCallError::RespondToModel(format!(
            "Cannot read '{}': this device file would block or produce infinite output.",
            args.file_path
        )));
    }
    if is_pdf_path(&path) {
        return Err(FunctionCallError::RespondToModel(
            "Read does not support PDFs yet; use Bash with an appropriate PDF extraction tool"
                .to_string(),
        ));
    }

    let metadata = fs.get_metadata(&path, Some(sandbox)).await.map_err(|err| {
        FunctionCallError::RespondToModel(format!("unable to read `{}`: {err}", path.display()))
    })?;
    if !metadata.is_file {
        return Err(FunctionCallError::RespondToModel(format!(
            "`{}` is not a file",
            path.display()
        )));
    }

    let bytes = fs.read_file(&path, Some(sandbox)).await.map_err(|err| {
        FunctionCallError::RespondToModel(format!("unable to read `{}`: {err}", path.display()))
    })?;
    let text = String::from_utf8_lossy(&bytes);
    let lines = split_lines_preserving_newline(&text);
    let start = args.offset.unwrap_or(1).saturating_sub(1).min(lines.len());
    let requested_limit = args.limit.unwrap_or(DEFAULT_READ_LINE_LIMIT);
    let end = start.saturating_add(requested_limit).min(lines.len());
    let mut output = add_line_numbers(&lines[start..end], start + 1);

    if end < lines.len() {
        if !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(&format!(
            "[Showing lines {}-{} of {}; pass offset/limit to read more]\n",
            start + 1,
            end,
            lines.len()
        ));
    }

    Ok(output)
}

fn is_blocked_device_path(path: &Path) -> bool {
    let path = path.to_string_lossy();
    if BLOCKED_DEVICE_PATHS.contains(&path.as_ref()) {
        return true;
    }

    path.starts_with("/proc/")
        && (path.ends_with("/fd/0") || path.ends_with("/fd/1") || path.ends_with("/fd/2"))
}

async fn handle_read_invocation(
    mut invocation: ToolInvocation,
) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
    let ToolPayload::Function { arguments } = &invocation.payload else {
        return Err(FunctionCallError::RespondToModel(
            "Read handler received unsupported payload".to_string(),
        ));
    };
    let args: ReadArgs = parse_arguments(arguments)?;

    if is_image_path(Path::new(&args.file_path)) {
        invocation.tool_name = ToolName::plain("view_image");
        invocation.payload = ToolPayload::Function {
            arguments: json!({
                "path": args.file_path,
                "environment_id": args.environment_id,
            })
            .to_string(),
        };
        return ViewImageHandler::default().handle(invocation).await;
    }

    let Some(turn_environment) =
        resolve_tool_environment(invocation.turn.as_ref(), args.environment_id.as_deref())?
    else {
        return Err(FunctionCallError::RespondToModel(
            "Read is unavailable in this session".to_string(),
        ));
    };
    let cwd = turn_environment.cwd.clone();
    let fs = turn_environment.environment.get_filesystem();
    let sandbox = invocation
        .turn
        .file_system_sandbox_context(/*additional_permissions*/ None, &cwd);

    let text = read_file(args, fs.as_ref(), &sandbox, &cwd).await?;
    Ok(boxed_tool_output(FunctionToolOutput::from_text(
        text,
        Some(true),
    )))
}

#[derive(Deserialize)]
struct FileEnvironmentArgs {
    #[serde(default, rename = "environment_id", alias = "environmentId")]
    environment_id: Option<String>,
}

fn file_environment_id(arguments: &str) -> Result<Option<String>, FunctionCallError> {
    let args: FileEnvironmentArgs = parse_arguments(arguments)?;
    Ok(args.environment_id)
}

#[derive(Deserialize)]
struct WriteArgs {
    file_path: String,
    content: String,
}

async fn write_file(
    arguments: String,
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    cwd: &AbsolutePathBuf,
) -> Result<String, FunctionCallError> {
    let args: WriteArgs = parse_arguments(&arguments)?;
    let path = resolve_path(cwd, &args.file_path);
    write_file_contents(fs, sandbox, &path, args.content.into_bytes()).await?;
    Ok(format!("Wrote {}", display_path(&path, cwd)))
}

#[derive(Deserialize)]
struct EditArgs {
    file_path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

async fn edit_file(
    arguments: String,
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    cwd: &AbsolutePathBuf,
) -> Result<String, FunctionCallError> {
    let args: EditArgs = parse_arguments(&arguments)?;
    if args.old_string == args.new_string {
        return Err(FunctionCallError::RespondToModel(
            "new_string must be different from old_string".to_string(),
        ));
    }

    let path = resolve_path(cwd, &args.file_path);
    if args.old_string.is_empty() {
        return edit_empty_old_string(args, fs, sandbox, cwd, &path).await;
    }

    let bytes = fs.read_file(&path, Some(sandbox)).await.map_err(|err| {
        FunctionCallError::RespondToModel(format!("unable to read `{}`: {err}", path.display()))
    })?;
    let text = String::from_utf8(bytes).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "unable to edit `{}` because it is not valid UTF-8: {err}",
            path.display()
        ))
    })?;
    let occurrences = text.matches(&args.old_string).count();
    if occurrences == 0 {
        return Err(FunctionCallError::RespondToModel(
            "old_string not found".to_string(),
        ));
    }
    if occurrences > 1 && !args.replace_all {
        return Err(FunctionCallError::RespondToModel(
            "old_string appears multiple times; set replace_all to true".to_string(),
        ));
    }

    let updated = if args.replace_all {
        text.replace(&args.old_string, &args.new_string)
    } else {
        text.replacen(&args.old_string, &args.new_string, 1)
    };
    write_file_contents(fs, sandbox, &path, updated.into_bytes()).await?;

    Ok(format!(
        "Updated {} ({} replacement{})",
        display_path(&path, cwd),
        if args.replace_all { occurrences } else { 1 },
        if args.replace_all && occurrences != 1 {
            "s"
        } else {
            ""
        }
    ))
}

async fn edit_empty_old_string(
    args: EditArgs,
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    cwd: &AbsolutePathBuf,
    path: &AbsolutePathBuf,
) -> Result<String, FunctionCallError> {
    match fs.read_file(path, Some(sandbox)).await {
        Ok(bytes) if !bytes.is_empty() => {
            return Err(FunctionCallError::RespondToModel(
                "Cannot create new file - file already exists.".to_string(),
            ));
        }
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => {
            return Err(FunctionCallError::RespondToModel(format!(
                "unable to read `{}`: {err}",
                path.display()
            )));
        }
    }

    write_file_contents(fs, sandbox, path, args.new_string.into_bytes()).await?;
    Ok(format!(
        "The file {} has been updated successfully.",
        display_path(path, cwd)
    ))
}

async fn write_file_contents(
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    path: &AbsolutePathBuf,
    contents: Vec<u8>,
) -> Result<(), FunctionCallError> {
    fs.write_file(path, contents, Some(sandbox))
        .await
        .map_err(|err| {
            FunctionCallError::RespondToModel(format!(
                "unable to write `{}`: {err}",
                path.display()
            ))
        })
}

#[derive(Deserialize)]
struct GlobArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

async fn glob_files(
    arguments: String,
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    cwd: &AbsolutePathBuf,
) -> Result<String, FunctionCallError> {
    let args: GlobArgs = parse_arguments(&arguments)?;
    let root = resolve_path(cwd, args.path.as_deref().unwrap_or("."));
    let matcher = glob_regex(&args.pattern)?;
    let files = collect_files(fs, sandbox, &root).await?;
    let mut matches = Vec::new();
    for path in files {
        if !matcher.is_match(&relative_slash_path(&path, &root)) {
            continue;
        }
        let modified_at_ms = fs
            .get_metadata(&path, Some(sandbox))
            .await
            .ok()
            .map_or(0, |metadata| metadata.modified_at_ms);
        matches.push((display_path(&path, cwd), modified_at_ms));
    }
    matches.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    Ok(join_limited_lines(
        matches.into_iter().map(|(path, _)| path).collect(),
        MAX_RESULT_LINES,
    ))
}

#[derive(Deserialize)]
struct GrepArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    glob: Option<String>,
    #[serde(default)]
    output_mode: Option<String>,
    #[serde(default, rename = "-B", alias = "before")]
    before: Option<usize>,
    #[serde(default, rename = "-A", alias = "after")]
    after: Option<usize>,
    #[serde(default, rename = "-C", alias = "context")]
    context: Option<usize>,
    #[serde(default, rename = "-n", alias = "line_numbers")]
    line_numbers: Option<bool>,
    #[serde(default, rename = "-i", alias = "ignore_case")]
    ignore_case: bool,
    #[serde(default, rename = "type", alias = "file_type")]
    file_type: Option<String>,
    #[serde(default)]
    head_limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    multiline: bool,
}

async fn grep_files(
    arguments: String,
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    cwd: &AbsolutePathBuf,
) -> Result<String, FunctionCallError> {
    let args: GrepArgs = parse_arguments(&arguments)?;
    let root = resolve_path(cwd, args.path.as_deref().unwrap_or("."));
    let regex = grep_regex(&args)?;
    let glob = args.glob.as_deref().map(glob_regex).transpose()?;
    let files = files_for_grep(fs, sandbox, &root).await?;
    let output_mode = args.output_mode.as_deref().unwrap_or("files_with_matches");
    let context_before = args.context.or(args.before).unwrap_or(0);
    let context_after = args.context.or(args.after).unwrap_or(0);
    let line_numbers = args.line_numbers.unwrap_or(output_mode == "content");
    let mut output = Vec::new();

    for path in files {
        let display = display_path(&path, cwd);
        if !type_filter_matches(&path, args.file_type.as_deref()) {
            continue;
        }
        if let Some(glob) = &glob
            && !glob.is_match(&relative_slash_path(&path, &root))
            && !glob.is_match(&display)
        {
            continue;
        }

        let bytes = match fs.read_file(&path, Some(sandbox)).await {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let text = String::from_utf8_lossy(&bytes);
        let lines = split_lines_preserving_newline(&text);
        let matched_lines = matching_line_indexes(&regex, &lines, args.multiline, &text);
        if matched_lines.is_empty() {
            continue;
        }

        match output_mode {
            "files_with_matches" => output.push(display),
            "count" => output.push(format!("{display}:{}", matched_lines.len())),
            "content" => push_content_matches(
                &mut output,
                &display,
                &lines,
                &matched_lines,
                line_numbers,
                context_before,
                context_after,
            ),
            other => {
                return Err(FunctionCallError::RespondToModel(format!(
                    "unsupported Grep output_mode `{other}`"
                )));
            }
        }
    }

    let offset = args.offset.unwrap_or(0).min(output.len());
    let limit = match args.head_limit {
        Some(0) => MAX_RESULT_LINES,
        Some(limit) => limit.min(MAX_RESULT_LINES),
        None => DEFAULT_GREP_HEAD_LIMIT,
    };
    Ok(join_limited_lines(output[offset..].to_vec(), limit))
}

fn resolve_path(cwd: &AbsolutePathBuf, path: &str) -> AbsolutePathBuf {
    cwd.join(path)
}

async fn collect_files(
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    root: &AbsolutePathBuf,
) -> Result<Vec<AbsolutePathBuf>, FunctionCallError> {
    let mut files = Vec::new();
    let mut stack = vec![root.clone()];
    let mut visited = 0usize;

    while let Some(dir) = stack.pop() {
        visited += 1;
        if visited > MAX_SCAN_ENTRIES {
            return Err(FunctionCallError::RespondToModel(format!(
                "scan exceeded {MAX_SCAN_ENTRIES} filesystem entries; narrow the path or glob"
            )));
        }

        let entries = fs
            .read_directory(&dir, Some(sandbox))
            .await
            .map_err(|err| {
                FunctionCallError::RespondToModel(format!(
                    "unable to read directory `{}`: {err}",
                    dir.display()
                ))
            })?;
        for entry in entries {
            if entry.is_directory
                && SEARCH_PRUNED_DIRECTORY_NAMES.contains(&entry.file_name.as_str())
            {
                continue;
            }
            let path = fs
                .join(&dir, Path::new(&entry.file_name))
                .await
                .map_err(|err| {
                    FunctionCallError::RespondToModel(format!(
                        "unable to resolve `{}` under `{}`: {err}",
                        entry.file_name,
                        dir.display()
                    ))
                })?;
            if entry.is_directory {
                stack.push(path);
            } else if entry.is_file {
                files.push(path);
            }
        }
    }

    Ok(files)
}

async fn files_for_grep(
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    root: &AbsolutePathBuf,
) -> Result<Vec<AbsolutePathBuf>, FunctionCallError> {
    let metadata = fs.get_metadata(root, Some(sandbox)).await.map_err(|err| {
        FunctionCallError::RespondToModel(format!("unable to stat `{}`: {err}", root.display()))
    })?;
    if metadata.is_file {
        Ok(vec![root.clone()])
    } else if metadata.is_directory {
        collect_files(fs, sandbox, root).await
    } else {
        Ok(Vec::new())
    }
}

fn split_lines_preserving_newline(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split_inclusive('\n').collect()
}

fn add_line_numbers(lines: &[&str], start_line: usize) -> String {
    let mut output = String::new();
    for (index, line) in lines.iter().enumerate() {
        output.push_str(&format!("{}\t{}", start_line + index, line));
    }
    output
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
        for index in start..end {
            if last_pushed.is_some_and(|last| index <= last) {
                continue;
            }
            last_pushed = Some(index);
            let line = lines[index].trim_end_matches(['\r', '\n']);
            if line_numbers {
                output.push(format!("{display}:{}:{line}", index + 1));
            } else {
                output.push(format!("{display}:{line}"));
            }
        }
    }
}

fn grep_regex(args: &GrepArgs) -> Result<Regex, FunctionCallError> {
    let mut pattern = String::new();
    if args.ignore_case {
        pattern.push_str("(?i)");
    }
    if args.multiline {
        pattern.push_str("(?s)");
    }
    pattern.push_str(&args.pattern);
    Regex::new(&pattern).map_err(|err| {
        FunctionCallError::RespondToModel(format!("invalid Grep pattern `{}`: {err}", args.pattern))
    })
}

fn glob_regex(pattern: &str) -> Result<Regex, FunctionCallError> {
    let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
    let mut regex = String::from("^");
    let chars = pattern.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '*' if chars.get(index + 1) == Some(&'*') => {
                if chars.get(index + 2) == Some(&'/') {
                    regex.push_str("(?:.*/)?");
                    index += 3;
                } else {
                    regex.push_str(".*");
                    index += 2;
                }
            }
            '*' => {
                regex.push_str("[^/]*");
                index += 1;
            }
            '?' => {
                regex.push_str("[^/]");
                index += 1;
            }
            '/' => {
                regex.push('/');
                index += 1;
            }
            ch => {
                regex.push_str(&regex_lite::escape(&ch.to_string()));
                index += 1;
            }
        }
    }
    regex.push('$');
    Regex::new(&regex).map_err(|err| {
        FunctionCallError::RespondToModel(format!("invalid Glob pattern `{pattern}`: {err}"))
    })
}

fn relative_slash_path(path: &AbsolutePathBuf, root: &AbsolutePathBuf) -> String {
    path.as_path()
        .strip_prefix(root.as_path())
        .unwrap_or_else(|_| path.as_path())
        .to_string_lossy()
        .replace('\\', "/")
}

fn display_path(path: &AbsolutePathBuf, cwd: &AbsolutePathBuf) -> String {
    path.as_path()
        .strip_prefix(cwd.as_path())
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| PathBuf::from(path.as_path()))
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_image_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp")
    )
}

fn is_pdf_path(path: &AbsolutePathBuf) -> bool {
    path.as_path()
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

fn join_limited_lines(lines: Vec<String>, limit: usize) -> String {
    let total = lines.len();
    let mut selected = lines.into_iter().take(limit).collect::<Vec<_>>();
    if total > limit {
        selected.push(format!("[Showing first {limit} of {total} results]"));
    }
    if selected.is_empty() {
        String::new()
    } else {
        format!("{}\n", selected.join("\n"))
    }
}

fn type_filter_matches(path: &AbsolutePathBuf, file_type: Option<&str>) -> bool {
    let Some(file_type) = file_type else {
        return true;
    };
    let extension = path
        .as_path()
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    let expected = match file_type {
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
    extension == expected
}

#[cfg(test)]
#[path = "astral_file_tools_tests.rs"]
mod tests;
