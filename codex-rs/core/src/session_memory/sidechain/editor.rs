use std::path::Path;
use std::path::PathBuf;

use codex_apply_patch::ApplyPatchFileChange;
use codex_apply_patch::MaybeApplyPatchVerified;
use codex_exec_server::LOCAL_FS;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path::paths_match_after_normalization;
use codex_utils_path_uri::PathUri;
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio::sync::Semaphore;

use super::SummaryUpdateTool;
use crate::session::turn_context::TurnContext;
use crate::session_memory::atomic_write;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::apply_patch_payload::apply_patch_input_from_payload;

#[derive(Deserialize)]
struct EditArgs {
    file_path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

#[derive(Debug)]
struct SummaryEditState {
    content: String,
    revision: u64,
}

pub(super) struct SummaryEditor {
    state: Mutex<SummaryEditState>,
    edit_lock: Semaphore,
}

impl SummaryEditor {
    pub(super) fn new(content: String) -> Self {
        Self {
            state: Mutex::new(SummaryEditState {
                content,
                revision: 0,
            }),
            edit_lock: Semaphore::new(/*permits*/ 1),
        }
    }

    pub(super) async fn content(&self) -> String {
        self.state.lock().await.content.clone()
    }

    async fn update(&self, content: String) {
        let mut state = self.state.lock().await;
        state.content = content;
        state.revision = state.revision.saturating_add(1);
    }

    pub(super) async fn revision(&self) -> u64 {
        self.state.lock().await.revision
    }
}

pub(super) async fn apply_summary_edit(
    turn_context: &TurnContext,
    summary_path: &Path,
    arguments: &str,
    editor: &SummaryEditor,
) -> SummaryEditResult {
    let Ok(_permit) = editor.edit_lock.acquire().await else {
        return SummaryEditResult {
            text: "Session-memory summary editor is unavailable.".to_string(),
            edited_summary: false,
        };
    };
    let args: EditArgs = match serde_json::from_str(arguments) {
        Ok(args) => args,
        Err(err) => {
            return SummaryEditResult {
                text: format!("Invalid Edit arguments: {err}"),
                edited_summary: false,
            };
        }
    };

    let target_path = resolve_edit_path(turn_context, &args.file_path);
    if !paths_match_after_normalization(&target_path, summary_path) {
        return SummaryEditResult {
            text: deny_tool_message(SummaryUpdateTool::Edit, summary_path),
            edited_summary: false,
        };
    }
    let current = editor.content().await;
    if args.old_string.is_empty() {
        if !current.trim().is_empty() {
            return SummaryEditResult {
                text: "Cannot create new file - file already exists.".to_string(),
                edited_summary: false,
            };
        }
        return match commit_summary_update(
            summary_path,
            args.new_string,
            &current,
            editor,
            SummaryUpdateTool::Edit,
        )
        .await
        {
            Ok(()) => SummaryEditResult {
                text: format!(
                    "The file {} has been updated successfully.",
                    summary_path.display()
                ),
                edited_summary: true,
            },
            Err(text) => SummaryEditResult {
                text,
                edited_summary: false,
            },
        };
    }

    let occurrences = current.matches(&args.old_string).count();
    if occurrences == 0 {
        return SummaryEditResult {
            text: format!(
                "String to replace not found in file.\nString: {}",
                args.old_string
            ),
            edited_summary: false,
        };
    }
    if occurrences > 1 && !args.replace_all {
        return SummaryEditResult {
            text: format!(
                "Found {occurrences} matches of the string to replace, but replace_all is false. To replace all occurrences, set replace_all to true. To replace only one occurrence, please provide more context to uniquely identify the instance.\nString: {}",
                args.old_string
            ),
            edited_summary: false,
        };
    }

    let updated = if args.replace_all {
        current.replace(&args.old_string, &args.new_string)
    } else {
        current.replacen(&args.old_string, &args.new_string, 1)
    };
    match commit_summary_update(
        summary_path,
        updated,
        &current,
        editor,
        SummaryUpdateTool::Edit,
    )
    .await
    {
        Ok(()) if args.replace_all => SummaryEditResult {
            text: format!(
                "The file {} has been updated. All occurrences were successfully replaced.",
                summary_path.display()
            ),
            edited_summary: true,
        },
        Ok(()) => SummaryEditResult {
            text: format!(
                "The file {} has been updated successfully.",
                summary_path.display()
            ),
            edited_summary: true,
        },
        Err(text) => SummaryEditResult {
            text,
            edited_summary: false,
        },
    }
}

pub(super) async fn apply_summary_patch(
    summary_path: &Path,
    payload: &ToolPayload,
    editor: &SummaryEditor,
) -> SummaryEditResult {
    let Ok(_permit) = editor.edit_lock.acquire().await else {
        return SummaryEditResult {
            text: "Session-memory summary editor is unavailable.".to_string(),
            edited_summary: false,
        };
    };
    let current = editor.content().await;
    let patch_input = match apply_patch_input_from_payload(payload) {
        Ok(input) => input,
        Err(err) => {
            return SummaryEditResult {
                text: err.to_string(),
                edited_summary: false,
            };
        }
    };
    let args = match codex_apply_patch::parse_patch(&patch_input) {
        Ok(args) => args,
        Err(err) => {
            return SummaryEditResult {
                text: format!("Invalid patch: {err}"),
                edited_summary: false,
            };
        }
    };
    if args.environment_id.is_some() {
        return SummaryEditResult {
            text: "Session-memory apply_patch does not accept an environment_id.".to_string(),
            edited_summary: false,
        };
    }
    let summary_path = match AbsolutePathBuf::from_absolute_path(summary_path) {
        Ok(path) => path,
        Err(err) => {
            return SummaryEditResult {
                text: format!("Session-memory summary path is not absolute: {err}"),
                edited_summary: false,
            };
        }
    };
    let summary_uri = PathUri::from_abs_path(&summary_path);
    let Some(summary_parent) = summary_uri.parent() else {
        return SummaryEditResult {
            text: "Session-memory summary path has no parent directory.".to_string(),
            edited_summary: false,
        };
    };
    let action = match codex_apply_patch::verify_apply_patch_args(
        args,
        &summary_parent,
        LOCAL_FS.as_ref(),
        /*sandbox*/ None,
    )
    .await
    {
        MaybeApplyPatchVerified::Body(action) => action,
        MaybeApplyPatchVerified::CorrectnessError(err) => {
            return SummaryEditResult {
                text: err.to_string(),
                edited_summary: false,
            };
        }
        MaybeApplyPatchVerified::ShellParseError(err) => {
            return SummaryEditResult {
                text: format!("Invalid apply_patch shell input: {err:?}"),
                edited_summary: false,
            };
        }
        MaybeApplyPatchVerified::NotApplyPatch => {
            return SummaryEditResult {
                text: "Invalid apply_patch invocation.".to_string(),
                edited_summary: false,
            };
        }
    };
    if action.changes().len() != 1 {
        return SummaryEditResult {
            text: deny_tool_message(SummaryUpdateTool::ApplyPatch, summary_path.as_path()),
            edited_summary: false,
        };
    }
    let Some(change) = action.changes().get(&summary_uri) else {
        return SummaryEditResult {
            text: deny_tool_message(SummaryUpdateTool::ApplyPatch, summary_path.as_path()),
            edited_summary: false,
        };
    };
    let updated = match change {
        ApplyPatchFileChange::Update {
            move_path: None,
            new_content,
            ..
        } => new_content.clone(),
        ApplyPatchFileChange::Add { .. }
        | ApplyPatchFileChange::Delete { .. }
        | ApplyPatchFileChange::Update {
            move_path: Some(_), ..
        } => {
            return SummaryEditResult {
                text: "Session-memory apply_patch only permits updating summary.md in place."
                    .to_string(),
                edited_summary: false,
            };
        }
    };
    match commit_summary_update(
        summary_path.as_path(),
        updated,
        &current,
        editor,
        SummaryUpdateTool::ApplyPatch,
    )
    .await
    {
        Ok(()) => SummaryEditResult {
            text: format!(
                "Success. Updated the following files:\n{}",
                summary_path.display()
            ),
            edited_summary: true,
        },
        Err(text) => SummaryEditResult {
            text,
            edited_summary: false,
        },
    }
}

async fn commit_summary_update(
    summary_path: &Path,
    updated: String,
    expected_content: &str,
    editor: &SummaryEditor,
    update_tool: SummaryUpdateTool,
) -> Result<(), String> {
    if updated == expected_content {
        return Err(format!(
            "No changes were made. Session-memory extraction is not complete. Make a substantive {} update to summary.md, then stop.",
            update_tool.name()
        ));
    }
    let current_file_matches_read_state = match tokio::fs::read_to_string(summary_path).await {
        Ok(current) => current == expected_content,
        Err(_) => expected_content.is_empty(),
    };
    if !current_file_matches_read_state {
        return Err(
            "File has been modified since it was pre-read for session-memory extraction. Retry extraction against the latest summary.md."
                .to_string(),
        );
    }
    atomic_write(summary_path, updated.clone().into_bytes())
        .await
        .map_err(|err| format!("Failed to update file: {err}"))?;
    editor.update(updated).await;
    Ok(())
}

pub(super) struct SummaryEditResult {
    pub(super) text: String,
    pub(super) edited_summary: bool,
}

fn resolve_edit_path(turn_context: &TurnContext, file_path: &str) -> PathBuf {
    let path = PathBuf::from(file_path);
    if path.is_absolute() {
        path
    } else {
        turn_context
            .environments
            .single_local_environment_cwd()
            .unwrap_or_else(|| turn_context.config.cwd.clone())
            .join(path)
            .to_path_buf()
    }
}

pub(super) fn deny_tool_message(update_tool: SummaryUpdateTool, summary_path: &Path) -> String {
    format!(
        "only {} on {} is allowed",
        update_tool.name(),
        summary_path.display()
    )
}
