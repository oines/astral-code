use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;

use crate::Prompt;
use crate::compact::InitialContextInjection;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::CompactedItem;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::Mutex;
use tracing::warn;

mod sidechain;
mod tail;

use sidechain::run_extraction;
use tail::DEFAULT_SUMMARY;
use tail::ExtractionBoundary;
use tail::count_tool_calls;
use tail::estimate_prompt_tokens;
use tail::extraction_boundary;
use tail::format_session_memory_summary;
use tail::raw_tail_after_summary_boundary;
use tail::truncate_summary_for_compact;
use tail::validate_post_compact_budget;
use tail::validate_summary;
use tail::validate_tail_budget;

const SUMMARY_FILE_NAME: &str = "summary.md";
const STATE_FILE_NAME: &str = "state.json";
const MINIMUM_MESSAGE_TOKENS_TO_INIT: i64 = 10_000;
const MINIMUM_TOKENS_BETWEEN_UPDATE: i64 = 5_000;
const TOOL_CALLS_BETWEEN_UPDATES: usize = 3;
const EXTRACTION_WAIT_TIMEOUT: Duration = Duration::from_secs(15);
const EXTRACTION_STALE_AFTER_SECS: u64 = 60;
const AUTO_COMPACT_FAILURE_BREAKER: u32 = 3;

static RUNNING_EXTRACTIONS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

#[derive(Clone, Debug)]
pub(crate) struct PromptTemplate {
    prompt: Prompt,
}

impl PromptTemplate {
    pub(crate) fn from_prompt(prompt: &Prompt) -> Self {
        let mut prompt = prompt.clone();
        prompt.input.clear();
        prompt.compact_input_placeholders = false;
        Self { prompt }
    }

    fn with_input(&self, input: Vec<ResponseItem>) -> Prompt {
        let mut prompt = self.prompt.clone();
        prompt.input = input;
        prompt
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ExtractionCandidate {
    prompt: Prompt,
    active_context_tokens: i64,
    natural_break: bool,
}

impl ExtractionCandidate {
    pub(crate) fn new(
        template: PromptTemplate,
        input: Vec<ResponseItem>,
        active_context_tokens: i64,
        natural_break: bool,
    ) -> Self {
        Self {
            prompt: template.with_input(input),
            active_context_tokens,
            natural_break,
        }
    }
}

#[derive(Debug)]
pub(crate) enum SessionMemoryCompactOutcome {
    Used { summary_suffix: String },
    Fallback { reason: String },
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SessionMemoryState {
    last_summary_index: Option<usize>,
    last_summary_fingerprint: Option<String>,
    last_summary_tokens: Option<i64>,
    last_summary_tool_calls: Option<usize>,
    extraction_started_at_unix: Option<u64>,
    last_error: Option<String>,
    consecutive_auto_compact_failures: u32,
}

#[derive(Clone, Debug)]
struct SessionMemoryStore {
    thread_key: String,
    dir: PathBuf,
    summary_path: PathBuf,
    state_path: PathBuf,
}

impl SessionMemoryStore {
    fn new(turn_context: &TurnContext, sess: &Session) -> Self {
        let thread_key = sess.thread_id().to_string();
        let dir = turn_context
            .config
            .codex_home
            .join("session-memory")
            .join(&thread_key)
            .to_path_buf();
        Self {
            thread_key,
            summary_path: dir.join(SUMMARY_FILE_NAME),
            state_path: dir.join(STATE_FILE_NAME),
            dir,
        }
    }

    async fn ensure(&self) -> CodexResult<()> {
        tokio::fs::create_dir_all(&self.dir).await?;
        if tokio::fs::metadata(&self.summary_path).await.is_err() {
            tokio::fs::write(&self.summary_path, DEFAULT_SUMMARY).await?;
        }
        if tokio::fs::metadata(&self.state_path).await.is_err() {
            self.write_state(&SessionMemoryState::default()).await?;
        }
        Ok(())
    }

    async fn read_summary(&self) -> CodexResult<String> {
        Ok(tokio::fs::read_to_string(&self.summary_path).await?)
    }

    async fn read_state(&self) -> CodexResult<SessionMemoryState> {
        match tokio::fs::read_to_string(&self.state_path).await {
            Ok(contents) => Ok(serde_json::from_str(&contents)?),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Ok(SessionMemoryState::default())
            }
            Err(err) => Err(err.into()),
        }
    }

    async fn write_state(&self, state: &SessionMemoryState) -> CodexResult<()> {
        let contents = serde_json::to_string_pretty(state)?;
        tokio::fs::write(&self.state_path, contents).await?;
        Ok(())
    }
}

pub(crate) async fn maybe_spawn_post_sampling_extraction(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    candidate: ExtractionCandidate,
) {
    if !turn_context.config.experimental_session_memory_compact {
        return;
    }

    let store = SessionMemoryStore::new(turn_context.as_ref(), sess.as_ref());
    if let Err(err) = store.ensure().await {
        warn!("failed to initialize session memory store: {err:#}");
        return;
    }

    let state = match store.read_state().await {
        Ok(state) => state,
        Err(err) => {
            warn!("failed to read session memory state: {err:#}");
            return;
        }
    };
    if !should_extract(&state, &candidate) {
        return;
    }

    let Some(mut boundary) = extraction_boundary(&candidate.prompt.input) else {
        return;
    };
    boundary.tokens = candidate
        .active_context_tokens
        .max(estimate_prompt_tokens(&candidate.prompt));

    {
        let mut running = RUNNING_EXTRACTIONS.lock().await;
        if !running.insert(store.thread_key.clone()) {
            return;
        }
    }

    if let Err(err) = mark_extraction_started(&store, state).await {
        warn!("failed to mark session memory extraction started: {err:#}");
        clear_running_extraction(&store.thread_key).await;
        return;
    }

    let handle = sess.services.runtime_handle.clone();
    handle.spawn(async move {
        let result = run_extraction(
            Arc::clone(&sess),
            Arc::clone(&turn_context),
            store.clone(),
            candidate,
            boundary,
        )
        .await;
        if let Err(err) = finish_extraction(&store, result).await {
            warn!("failed to finish session memory extraction: {err:#}");
        }
        clear_running_extraction(&store.thread_key).await;
    });
}

pub(crate) async fn try_compact(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    initial_context_injection: InitialContextInjection,
    is_auto_compact: bool,
    compaction_item: &codex_protocol::items::TurnItem,
) -> CodexResult<SessionMemoryCompactOutcome> {
    if !turn_context.config.experimental_session_memory_compact {
        return Ok(SessionMemoryCompactOutcome::Fallback {
            reason: "disabled".to_string(),
        });
    }

    let store = SessionMemoryStore::new(turn_context.as_ref(), sess.as_ref());
    store.ensure().await?;
    let mut state = store.read_state().await?;
    if is_auto_compact && state.consecutive_auto_compact_failures >= AUTO_COMPACT_FAILURE_BREAKER {
        return Ok(SessionMemoryCompactOutcome::Fallback {
            reason: "session memory compact breaker open".to_string(),
        });
    }

    let result = try_compact_inner(
        Arc::clone(&sess),
        Arc::clone(&turn_context),
        &store,
        initial_context_injection,
        compaction_item,
        &mut state,
    )
    .await;

    match result {
        Ok(summary_suffix) => {
            state.consecutive_auto_compact_failures = 0;
            state.last_error = None;
            store.write_state(&state).await?;
            Ok(SessionMemoryCompactOutcome::Used { summary_suffix })
        }
        Err(err) => {
            if is_auto_compact {
                state.consecutive_auto_compact_failures =
                    state.consecutive_auto_compact_failures.saturating_add(1);
            }
            state.last_error = Some(err.to_string());
            store.write_state(&state).await?;
            Ok(SessionMemoryCompactOutcome::Fallback {
                reason: err.to_string(),
            })
        }
    }
}

async fn try_compact_inner(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    store: &SessionMemoryStore,
    initial_context_injection: InitialContextInjection,
    compaction_item: &codex_protocol::items::TurnItem,
    state: &mut SessionMemoryState,
) -> CodexResult<String> {
    wait_for_running_extraction(store, state).await?;

    let summary = store.read_summary().await?;
    validate_summary(&summary)?;
    let history_snapshot = sess.clone_history().await;
    let history_items = history_snapshot.raw_items();
    let tail = raw_tail_after_summary_boundary(history_items, state)?;
    validate_tail_budget(&tail)?;

    let (summary_for_compact, was_truncated_for_compact) = truncate_summary_for_compact(&summary);
    let summary_text =
        format_session_memory_summary(&summary_for_compact, was_truncated_for_compact);
    let mut new_history = vec![ResponseItem::Compaction {
        encrypted_content: summary_text.clone(),
    }];
    new_history.extend(tail);

    if matches!(
        initial_context_injection,
        InitialContextInjection::BeforeLastUserMessage
    ) {
        let initial_context = sess.build_initial_context(turn_context.as_ref()).await;
        new_history = crate::compact::insert_initial_context_before_last_real_user_or_summary(
            new_history,
            initial_context,
        );
    }

    validate_post_compact_budget(&new_history)?;

    let reference_context_item = match initial_context_injection {
        InitialContextInjection::DoNotInject => None,
        InitialContextInjection::BeforeLastUserMessage => Some(turn_context.to_turn_context_item()),
    };
    let compacted_item = CompactedItem {
        message: summary_text.clone(),
        replacement_history: Some(new_history.clone()),
    };
    sess.replace_compacted_history(new_history, reference_context_item, compacted_item)
        .await;
    sess.recompute_token_usage(turn_context.as_ref()).await;
    sess.emit_turn_item_completed(turn_context.as_ref(), compaction_item.clone())
        .await;

    Ok(summary_text)
}

fn should_extract(state: &SessionMemoryState, candidate: &ExtractionCandidate) -> bool {
    let total_tokens = candidate
        .active_context_tokens
        .max(estimate_prompt_tokens(&candidate.prompt));
    let tool_calls = count_tool_calls(&candidate.prompt.input);

    let Some(last_tokens) = state.last_summary_tokens else {
        return total_tokens >= MINIMUM_MESSAGE_TOKENS_TO_INIT;
    };
    if total_tokens <= last_tokens {
        return false;
    }

    let token_delta = total_tokens.saturating_sub(last_tokens);
    if token_delta >= MINIMUM_TOKENS_BETWEEN_UPDATE {
        return true;
    }

    let last_tool_calls = state.last_summary_tool_calls.unwrap_or_default();
    if tool_calls.saturating_sub(last_tool_calls) >= TOOL_CALLS_BETWEEN_UPDATES {
        return true;
    }

    candidate.natural_break
}

async fn mark_extraction_started(
    store: &SessionMemoryStore,
    mut state: SessionMemoryState,
) -> CodexResult<()> {
    state.extraction_started_at_unix = Some(now_unix_seconds());
    state.last_error = None;
    store.write_state(&state).await
}

async fn finish_extraction(
    store: &SessionMemoryStore,
    result: CodexResult<ExtractionBoundary>,
) -> CodexResult<()> {
    let mut state = store.read_state().await.unwrap_or_default();
    state.extraction_started_at_unix = None;
    match result {
        Ok(boundary) => {
            state.last_summary_index = Some(boundary.index);
            state.last_summary_fingerprint = Some(boundary.fingerprint);
            state.last_summary_tokens = Some(boundary.tokens);
            state.last_summary_tool_calls = Some(boundary.tool_calls);
            state.last_error = None;
        }
        Err(err) => {
            state.last_error = Some(err.to_string());
        }
    }
    store.write_state(&state).await
}

async fn clear_running_extraction(thread_key: &str) {
    let mut running = RUNNING_EXTRACTIONS.lock().await;
    running.remove(thread_key);
}

async fn wait_for_running_extraction(
    store: &SessionMemoryStore,
    state: &mut SessionMemoryState,
) -> CodexResult<()> {
    let Some(started_at) = state.extraction_started_at_unix else {
        return Ok(());
    };
    if now_unix_seconds().saturating_sub(started_at) > EXTRACTION_STALE_AFTER_SECS {
        state.extraction_started_at_unix = None;
        store.write_state(state).await?;
        return Err(CodexErr::Fatal(
            "session memory extraction is stale".to_string(),
        ));
    }

    tokio::time::sleep(EXTRACTION_WAIT_TIMEOUT).await;
    let refreshed = store.read_state().await?;
    if refreshed.extraction_started_at_unix.is_some() {
        return Err(CodexErr::Fatal(
            "session memory extraction did not finish before compact timeout".to_string(),
        ));
    }
    *state = refreshed;
    Ok(())
}

fn now_unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "session_memory_tests.rs"]
mod tests;
