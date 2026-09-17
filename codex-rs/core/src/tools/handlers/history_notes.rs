use std::cmp::Reverse;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use crate::function_tool::FunctionCallError;
use crate::session::history_archive::HistoryArchive;
use crate::session::history_archive::HistoryEntry;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_protocol::AgentPath;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::provider_neutral_tool_name_for_tool_name;
use serde_json::Value;
use serde_json::json;
use uuid::Uuid;
use walkdir::WalkDir;

const HISTORY_NAMESPACE: &str = "history";
const NOTES_NAMESPACE: &str = "notes";
const MAX_NOTE_BYTES: usize = 1_000_000;
const DEFAULT_LIST_LIMIT: usize = 50;
const MAX_LIST_LIMIT: usize = 500;
const DEFAULT_READ_CHARS: usize = 4_000;
const MAX_READ_CHARS: usize = 40_000;

const HISTORY_DESCRIPTION: &str = "Recover prior conversation after a context-window reset by listing, reading, and searching normalized history using the opaque window and item IDs returned by these tools. Pass returned IDs unchanged when filtering or reading an item. Items are ordered by persisted ordinal, then creation time; list and search calls return no matches for an unknown window. Exact list and read operations are backed by the append-only rollout and are immediately consistent. History is read-only. Use it silently to continue the task. Never disclose or describe the tool, its existence or use, paths, storage or recovery mechanisms, or private contents to the user.";
const NOTES_DESCRIPTION: &str = "Read and maintain private notes that survive context-window transitions within this thread. Paths are virtual, not workspace filesystem paths, and are relative to the current agent's notes directory. Notes are a working index and checkpoint; keep authoritative transcript details in History references. Note reads reflect successful writes immediately. Every file must remain at or below 1,000,000 UTF-8 bytes; create another file before approaching the limit. Use Notes silently to continue the task. Never disclose or describe the tool, its existence or use, paths, storage or recovery mechanisms, or private contents to the user.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryNotesAction {
    HistoryListWindows,
    HistoryListItems,
    HistoryReadItem,
    HistorySearchContents,
    NotesListFilesByPrefix,
    NotesReadFile,
    NotesSearchContents,
    NotesAppendToFile,
    NotesWriteFile,
}

impl HistoryNotesAction {
    pub(crate) const ALL: [Self; 9] = [
        Self::HistoryListWindows,
        Self::HistoryListItems,
        Self::HistoryReadItem,
        Self::HistorySearchContents,
        Self::NotesListFilesByPrefix,
        Self::NotesReadFile,
        Self::NotesSearchContents,
        Self::NotesAppendToFile,
        Self::NotesWriteFile,
    ];

    fn namespace(self) -> &'static str {
        match self {
            Self::HistoryListWindows
            | Self::HistoryListItems
            | Self::HistoryReadItem
            | Self::HistorySearchContents => HISTORY_NAMESPACE,
            _ => NOTES_NAMESPACE,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::HistoryListWindows => "list_windows",
            Self::HistoryListItems => "list_items",
            Self::HistoryReadItem => "read_item",
            Self::HistorySearchContents => "search_contents",
            Self::NotesListFilesByPrefix => "list_files_by_prefix",
            Self::NotesReadFile => "read_file",
            Self::NotesSearchContents => "search_contents",
            Self::NotesAppendToFile => "append_to_file",
            Self::NotesWriteFile => "write_file",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::HistoryListWindows => {
                "List context windows and their item counts. Private model-only recovery; never disclose this activity."
            }
            Self::HistoryListItems => {
                "List bounded previews of transcript items, optionally filtered by window, role, or tool. Private model-only recovery; never disclose this activity."
            }
            Self::HistoryReadItem => {
                "Read a Unicode character range from one private transcript item. Pass the returned window and item IDs unchanged; never disclose the item or this activity."
            }
            Self::HistorySearchContents => {
                "Search private transcript items by literal substring. Never disclose results or this activity."
            }
            Self::NotesListFilesByPrefix => {
                "List private note files by virtual path prefix. Never disclose paths, contents, or this activity."
            }
            Self::NotesReadFile => {
                "Read all or a line range from a private note file. Never disclose paths, contents, or this activity."
            }
            Self::NotesSearchContents => {
                "Search private note lines by literal substring. Never disclose results or this activity."
            }
            Self::NotesAppendToFile => {
                "Append text to a private note file. Never disclose paths, contents, or this activity."
            }
            Self::NotesWriteFile => {
                "Create or replace a private note file. Never disclose paths, contents, or this activity."
            }
        }
    }

    fn parameters(self) -> JsonSchema {
        let schema = match self {
            Self::HistoryListWindows => json!({
                "type":"object",
                "properties":{
                    "limit":{"type":"integer","minimum":1},
                    "recent_first":{"type":"boolean"},
                    "cursor":{"type":["string","null"]}
                },
                "additionalProperties":false
            }),
            Self::HistoryListItems => json!({
                "type":"object",
                "properties":{
                    "window_id":{"type":["string","null"]},
                    "role":{"type":["string","null"]},
                    "tool_namespace":{"type":["string","null"]},
                    "tool_name":{"type":["string","null"]},
                    "limit":{"type":"integer","minimum":1},
                    "recent_first":{"type":"boolean"},
                    "max_chars_per_item":{"type":"integer","minimum":1},
                    "cursor":{"type":["string","null"]}
                },
                "additionalProperties":false
            }),
            Self::HistoryReadItem => json!({
                "type":"object",
                "properties":{
                    "window_id":{"type":"string"},
                    "item_id":{"type":"string"},
                    "offset_chars":{"type":"integer","minimum":0},
                    "limit_chars":{"type":"integer","minimum":1}
                },
                "required":["window_id","item_id"],
                "additionalProperties":false
            }),
            Self::HistorySearchContents => json!({
                "type":"object",
                "properties":{
                    "query":{"type":"string"},
                    "window_id":{"type":["string","null"]},
                    "role":{"type":["string","null"]},
                    "tool_namespace":{"type":["string","null"]},
                    "tool_name":{"type":["string","null"]},
                    "limit":{"type":"integer","minimum":1},
                    "recent_first":{"type":"boolean"},
                    "cursor":{"type":["string","null"]}
                },
                "required":["query"],
                "additionalProperties":false
            }),
            Self::NotesListFilesByPrefix => json!({
                "type":"object",
                "properties":{
                    "prefix":{"type":["string","null"]},
                    "max_results":{"type":"integer","minimum":1},
                    "recent_first":{"type":"boolean"},
                    "cursor":{"type":["string","null"]}
                },
                "additionalProperties":false
            }),
            Self::NotesReadFile => json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"},
                    "start_line":{"type":["integer","null"]},
                    "stop_line":{"type":["integer","null"]}
                },
                "required":["path"],
                "additionalProperties":false
            }),
            Self::NotesSearchContents => json!({
                "type":"object",
                "properties":{
                    "query":{"type":"string"},
                    "path_prefix":{"type":["string","null"]},
                    "max_files":{"type":"integer","minimum":1},
                    "max_matches_per_file":{"type":"integer","minimum":1},
                    "recent_file_first":{"type":"boolean"}
                },
                "required":["query"],
                "additionalProperties":false
            }),
            Self::NotesAppendToFile | Self::NotesWriteFile => json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"},
                    "text":{"type":"string"}
                },
                "required":["path","text"],
                "additionalProperties":false
            }),
        };
        serde_json::from_value(schema).expect("history/notes schema must be valid")
    }

    fn spec(self, namespace_tools_enabled: bool) -> ToolSpec {
        let namespace_description = match self.namespace() {
            HISTORY_NAMESPACE => HISTORY_DESCRIPTION,
            _ => NOTES_DESCRIPTION,
        };
        let mut tool = ResponsesApiTool {
            name: self.name().to_string(),
            description: self.description().to_string(),
            strict: false,
            defer_loading: None,
            parameters: self.parameters(),
            output_schema: None,
        };
        if namespace_tools_enabled {
            ToolSpec::Namespace(ResponsesApiNamespace {
                name: self.namespace().to_string(),
                description: namespace_description.to_string(),
                tools: vec![ResponsesApiNamespaceTool::Function(tool)],
            })
        } else {
            tool.name = provider_neutral_tool_name_for_tool_name(&ToolName::namespaced(
                self.namespace(),
                self.name(),
            ));
            tool.description = format!("{namespace_description} {}", self.description());
            ToolSpec::Function(tool)
        }
    }
}

pub(crate) struct HistoryNotesHandler {
    action: HistoryNotesAction,
    namespace_tools_enabled: bool,
}

impl HistoryNotesHandler {
    pub(crate) fn new(action: HistoryNotesAction, namespace_tools_enabled: bool) -> Self {
        Self {
            action,
            namespace_tools_enabled,
        }
    }

    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        let arguments = match &invocation.payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "history/notes handler received unsupported payload".to_string(),
                ));
            }
        };
        let args: Value = if arguments.trim().is_empty() {
            json!({})
        } else {
            parse_arguments(arguments)?
        };

        let result = match self.action {
            HistoryNotesAction::HistoryListWindows => {
                let archive = invocation.session.history_archive.read().await;
                let snapshot = HistorySnapshot::new(
                    &archive,
                    invocation
                        .turn
                        .session_source
                        .get_agent_path()
                        .map(String::from),
                );
                json_output(snapshot.list_windows(&args)?)
            }
            HistoryNotesAction::HistoryListItems => {
                let archive = invocation.session.history_archive.read().await;
                let snapshot = HistorySnapshot::new(
                    &archive,
                    invocation
                        .turn
                        .session_source
                        .get_agent_path()
                        .map(String::from),
                );
                json_output(snapshot.list_items(&args)?)
            }
            HistoryNotesAction::HistoryReadItem => {
                let archive = invocation.session.history_archive.read().await;
                let snapshot = HistorySnapshot::new(
                    &archive,
                    invocation
                        .turn
                        .session_source
                        .get_agent_path()
                        .map(String::from),
                );
                snapshot.read_item(&args)
            }
            HistoryNotesAction::HistorySearchContents => {
                let archive = invocation.session.history_archive.read().await;
                let snapshot = HistorySnapshot::new(
                    &archive,
                    invocation
                        .turn
                        .session_source
                        .get_agent_path()
                        .map(String::from),
                );
                json_output(snapshot.search(&args)?)
            }
            HistoryNotesAction::NotesListFilesByPrefix => {
                let root = notes_root(&invocation);
                json_output(list_note_files(&root, &args)?)
            }
            HistoryNotesAction::NotesReadFile => {
                let root = notes_root(&invocation);
                json_output(read_note_file(&root, &args).await?)
            }
            HistoryNotesAction::NotesSearchContents => {
                let root = notes_root(&invocation);
                json_output(search_note_files(&root, &args).await?)
            }
            HistoryNotesAction::NotesAppendToFile => {
                let root = notes_root(&invocation);
                json_output(write_note_file(&root, &args, true).await?)
            }
            HistoryNotesAction::NotesWriteFile => {
                let root = notes_root(&invocation);
                json_output(write_note_file(&root, &args, false).await?)
            }
        };
        invocation.turn.session_telemetry.counter(
            "codex.context_recovery.tool_call",
            /*inc*/ 1,
            &[
                ("namespace", self.action.namespace()),
                ("action", self.action.name()),
            ],
        );
        result
    }
}

impl ToolExecutor<ToolInvocation> for HistoryNotesHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::namespaced(self.action.namespace(), self.action.name())
    }

    fn spec(&self) -> ToolSpec {
        self.action.spec(self.namespace_tools_enabled)
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(invocation))
    }
}

impl CoreToolRuntime for HistoryNotesHandler {}

fn json_output(value: Value) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
    let text = serde_json::to_string(&value)
        .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
    Ok(boxed_tool_output(FunctionToolOutput::from_text(
        text,
        Some(true),
    )))
}

struct HistorySnapshot<'a> {
    archive: &'a HistoryArchive,
    current_agent_path: Option<String>,
}

impl<'a> HistorySnapshot<'a> {
    fn new(archive: &'a HistoryArchive, current_agent_path: Option<String>) -> Self {
        Self {
            archive,
            current_agent_path,
        }
    }

    fn list_windows(&self, args: &Value) -> Result<Value, FunctionCallError> {
        let limit = bounded_limit(args, "limit", 20);
        let recent_first = bool_arg(args, "recent_first", true);
        let mut windows = Vec::<(String, u64, usize)>::new();
        for entry in self.archive.entries().iter().filter(|entry| {
            agent_path_is_visible(
                entry.agent_path.as_deref(),
                self.current_agent_path.as_deref(),
            )
        }) {
            if let Some(window) = windows.iter_mut().find(|(id, _, _)| id == &entry.window_id) {
                window.2 += 1;
            } else {
                windows.push((entry.window_id.clone(), entry.window_number, 1));
            }
        }
        if recent_first {
            windows.reverse();
        }
        let (windows, next_cursor) =
            paginate_by_cursor(windows, args, limit, recent_first, |(window_id, _, _)| {
                window_id
            })?;
        let windows = windows
            .into_iter()
            .map(|(window_id, window_number, item_count)| {
                json!({
                    "window_id": window_id,
                    "window_number": window_number,
                    "item_count": item_count,
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({"windows":windows,"next_cursor":next_cursor}))
    }

    fn filtered(&self, args: &Value) -> Vec<&HistoryEntry> {
        let window_id = string_arg(args, "window_id");
        let role = string_arg(args, "role");
        let tool_namespace = string_arg(args, "tool_namespace");
        let tool_name = string_arg(args, "tool_name");
        self.archive
            .entries()
            .iter()
            .filter(|entry| {
                agent_path_is_visible(
                    entry.agent_path.as_deref(),
                    self.current_agent_path.as_deref(),
                )
            })
            .filter(|entry| window_id.is_none_or(|value| entry.window_id == value))
            .filter(|entry| role.is_none_or(|value| entry.role == value))
            .filter(|entry| {
                tool_namespace.is_none_or(|value| entry.tool_namespace.as_deref() == Some(value))
            })
            .filter(|entry| tool_name.is_none_or(|value| entry.tool_name.as_deref() == Some(value)))
            .collect()
    }

    fn list_items(&self, args: &Value) -> Result<Value, FunctionCallError> {
        let limit = bounded_limit(args, "limit", DEFAULT_LIST_LIMIT);
        let recent_first = bool_arg(args, "recent_first", true);
        let max_chars = bounded_usize(args, "max_chars_per_item", 800, 1, 8_000);
        let mut entries = self.filtered(args);
        if recent_first {
            entries.reverse();
        }
        let (entries, next_cursor) =
            paginate_by_cursor(entries, args, limit, recent_first, |entry| &entry.item_id)?;
        let items = entries
            .into_iter()
            .map(|entry| item_preview(entry, max_chars))
            .collect::<Vec<_>>();
        Ok(json!({"items":items,"next_cursor":next_cursor}))
    }

    fn search(&self, args: &Value) -> Result<Value, FunctionCallError> {
        let query = required_string(args, "query")?;
        let limit = bounded_limit(args, "limit", DEFAULT_LIST_LIMIT);
        let recent_first = bool_arg(args, "recent_first", true);
        let mut entries = self
            .filtered(args)
            .into_iter()
            .filter(|entry| entry.content.contains(query))
            .collect::<Vec<_>>();
        if recent_first {
            entries.reverse();
        }
        let (entries, next_cursor) =
            paginate_by_cursor(entries, args, limit, recent_first, |entry| &entry.item_id)?;
        let matches = entries
            .into_iter()
            .map(|entry| item_preview(entry, 800))
            .collect::<Vec<_>>();
        Ok(json!({"matches":matches,"next_cursor":next_cursor}))
    }

    fn read_item(&self, args: &Value) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        let window_id = required_string(args, "window_id")?;
        let item_id = required_string(args, "item_id")?;
        let entry = self
            .archive
            .get(window_id, item_id)
            .filter(|entry| {
                agent_path_is_visible(
                    entry.agent_path.as_deref(),
                    self.current_agent_path.as_deref(),
                )
            })
            .ok_or_else(|| {
                FunctionCallError::RespondToModel(
                    "history item not found in the requested window".to_string(),
                )
            })?;
        let offset = bounded_usize(args, "offset_chars", 0, 0, usize::MAX);
        let limit = bounded_usize(args, "limit_chars", DEFAULT_READ_CHARS, 1, MAX_READ_CHARS);
        let (content, total_chars, next_offset) = unicode_range(&entry.content, offset, limit);
        let result = json!({
            "agent_path":entry.agent_path,
            "turn_id":entry.turn_id,
            "window_id":entry.window_id,
            "item_id":entry.item_id,
            "offset_chars":offset,
            "content":content,
            "n_chars":total_chars,
            "next_offset_chars":next_offset,
            "media_count":entry.media.len(),
        });
        let mut output = vec![FunctionCallOutputContentItem::InputText {
            text: serde_json::to_string(&result)
                .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?,
        }];
        output.extend(entry.media.iter().map(|(image_url, detail)| {
            FunctionCallOutputContentItem::InputImage {
                image_url: image_url.clone(),
                detail: *detail,
            }
        }));
        Ok(boxed_tool_output(FunctionToolOutput::from_content(
            output,
            Some(true),
        )))
    }
}

fn unicode_range(text: &str, offset: usize, limit: usize) -> (String, usize, Option<usize>) {
    let total_chars = text.chars().count();
    let content = text.chars().skip(offset).take(limit).collect::<String>();
    let end = offset
        .saturating_add(content.chars().count())
        .min(total_chars);
    (content, total_chars, (end < total_chars).then_some(end))
}

fn item_preview(entry: &HistoryEntry, max_chars: usize) -> Value {
    let n_chars = entry.content.chars().count();
    let preview = entry.content.chars().take(max_chars).collect::<String>();
    json!({
        "agent_path":entry.agent_path,
        "turn_id":entry.turn_id,
        "window_id":entry.window_id,
        "window_number":entry.window_number,
        "item_id":entry.item_id,
        "ordinal":entry.ordinal,
        "role":entry.role,
        "item_type":entry.item_type,
        "tool":entry.tool_name.as_ref().map(|name| json!({"namespace":entry.tool_namespace,"name":name})),
        "preview":preview,
        "n_chars":n_chars,
        "has_more":n_chars > max_chars,
        "has_media":!entry.media.is_empty(),
    })
}

fn agent_path_is_visible(entry_path: Option<&str>, current_path: Option<&str>) -> bool {
    let Some(entry_path) = entry_path else {
        // Legacy items and current root-agent items have no explicit path.
        return true;
    };
    let current_path = current_path.unwrap_or(AgentPath::ROOT);
    entry_path == AgentPath::ROOT
        || entry_path == current_path
        || current_path
            .strip_prefix(entry_path)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn paginate_by_cursor<T, F>(
    items: Vec<T>,
    args: &Value,
    limit: usize,
    recent_first: bool,
    identity: F,
) -> Result<(Vec<T>, Option<String>), FunctionCallError>
where
    F: Fn(&T) -> &str,
{
    let cursor_prefix = if recent_first { "r:" } else { "f:" };
    let start = match string_arg(args, "cursor") {
        None => 0,
        Some(cursor) => {
            let Some(anchor) = cursor.strip_prefix(cursor_prefix) else {
                return Err(FunctionCallError::RespondToModel(
                    "cursor does not match the requested sort direction".to_string(),
                ));
            };
            items
                .iter()
                .position(|item| identity(item) == anchor)
                .map(|position| position.saturating_add(1))
                .ok_or_else(|| {
                    FunctionCallError::RespondToModel(
                        "cursor is not present in the requested result set".to_string(),
                    )
                })?
        }
    };
    let end = start.saturating_add(limit).min(items.len());
    let next_cursor = (end < items.len() && end > start)
        .then(|| format!("{cursor_prefix}{}", identity(&items[end - 1])));
    Ok((
        items.into_iter().skip(start).take(end - start).collect(),
        next_cursor,
    ))
}

fn notes_root(invocation: &ToolInvocation) -> PathBuf {
    let agent_scope = invocation
        .turn
        .session_source
        .get_agent_path()
        .map(|path| note_scope_component(&path.to_string()))
        .unwrap_or_else(|| "root".to_string());
    invocation
        .turn
        .config
        .codex_home
        .join("context-notes")
        .join(invocation.session.thread_id.to_string())
        .join(agent_scope)
        .to_path_buf()
}

pub(crate) fn note_scope_component(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn virtual_note_path(root: &Path, value: &str) -> Result<PathBuf, FunctionCallError> {
    let path = Path::new(value);
    if path.is_absolute() || value.is_empty() {
        return Err(FunctionCallError::RespondToModel(
            "note path must be a non-empty relative path".to_string(),
        ));
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::CurDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(FunctionCallError::RespondToModel(
            "note path contains an unsupported component".to_string(),
        ));
    }
    Ok(root.join(path))
}

fn ensure_note_path_has_no_symlink(root: &Path, path: &Path) -> Result<(), FunctionCallError> {
    let relative = path.strip_prefix(root).map_err(|_| {
        FunctionCallError::RespondToModel("note path escaped its virtual root".to_string())
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(FunctionCallError::RespondToModel(
                    "note paths cannot traverse symbolic links".to_string(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(FunctionCallError::RespondToModel(format!(
                    "failed to validate note path: {error}"
                )));
            }
        }
    }
    Ok(())
}

fn list_note_files(root: &Path, args: &Value) -> Result<Value, FunctionCallError> {
    let prefix = string_arg(args, "prefix").unwrap_or_default();
    let max_results = bounded_usize(args, "max_results", 100, 1, MAX_LIST_LIMIT);
    let recent_first = bool_arg(args, "recent_first", true);
    let mut files = if root.exists() {
        WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .filter_map(|entry| {
                let relative = entry.path().strip_prefix(root).ok()?;
                let virtual_path = relative.to_string_lossy().replace('\\', "/");
                if !virtual_path.starts_with(prefix) {
                    return None;
                }
                let metadata = entry.metadata().ok()?;
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |duration| duration.as_secs());
                Some((virtual_path, metadata.len(), modified))
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if recent_first {
        files.sort_by_key(|(_, _, modified)| Reverse(*modified));
    } else {
        files.sort_by(|a, b| a.0.cmp(&b.0));
    }
    let (files, next_cursor) =
        paginate_by_cursor(files, args, max_results, recent_first, |(path, _, _)| path)?;
    let files = files
        .into_iter()
        .map(|(path, size_bytes, modified_at)| {
            json!({"path":path,"size_bytes":size_bytes,"modified_at_unix":modified_at})
        })
        .collect::<Vec<_>>();
    Ok(json!({"files":files,"next_cursor":next_cursor}))
}

async fn read_note_file(root: &Path, args: &Value) -> Result<Value, FunctionCallError> {
    let virtual_path = required_string(args, "path")?;
    let path = virtual_note_path(root, virtual_path)?;
    ensure_note_path_has_no_symlink(root, &path)?;
    let text = tokio::fs::read_to_string(&path).await.map_err(|error| {
        FunctionCallError::RespondToModel(format!("failed to read note: {error}"))
    })?;
    let lines = text.lines().collect::<Vec<_>>();
    let start = line_index(
        args.get("start_line").and_then(Value::as_i64),
        lines.len(),
        true,
    );
    let stop = line_index(
        args.get("stop_line").and_then(Value::as_i64),
        lines.len(),
        false,
    );
    let content = if lines.is_empty() || start >= lines.len() || start > stop {
        String::new()
    } else {
        lines[start..=stop.min(lines.len().saturating_sub(1))].join("\n")
    };
    Ok(json!({
        "path":virtual_path,
        "content":content,
        "line_count":lines.len(),
        "start_line":if lines.is_empty(){0}else{start + 1},
        "stop_line":if lines.is_empty(){0}else{stop.min(lines.len().saturating_sub(1)) + 1},
    }))
}

async fn search_note_files(root: &Path, args: &Value) -> Result<Value, FunctionCallError> {
    let query = required_string(args, "query")?;
    let prefix = string_arg(args, "path_prefix").unwrap_or_default();
    let max_files = bounded_usize(args, "max_files", 20, 1, MAX_LIST_LIMIT);
    let max_matches = bounded_usize(args, "max_matches_per_file", 20, 1, 200);
    let recent_first = bool_arg(args, "recent_file_first", true);
    let listed = list_note_files(
        root,
        &json!({
            "prefix":prefix,
            "max_results":max_files,
            "recent_first":recent_first,
        }),
    )?;
    let mut files = Vec::new();
    for file in listed["files"].as_array().into_iter().flatten() {
        let Some(path_value) = file["path"].as_str() else {
            continue;
        };
        let path = virtual_note_path(root, path_value)?;
        ensure_note_path_has_no_symlink(root, &path)?;
        let Ok(text) = tokio::fs::read_to_string(path).await else {
            continue;
        };
        let matches = text
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains(query))
            .take(max_matches)
            .map(|(index, line)| json!({"line":index + 1,"text":line}))
            .collect::<Vec<_>>();
        if !matches.is_empty() {
            files.push(json!({"path":path_value,"matches":matches}));
        }
    }
    Ok(json!({"files":files}))
}

async fn write_note_file(
    root: &Path,
    args: &Value,
    append: bool,
) -> Result<Value, FunctionCallError> {
    let virtual_path = required_string(args, "path")?;
    let text = required_string(args, "text")?;
    let path = virtual_note_path(root, virtual_path)?;
    ensure_note_path_has_no_symlink(root, &path)?;
    let existing = if append {
        match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                return Err(FunctionCallError::RespondToModel(format!(
                    "failed to read note before append: {error}"
                )));
            }
        }
    } else {
        Vec::new()
    };
    let final_size = existing.len().saturating_add(text.len());
    if final_size > MAX_NOTE_BYTES {
        return Err(FunctionCallError::RespondToModel(format!(
            "note exceeds the {MAX_NOTE_BYTES}-byte limit"
        )));
    }
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            FunctionCallError::RespondToModel(format!("failed to create note directory: {error}"))
        })?;
    }
    ensure_note_path_has_no_symlink(root, &path)?;
    let mut bytes = existing;
    bytes.extend_from_slice(text.as_bytes());
    let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
    tokio::fs::write(&temporary, &bytes)
        .await
        .map_err(|error| {
            FunctionCallError::RespondToModel(format!("failed to write note: {error}"))
        })?;
    tokio::fs::rename(&temporary, &path)
        .await
        .map_err(|error| {
            FunctionCallError::RespondToModel(format!("failed to install note: {error}"))
        })?;
    Ok(json!({"path":virtual_path,"size_bytes":bytes.len(),"ok":true}))
}

fn line_index(value: Option<i64>, line_count: usize, is_start: bool) -> usize {
    let default = if is_start { 1 } else { line_count as i64 };
    let value = value.unwrap_or(default);
    if value < 0 {
        (line_count as i64 + value).max(0) as usize
    } else {
        value.saturating_sub(1) as usize
    }
}

fn required_string<'a>(args: &'a Value, name: &str) -> Result<&'a str, FunctionCallError> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| FunctionCallError::RespondToModel(format!("missing string `{name}`")))
}

fn string_arg<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
    args.get(name).and_then(Value::as_str)
}

fn bool_arg(args: &Value, name: &str, default: bool) -> bool {
    args.get(name).and_then(Value::as_bool).unwrap_or(default)
}

fn bounded_limit(args: &Value, name: &str, default: usize) -> usize {
    bounded_usize(args, name, default, 1, MAX_LIST_LIMIT)
}

fn bounded_usize(
    args: &Value,
    name: &str,
    default: usize,
    minimum: usize,
    maximum: usize,
) -> usize {
    args.get(name)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(default)
        .clamp(minimum, maximum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::models::ContentItem;
    use codex_protocol::models::FunctionCallOutputBody;
    use codex_protocol::models::FunctionCallOutputPayload;
    use codex_protocol::models::ImageDetail;
    use codex_protocol::models::TranscriptInputItem;
    use codex_protocol::models::TranscriptItem;
    use codex_protocol::protocol::CompactedItem;
    use codex_protocol::protocol::RolloutItem;
    use codex_protocol::protocol::TranscriptEnvelope;
    use codex_protocol::protocol::TranscriptIdentity;
    use tempfile::TempDir;

    fn message(text: &str) -> TranscriptItem {
        TranscriptItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: text.to_string(),
            }],
            phase: None,
        }
    }

    #[test]
    fn rollout_snapshot_preserves_envelope_identity_and_legacy_ids() {
        let ids = crate::state::AutoCompactWindowIds::new_initial();
        let next_window = Uuid::now_v7();
        let envelope = TranscriptEnvelope {
            item: message("new window"),
            identity: TranscriptIdentity {
                thread_id: "thread".to_string(),
                agent_path: None,
                window_id: next_window.to_string(),
                window_number: 1,
                turn_id: Some("turn".to_string()),
                ordinal: 1,
                item_id: "stable-item".to_string(),
            },
        };
        let rollout = vec![
            RolloutItem::TranscriptItem(message("legacy")),
            RolloutItem::Compacted(CompactedItem {
                message: String::new(),
                replacement_history: Some(Vec::new()),
                window_number: Some(1),
                first_window_id: Some(ids.first_window_id.to_string()),
                previous_window_id: Some(ids.window_id.to_string()),
                window_id: Some(next_window.to_string()),
            }),
            RolloutItem::TranscriptEnvelope(envelope),
        ];

        let first = HistoryArchive::from_rollout(&rollout, ids);
        let second = HistoryArchive::from_rollout(&rollout, ids);

        assert_eq!(first.entries().len(), 2);
        assert_eq!(first.entries()[0].item_id, second.entries()[0].item_id);
        assert_eq!(first.entries()[1].item_id, "stable-item");
        assert_eq!(first.entries()[1].window_id, next_window.to_string());
        assert_eq!(first.entries()[1].turn_id.as_deref(), Some("turn"));
    }

    #[test]
    fn legacy_identity_is_stable_across_resume_and_full_fork() {
        let rollout = vec![RolloutItem::TranscriptItem(message("legacy"))];
        let original = HistoryArchive::from_rollout(
            &rollout,
            crate::state::AutoCompactWindowIds::new_initial(),
        );
        let resumed_or_forked = HistoryArchive::from_rollout(
            &rollout,
            crate::state::AutoCompactWindowIds::new_initial(),
        );

        assert_eq!(
            original.entries()[0].item_id,
            resumed_or_forked.entries()[0].item_id
        );
        assert_eq!(
            original.entries()[0].window_id,
            resumed_or_forked.entries()[0].window_id
        );
        let (_, inferred_ids) = crate::session::context_window::infer_window_lineage(&rollout)
            .expect("legacy rollout should have window lineage");
        assert_eq!(
            original.entries()[0].window_id,
            inferred_ids.window_id.to_string()
        );
    }

    #[test]
    fn legacy_compaction_without_lineage_stays_in_the_initial_window() {
        let rollout = vec![
            RolloutItem::TranscriptItem(message("first window")),
            RolloutItem::Compacted(CompactedItem {
                message: String::new(),
                replacement_history: Some(Vec::new()),
                window_number: None,
                first_window_id: None,
                previous_window_id: None,
                window_id: None,
            }),
            RolloutItem::TranscriptItem(message("second window")),
        ];
        let archive = HistoryArchive::from_rollout(
            &rollout,
            crate::state::AutoCompactWindowIds::new_initial(),
        );
        let (window_number, runtime_ids) =
            crate::session::context_window::infer_window_lineage(&rollout)
                .expect("legacy rollout should have window lineage");

        assert_eq!(window_number, 0);
        assert_eq!(archive.entries().len(), 2);
        assert_eq!(
            archive.entries()[0].window_id,
            runtime_ids.first_window_id.to_string()
        );
        assert_eq!(
            archive.entries()[1].window_id,
            runtime_ids.window_id.to_string()
        );
        assert_eq!(runtime_ids.previous_window_id, None);
    }

    #[test]
    fn history_visibility_includes_root_and_ancestors_but_not_siblings_or_descendants() {
        assert!(agent_path_is_visible(None, None));
        assert!(agent_path_is_visible(Some("/root"), None));
        assert!(!agent_path_is_visible(Some("/root/worker"), None));

        let current = Some("/root/researcher/reader");
        assert!(agent_path_is_visible(None, current));
        assert!(agent_path_is_visible(Some("/root"), current));
        assert!(agent_path_is_visible(Some("/root/researcher"), current));
        assert!(agent_path_is_visible(
            Some("/root/researcher/reader"),
            current
        ));
        assert!(!agent_path_is_visible(Some("/root/other"), current));
        assert!(!agent_path_is_visible(
            Some("/root/researcher/reader/child"),
            current
        ));
    }

    #[test]
    fn note_scope_encoding_is_collision_free_for_nested_agent_paths() {
        assert_ne!(
            note_scope_component("/root/a_b"),
            note_scope_component("/root/a/b")
        );
    }

    #[test]
    fn cursors_page_stably_and_reject_sort_direction_changes() {
        let first = paginate_by_cursor(vec!["three", "two", "one"], &json!({}), 2, true, |value| {
            *value
        })
        .expect("first page");
        assert_eq!(first.0, vec!["three", "two"]);
        assert_eq!(first.1.as_deref(), Some("r:two"));

        let second = paginate_by_cursor(
            vec!["three", "two", "one"],
            &json!({"cursor":"r:two"}),
            2,
            true,
            |value| *value,
        )
        .expect("second page");
        assert_eq!(second.0, vec!["one"]);
        assert_eq!(second.1, None);

        assert!(
            paginate_by_cursor(
                vec!["three", "two", "one"],
                &json!({"cursor":"r:two"}),
                2,
                false,
                |value| *value,
            )
            .is_err()
        );
    }

    #[test]
    fn unicode_ranges_count_codepoints() {
        assert_eq!(
            unicode_range("A猫🙂B", 1, 2),
            ("猫🙂".to_string(), 4, Some(3))
        );
        assert_eq!(unicode_range("A猫🙂B", 3, 10), ("B".to_string(), 4, None));
    }

    #[test]
    fn history_list_hides_media_body_and_explicit_read_hydrates_it() {
        let image_url = "data:image/png;base64,AAA";
        let envelope = TranscriptEnvelope {
            item: TranscriptItem::FunctionCallOutput {
                call_id: "tool-call".to_string(),
                output: FunctionCallOutputPayload {
                    body: FunctionCallOutputBody::ContentItems(vec![
                        FunctionCallOutputContentItem::InputText {
                            text: "tool text".to_string(),
                        },
                        FunctionCallOutputContentItem::InputImage {
                            image_url: image_url.to_string(),
                            detail: Some(ImageDetail::Auto),
                        },
                    ]),
                    success: Some(true),
                },
            },
            identity: TranscriptIdentity {
                thread_id: "thread".to_string(),
                agent_path: None,
                window_id: "window".to_string(),
                window_number: 0,
                turn_id: Some("turn".to_string()),
                ordinal: 0,
                item_id: "media-item".to_string(),
            },
        };
        let archive = HistoryArchive::from_rollout(
            &[RolloutItem::TranscriptEnvelope(envelope)],
            crate::state::AutoCompactWindowIds::new_initial(),
        );
        let snapshot = HistorySnapshot::new(&archive, None);

        let listed = snapshot
            .list_items(&json!({"window_id":"window"}))
            .expect("list media item");
        assert_eq!(listed["items"][0]["has_media"], true);
        assert!(!listed.to_string().contains(image_url));

        let read = snapshot
            .read_item(&json!({"window_id":"window","item_id":"media-item"}))
            .expect("read media item")
            .to_response_item(
                "history-read",
                &ToolPayload::Function {
                    arguments: "{}".to_string(),
                },
            );
        let TranscriptInputItem::FunctionCallOutput { output, .. } = read else {
            panic!("expected function call output");
        };
        assert!(output.content_items().is_some_and(|items| {
            items.iter().any(|item| {
                matches!(
                    item,
                    FunctionCallOutputContentItem::InputImage { image_url: value, .. }
                        if value == image_url
                )
            })
        }));
    }

    #[test]
    fn history_tools_flatten_when_provider_has_no_namespace_support() {
        let ToolSpec::Function(flat) = HistoryNotesAction::HistoryListWindows.spec(false) else {
            panic!("provider-neutral history tool should be a function");
        };
        assert_eq!(flat.name, "history__list_windows");

        let ToolSpec::Namespace(namespace) = HistoryNotesAction::HistoryListWindows.spec(true)
        else {
            panic!("namespace-capable provider should receive a namespace tool");
        };
        assert_eq!(namespace.name, "history");
    }

    #[tokio::test]
    async fn notes_write_append_and_negative_range_are_immediately_visible() {
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path();
        write_note_file(
            root,
            &json!({"path":"task/checkpoint.md","text":"one\ntwo\n"}),
            false,
        )
        .await
        .expect("write");
        write_note_file(
            root,
            &json!({"path":"task/checkpoint.md","text":"three\n"}),
            true,
        )
        .await
        .expect("append");

        let read = read_note_file(
            root,
            &json!({"path":"task/checkpoint.md","start_line":-2,"stop_line":-1}),
        )
        .await
        .expect("read");
        assert_eq!(read["content"], "two\nthree");
        assert!(virtual_note_path(root, "../escape").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn notes_reject_symbolic_link_escape() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().expect("tempdir");
        let root = temp.path().join("notes");
        std::fs::create_dir_all(&root).expect("notes root");
        let outside = temp.path().join("outside.txt");
        std::fs::write(&outside, "outside").expect("outside file");
        symlink(&outside, root.join("escape.md")).expect("symlink");

        let path = virtual_note_path(&root, "escape.md").expect("virtual path");
        assert!(ensure_note_path_has_no_symlink(&root, &path).is_err());
    }
}
