use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::PoisonError;

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
use codex_exec_server::FileMetadata;
use codex_exec_server::FileSystemSandboxContext;
use codex_exec_server::GlobSearchRequest;
use codex_exec_server::GrepOutputMode;
use codex_exec_server::GrepSearchRequest;
use codex_exec_server::GrepSearchResponse;
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
use serde::Deserialize;
use serde_json::json;

const DEFAULT_READ_LINE_LIMIT: usize = 2_000;
const DEFAULT_GLOB_RESULT_LIMIT: usize = 100;
const DEFAULT_GREP_HEAD_LIMIT: usize = 250;
const FILE_UNCHANGED_STUB: &str = "File unchanged since last read. The content from the earlier Read tool_result in this conversation is still current — refer to that instead of re-reading.";
const EMPTY_FILE_REMINDER: &str =
    "<system-reminder>Warning: the file exists but the contents are empty.</system-reminder>";
const FILE_HAS_NOT_BEEN_READ_ERROR: &str =
    "File has not been read yet. Read it first before writing to it.";
const FILE_MODIFIED_SINCE_READ_ERROR: &str = "File has been modified since read, either by the user or by a linter. Read it again before attempting to write it.";
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

#[derive(Clone, Debug)]
struct FileReadState {
    content: String,
    modified_at_ms: i64,
    offset: Option<usize>,
    limit: Option<usize>,
    is_partial_view: bool,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct FileReadStateKey {
    environment_id: Option<String>,
    path: String,
}

#[derive(Debug, Default)]
struct FileReadStateStore {
    entries: Mutex<HashMap<FileReadStateKey, FileReadState>>,
}

impl FileReadStateStore {
    fn get(&self, key: &FileReadStateKey) -> Option<FileReadState> {
        self.entries().get(key).cloned()
    }

    fn insert(&self, key: FileReadStateKey, state: FileReadState) {
        self.entries().insert(key, state);
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<FileReadStateKey, FileReadState>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
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
            session,
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
        let read_state = session
            .services
            .session_extension_data
            .get_or_init(FileReadStateStore::default);

        let text = match self.kind {
            AstralFileToolKind::Read => unreachable!("Read is handled before generic file tools"),
            AstralFileToolKind::Write => {
                let output = write_file(
                    arguments,
                    fs.as_ref(),
                    &sandbox,
                    &cwd,
                    environment_id.clone(),
                    read_state.as_ref(),
                )
                .await?;
                tracker.lock().await.invalidate();
                output
            }
            AstralFileToolKind::Edit => {
                let output = edit_file(
                    arguments,
                    fs.as_ref(),
                    &sandbox,
                    &cwd,
                    environment_id.clone(),
                    read_state.as_ref(),
                )
                .await?;
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
        strict: true,
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
}

async fn read_file(
    args: ReadArgs,
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    cwd: &AbsolutePathBuf,
    read_state: &FileReadStateStore,
) -> Result<String, FunctionCallError> {
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

    let metadata = read_metadata(fs, sandbox, cwd, &path).await?;
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
    let start_line = args.offset.unwrap_or(1).max(1);
    let start = start_line.saturating_sub(1).min(lines.len());
    let requested_limit = args.limit.unwrap_or(DEFAULT_READ_LINE_LIMIT);
    let end = start.saturating_add(requested_limit).min(lines.len());
    let requested_offset = Some(start_line);
    let state_key = read_state_key(fs, sandbox, args.environment_id.clone(), &path).await?;
    let is_partial_view = args.offset.is_some() || args.limit.is_some() || end < lines.len();

    if let Some(previous) = read_state.get(&state_key)
        && !previous.is_partial_view
        && previous.offset == requested_offset
        && previous.limit == args.limit
        && previous.content == text
    {
        return Ok(FILE_UNCHANGED_STUB.to_string());
    }

    read_state.insert(
        state_key,
        FileReadState {
            content: text.to_string(),
            modified_at_ms: metadata.modified_at_ms,
            offset: requested_offset,
            limit: args.limit,
            is_partial_view,
        },
    );

    if lines.is_empty() {
        return Ok(EMPTY_FILE_REMINDER.to_string());
    }
    if start_line > lines.len() {
        return Ok(format!(
            "<system-reminder>Warning: the file exists but is shorter than the provided offset ({start_line}). The file has {} lines.</system-reminder>",
            lines.len()
        ));
    }

    Ok(add_line_numbers(&lines[start..end], start + 1))
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
    let read_state = invocation
        .session
        .services
        .session_extension_data
        .get_or_init(FileReadStateStore::default);

    let text = read_file(args, fs.as_ref(), &sandbox, &cwd, read_state.as_ref()).await?;
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
    environment_id: Option<String>,
    read_state: &FileReadStateStore,
) -> Result<String, FunctionCallError> {
    let args: WriteArgs = parse_arguments(&arguments)?;
    let path = resolve_path(cwd, &args.file_path);
    let existing_metadata = optional_file_metadata(fs, sandbox, &path).await?;
    if let Some(metadata) = existing_metadata.as_ref() {
        if !metadata.is_file {
            return Err(FunctionCallError::RespondToModel(format!(
                "`{}` is not a file",
                path.display()
            )));
        }
        let current_text = read_text_lossy(fs, sandbox, &path).await?;
        let state_key = read_state_key(fs, sandbox, environment_id.clone(), &path).await?;
        validate_full_read_state(
            read_state,
            &state_key,
            &current_text,
            metadata.modified_at_ms,
        )?;
        write_file_contents(fs, sandbox, &path, args.content.clone().into_bytes()).await?;
        record_full_file_state(fs, sandbox, read_state, environment_id, &path, args.content).await;
        Ok(format!(
            "The file {} has been updated successfully.",
            path.display()
        ))
    } else {
        write_file_contents(fs, sandbox, &path, args.content.clone().into_bytes()).await?;
        record_full_file_state(fs, sandbox, read_state, environment_id, &path, args.content).await;
        Ok(format!("File created successfully at: {}", path.display()))
    }
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
    environment_id: Option<String>,
    read_state: &FileReadStateStore,
) -> Result<String, FunctionCallError> {
    let args: EditArgs = parse_arguments(&arguments)?;
    if args.old_string == args.new_string {
        return Err(FunctionCallError::RespondToModel(
            "No changes to make: old_string and new_string are exactly the same.".to_string(),
        ));
    }

    let path = resolve_path(cwd, &args.file_path);
    let current = read_existing_text(fs, sandbox, cwd, &path).await?;
    if args.old_string.is_empty() {
        return edit_empty_old_string(
            args,
            fs,
            sandbox,
            read_state,
            environment_id,
            &path,
            current,
        )
        .await;
    }

    let Some((text, metadata)) = current else {
        return Err(file_does_not_exist_error(cwd));
    };
    let state_key = read_state_key(fs, sandbox, environment_id.clone(), &path).await?;
    validate_full_read_state(read_state, &state_key, &text, metadata.modified_at_ms)?;
    let occurrences = text.matches(&args.old_string).count();
    if occurrences == 0 {
        return Err(FunctionCallError::RespondToModel(format!(
            "String to replace not found in file.\nString: {}",
            args.old_string
        )));
    }
    if occurrences > 1 && !args.replace_all {
        return Err(FunctionCallError::RespondToModel(format!(
            "Found {occurrences} matches of the string to replace, but replace_all is false. To replace all occurrences, set replace_all to true. To replace only one occurrence, please provide more context to uniquely identify the instance.\nString: {}",
            args.old_string
        )));
    }

    let updated = if args.replace_all {
        text.replace(&args.old_string, &args.new_string)
    } else {
        text.replacen(&args.old_string, &args.new_string, 1)
    };
    write_file_contents(fs, sandbox, &path, updated.clone().into_bytes()).await?;
    record_full_file_state(fs, sandbox, read_state, environment_id, &path, updated).await;

    if args.replace_all {
        Ok(format!(
            "The file {} has been updated. All occurrences were successfully replaced.",
            path.display()
        ))
    } else {
        Ok(format!(
            "The file {} has been updated successfully.",
            path.display()
        ))
    }
}

async fn edit_empty_old_string(
    args: EditArgs,
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    read_state: &FileReadStateStore,
    environment_id: Option<String>,
    path: &AbsolutePathBuf,
    current: Option<(String, FileMetadata)>,
) -> Result<String, FunctionCallError> {
    if let Some((text, _metadata)) = current
        && !text.trim().is_empty()
    {
        return Err(FunctionCallError::RespondToModel(
            "Cannot create new file - file already exists.".to_string(),
        ));
    }

    write_file_contents(fs, sandbox, path, args.new_string.clone().into_bytes()).await?;
    record_full_file_state(
        fs,
        sandbox,
        read_state,
        environment_id,
        path,
        args.new_string,
    )
    .await;
    Ok(format!(
        "The file {} has been updated successfully.",
        path.display()
    ))
}

async fn read_metadata(
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    cwd: &AbsolutePathBuf,
    path: &AbsolutePathBuf,
) -> Result<FileMetadata, FunctionCallError> {
    fs.get_metadata(path, Some(sandbox)).await.map_err(|err| {
        if err.kind() == ErrorKind::NotFound {
            file_does_not_exist_error(cwd)
        } else {
            FunctionCallError::RespondToModel(format!("unable to read `{}`: {err}", path.display()))
        }
    })
}

async fn optional_file_metadata(
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    path: &AbsolutePathBuf,
) -> Result<Option<FileMetadata>, FunctionCallError> {
    fs.get_metadata(path, Some(sandbox))
        .await
        .map(Some)
        .or_else(|err| {
            if err.kind() == ErrorKind::NotFound {
                Ok(None)
            } else {
                Err(FunctionCallError::RespondToModel(format!(
                    "unable to inspect `{}`: {err}",
                    path.display()
                )))
            }
        })
}

async fn read_existing_text(
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    cwd: &AbsolutePathBuf,
    path: &AbsolutePathBuf,
) -> Result<Option<(String, FileMetadata)>, FunctionCallError> {
    let metadata = match optional_file_metadata(fs, sandbox, path).await? {
        Some(metadata) => metadata,
        None => return Ok(None),
    };
    if !metadata.is_file {
        return Err(FunctionCallError::RespondToModel(format!(
            "`{}` is not a file",
            path.display()
        )));
    }
    let bytes = fs.read_file(path, Some(sandbox)).await.map_err(|err| {
        if err.kind() == ErrorKind::NotFound {
            file_does_not_exist_error(cwd)
        } else {
            FunctionCallError::RespondToModel(format!("unable to read `{}`: {err}", path.display()))
        }
    })?;
    let text = String::from_utf8(bytes).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "unable to edit `{}` because it is not valid UTF-8: {err}",
            path.display()
        ))
    })?;
    Ok(Some((text, metadata)))
}

async fn read_text_lossy(
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    path: &AbsolutePathBuf,
) -> Result<String, FunctionCallError> {
    let bytes = fs.read_file(path, Some(sandbox)).await.map_err(|err| {
        FunctionCallError::RespondToModel(format!("unable to read `{}`: {err}", path.display()))
    })?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn read_state_key(
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    environment_id: Option<String>,
    path: &AbsolutePathBuf,
) -> Result<FileReadStateKey, FunctionCallError> {
    let canonical = fs.canonicalize(path, Some(sandbox)).await.map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "unable to canonicalize `{}`: {err}",
            path.display()
        ))
    })?;
    Ok(FileReadStateKey {
        environment_id,
        path: canonical.to_string_lossy().into_owned(),
    })
}

async fn best_effort_read_state_key(
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    environment_id: Option<String>,
    path: &AbsolutePathBuf,
) -> FileReadStateKey {
    let canonical = fs
        .canonicalize(path, Some(sandbox))
        .await
        .unwrap_or_else(|_| path.clone());
    FileReadStateKey {
        environment_id,
        path: canonical.to_string_lossy().into_owned(),
    }
}

fn validate_full_read_state(
    read_state: &FileReadStateStore,
    key: &FileReadStateKey,
    current_text: &str,
    current_modified_at_ms: i64,
) -> Result<(), FunctionCallError> {
    let Some(state) = read_state.get(key) else {
        return Err(FunctionCallError::RespondToModel(
            FILE_HAS_NOT_BEEN_READ_ERROR.to_string(),
        ));
    };
    if state.is_partial_view {
        return Err(FunctionCallError::RespondToModel(
            FILE_HAS_NOT_BEEN_READ_ERROR.to_string(),
        ));
    }
    if state.modified_at_ms < current_modified_at_ms && state.content == current_text {
        return Ok(());
    }
    if state.content != current_text {
        return Err(FunctionCallError::RespondToModel(
            FILE_MODIFIED_SINCE_READ_ERROR.to_string(),
        ));
    }
    Ok(())
}

async fn record_full_file_state(
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    read_state: &FileReadStateStore,
    environment_id: Option<String>,
    path: &AbsolutePathBuf,
    content: String,
) {
    let key = best_effort_read_state_key(fs, sandbox, environment_id, path).await;
    let modified_at_ms = fs
        .get_metadata(path, Some(sandbox))
        .await
        .map(|metadata| metadata.modified_at_ms)
        .unwrap_or(0);
    read_state.insert(
        key,
        FileReadState {
            content,
            modified_at_ms,
            offset: None,
            limit: None,
            is_partial_view: false,
        },
    );
}

fn file_does_not_exist_error(cwd: &AbsolutePathBuf) -> FunctionCallError {
    FunctionCallError::RespondToModel(format!(
        "File does not exist. Note: your current working directory is {}.",
        cwd.display()
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
    if let Some(path) = args.path.as_deref() {
        validate_glob_path(fs, sandbox, &root, path, cwd).await?;
    }
    let response = fs
        .glob_search(
            GlobSearchRequest {
                root,
                pattern: args.pattern,
                max_results: DEFAULT_GLOB_RESULT_LIMIT,
            },
            Some(sandbox),
        )
        .await
        .map_err(|err| FunctionCallError::RespondToModel(format!("glob search failed: {err}")))?;
    Ok(format_glob_response(response, cwd))
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
    #[serde(default, rename = "-C")]
    context_c: Option<usize>,
    #[serde(default)]
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
    if let Some(path) = args.path.as_deref() {
        validate_grep_path(fs, sandbox, &root, path, cwd).await?;
    }
    let output_mode = grep_output_mode(args.output_mode.as_deref())?;
    let (context_before, context_after) = if let Some(context) = args.context {
        (context, context)
    } else if let Some(context_c) = args.context_c {
        (context_c, context_c)
    } else {
        (args.before.unwrap_or(0), args.after.unwrap_or(0))
    };
    let line_numbers = args
        .line_numbers
        .unwrap_or(output_mode == GrepOutputMode::Content);
    let limit = args.head_limit.unwrap_or(DEFAULT_GREP_HEAD_LIMIT);
    let response = fs
        .grep_search(
            GrepSearchRequest {
                root: root.clone(),
                pattern: args.pattern,
                glob: args.glob,
                file_type: args.file_type,
                output_mode,
                context_before,
                context_after,
                line_numbers,
                ignore_case: args.ignore_case,
                head_limit: limit,
                offset: args.offset.unwrap_or(0),
                multiline: args.multiline,
            },
            Some(sandbox),
        )
        .await
        .map_err(|err| FunctionCallError::RespondToModel(format!("grep search failed: {err}")))?;
    Ok(format_grep_response(response, output_mode, &root, cwd))
}

fn resolve_path(cwd: &AbsolutePathBuf, path: &str) -> AbsolutePathBuf {
    cwd.join(path.trim())
}

async fn validate_glob_path(
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    root: &AbsolutePathBuf,
    path: &str,
    cwd: &AbsolutePathBuf,
) -> Result<(), FunctionCallError> {
    if is_unc_path(path) {
        return Ok(());
    }
    let metadata = fs.get_metadata(root, Some(sandbox)).await.map_err(|err| {
        if err.kind() == ErrorKind::NotFound {
            FunctionCallError::RespondToModel(format!(
                "Directory does not exist: {path}. Note: your current working directory is {}.",
                cwd.display()
            ))
        } else {
            FunctionCallError::RespondToModel(format!(
                "unable to inspect `{}`: {err}",
                root.display()
            ))
        }
    })?;
    if !metadata.is_directory {
        return Err(FunctionCallError::RespondToModel(format!(
            "Path is not a directory: {path}"
        )));
    }
    Ok(())
}

async fn validate_grep_path(
    fs: &dyn ExecutorFileSystem,
    sandbox: &FileSystemSandboxContext,
    root: &AbsolutePathBuf,
    path: &str,
    cwd: &AbsolutePathBuf,
) -> Result<(), FunctionCallError> {
    if is_unc_path(path) {
        return Ok(());
    }
    fs.get_metadata(root, Some(sandbox)).await.map_err(|err| {
        if err.kind() == ErrorKind::NotFound {
            FunctionCallError::RespondToModel(format!(
                "Path does not exist: {path}. Note: your current working directory is {}.",
                cwd.display()
            ))
        } else {
            FunctionCallError::RespondToModel(format!(
                "unable to inspect `{}`: {err}",
                root.display()
            ))
        }
    })?;
    Ok(())
}

fn is_unc_path(path: &str) -> bool {
    path.starts_with(r"\\") || path.starts_with("//")
}

fn grep_output_mode(output_mode: Option<&str>) -> Result<GrepOutputMode, FunctionCallError> {
    match output_mode.unwrap_or("files_with_matches") {
        "content" => Ok(GrepOutputMode::Content),
        "files_with_matches" => Ok(GrepOutputMode::FilesWithMatches),
        "count" => Ok(GrepOutputMode::Count),
        other => Err(FunctionCallError::RespondToModel(format!(
            "unsupported Grep output_mode `{other}`"
        ))),
    }
}

fn format_grep_lines(
    lines: Vec<String>,
    output_mode: GrepOutputMode,
    root: &AbsolutePathBuf,
    cwd: &AbsolutePathBuf,
) -> Vec<String> {
    let prefix = display_path(root, cwd);
    let prefix = (!prefix.is_empty()).then_some(prefix);
    lines
        .into_iter()
        .map(|line| format_grep_line(line, output_mode, prefix.as_deref()))
        .collect()
}

fn format_grep_line(line: String, output_mode: GrepOutputMode, prefix: Option<&str>) -> String {
    match output_mode {
        GrepOutputMode::FilesWithMatches => prefix_relative_path(&line, prefix),
        GrepOutputMode::Count | GrepOutputMode::Content => {
            let Some((path, rest)) = line.split_once(':') else {
                return format_grep_context_line(&line, prefix);
            };
            format!("{}:{rest}", prefix_relative_path(path, prefix))
        }
    }
}

fn format_grep_context_line(line: &str, prefix: Option<&str>) -> String {
    if line == "--" {
        return line.to_string();
    }
    let Some((path, rest)) = split_context_line(line) else {
        return prefix_relative_path(line, prefix);
    };
    format!("{}-{rest}", prefix_relative_path(path, prefix))
}

fn split_context_line(line: &str) -> Option<(&str, &str)> {
    let (path_and_line, _text) = line.rsplit_once('-')?;
    let (path, line_number) = path_and_line.rsplit_once('-')?;
    if !line_number.chars().all(|char| char.is_ascii_digit()) {
        return None;
    }
    let rest = &line[path.len() + 1..];
    Some((path, rest))
}

fn prefix_relative_path(path: &str, prefix: Option<&str>) -> String {
    let Some(prefix) = prefix else {
        return path.to_string();
    };
    if path.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}/{path}")
    }
}

fn format_glob_response(
    response: codex_exec_server::GlobSearchResponse,
    cwd: &AbsolutePathBuf,
) -> String {
    let mut lines = response
        .matches
        .into_iter()
        .map(|matched| display_path(&matched.path, cwd))
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return "No files found".to_string();
    }
    if response.truncated {
        lines.push(
            "(Results are truncated. Consider using a more specific path or pattern.)".to_string(),
        );
    }
    lines.join("\n")
}

fn format_grep_response(
    response: GrepSearchResponse,
    output_mode: GrepOutputMode,
    root: &AbsolutePathBuf,
    cwd: &AbsolutePathBuf,
) -> String {
    let lines = format_grep_lines(response.lines, output_mode, root, cwd);
    let limit_info = format_limit_info(response.applied_limit, response.applied_offset);
    match output_mode {
        GrepOutputMode::Content => {
            let result = if lines.is_empty() {
                "No matches found".to_string()
            } else {
                lines.join("\n")
            };
            if limit_info.is_empty() {
                result
            } else {
                format!("{result}\n\n[Showing results with pagination = {limit_info}]")
            }
        }
        GrepOutputMode::Count => {
            let raw_content = if lines.is_empty() {
                "No matches found".to_string()
            } else {
                lines.join("\n")
            };
            let num_matches = response.num_matches.unwrap_or(0);
            let occurrence = if num_matches == 1 {
                "occurrence"
            } else {
                "occurrences"
            };
            let file = if response.num_files == 1 {
                "file"
            } else {
                "files"
            };
            let pagination = if limit_info.is_empty() {
                String::new()
            } else {
                format!(" with pagination = {limit_info}")
            };
            format!(
                "{raw_content}\n\nFound {num_matches} total {occurrence} across {} {file}.{pagination}",
                response.num_files
            )
        }
        GrepOutputMode::FilesWithMatches => {
            if response.num_files == 0 {
                return "No files found".to_string();
            }
            let file = if response.num_files == 1 {
                "file"
            } else {
                "files"
            };
            let pagination = if limit_info.is_empty() {
                String::new()
            } else {
                format!(" {limit_info}")
            };
            format!(
                "Found {} {file}{pagination}\n{}",
                response.num_files,
                lines.join("\n")
            )
        }
    }
}

fn format_limit_info(applied_limit: Option<usize>, applied_offset: Option<usize>) -> String {
    let mut parts = Vec::new();
    if let Some(applied_limit) = applied_limit {
        parts.push(format!("limit: {applied_limit}"));
    }
    if let Some(applied_offset) = applied_offset
        && applied_offset > 0
    {
        parts.push(format!("offset: {applied_offset}"));
    }
    parts.join(", ")
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

#[cfg(test)]
#[path = "astral_file_tools_tests.rs"]
mod tests;
