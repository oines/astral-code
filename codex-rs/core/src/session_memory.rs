use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;

use crate::Prompt;
use crate::compact::InitialContextInjection;
use crate::config::Config;
use crate::context_manager::ContextManager;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_protocol::config_types::AutoCompactTokenLimitScope;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::models::TranscriptItem;
use codex_protocol::openai_models::InputModality;
use codex_protocol::protocol::CompactedItem;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::Mutex;
use tracing::warn;

mod sidechain;
mod tail;

use sidechain::run_extraction;
pub(crate) use tail::DEFAULT_SUMMARY;
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
pub(crate) const DEFAULT_MINIMUM_MESSAGE_TOKENS_TO_INIT: i64 = 100_000;
pub(crate) const DEFAULT_MINIMUM_TOKENS_BETWEEN_UPDATE: i64 = 20_000;
pub(crate) const DEFAULT_TOOL_CALLS_BETWEEN_UPDATES: usize = 10;
const EXTRACTION_WAIT_TIMEOUT: Duration = Duration::from_secs(15);
const EXTRACTION_SHUTDOWN_WAIT_TIMEOUT: Duration = Duration::from_secs(20);
const EXTRACTION_STALE_AFTER_SECS: u64 = 60;

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

    fn with_input(&self, input: Vec<TranscriptItem>) -> Prompt {
        let mut prompt = self.prompt.clone();
        prompt.input = input;
        prompt
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ExtractionCandidate {
    prompt: Prompt,
    raw_boundary: Option<ExtractionBoundary>,
    active_context_tokens: i64,
    natural_break: bool,
}

impl ExtractionCandidate {
    pub(crate) fn from_history(
        template: PromptTemplate,
        history: ContextManager,
        input_modalities: &[InputModality],
        active_context_tokens: i64,
        natural_break: bool,
    ) -> Self {
        let raw_boundary = extraction_boundary(history.raw_items());
        let input = history.for_prompt(input_modalities);
        Self {
            prompt: template.with_input(input),
            raw_boundary,
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

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct SessionMemoryState {
    last_summary_index: Option<usize>,
    last_summary_fingerprint: Option<String>,
    last_summary_tokens: Option<i64>,
    last_summary_tool_calls: Option<usize>,
    extraction_started_at_unix: Option<u64>,
    last_error: Option<String>,
}

impl SessionMemoryState {
    fn clear_summary_boundary(&mut self) {
        self.last_summary_index = None;
        self.last_summary_fingerprint = None;
    }

    fn record_post_compact_baseline(&mut self, tokens: i64, tool_calls: usize) {
        self.clear_summary_boundary();
        self.last_summary_tokens = Some(tokens);
        self.last_summary_tool_calls = Some(tool_calls);
    }
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
        Self::for_thread(&turn_context.config, sess.thread_id().to_string())
    }

    fn for_thread(config: &Config, thread_key: String) -> Self {
        let dir = config
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

    async fn ensure(&self, template: &str) -> CodexResult<()> {
        tokio::fs::create_dir_all(&self.dir).await?;
        if tokio::fs::metadata(&self.summary_path).await.is_err() {
            atomic_write(&self.summary_path, template.as_bytes().to_vec()).await?;
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
        atomic_write(&self.state_path, contents.into_bytes()).await?;
        Ok(())
    }
}

async fn atomic_write(path: &Path, contents: Vec<u8>) -> CodexResult<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        let parent = path.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("path has no parent: {}", path.display()),
            )
        })?;
        let mut temp_file = tempfile::NamedTempFile::new_in(parent)?;
        temp_file.write_all(&contents)?;
        temp_file.as_file().sync_all()?;
        temp_file.persist(&path).map_err(|err| err.error)?;
        Ok(())
    })
    .await??;
    Ok(())
}

pub(crate) async fn wait_for_pending_extraction_on_shutdown(sess: &Arc<Session>) {
    let config = sess.get_config().await;
    if !config.experimental_session_memory_compact {
        return;
    }

    let store = SessionMemoryStore::for_thread(config.as_ref(), sess.thread_id().to_string());
    if let Err(err) = wait_for_extraction_completion(&store, EXTRACTION_SHUTDOWN_WAIT_TIMEOUT).await
    {
        warn!("failed to wait for session memory extraction during shutdown: {err:#}");
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
    if let Err(err) = store.ensure(turn_context.session_memory_template()).await {
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
    if !should_extract(
        &state,
        &candidate,
        ExtractionThresholds::from_config(&turn_context.config),
    ) {
        return;
    }

    let Some(mut boundary) = candidate.raw_boundary.clone() else {
        return;
    };
    boundary.tokens = candidate
        .active_context_tokens
        .max(estimate_prompt_tokens(&candidate.prompt));
    boundary.tool_calls = count_tool_calls(&candidate.prompt.input);

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
    initial_context_injection: &InitialContextInjection,
    _is_auto_compact: bool,
    compaction_item: &codex_protocol::items::TurnItem,
) -> CodexResult<SessionMemoryCompactOutcome> {
    if !turn_context.config.experimental_session_memory_compact {
        return Ok(SessionMemoryCompactOutcome::Fallback {
            reason: "disabled".to_string(),
        });
    }

    let store = SessionMemoryStore::new(turn_context.as_ref(), sess.as_ref());
    store.ensure(turn_context.session_memory_template()).await?;
    let mut state = store.read_state().await?;

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
            state.last_error = None;
            store.write_state(&state).await?;
            Ok(SessionMemoryCompactOutcome::Used { summary_suffix })
        }
        Err(err) => {
            warn!("session memory compact failed; falling back to legacy compact: {err:#}");
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
    initial_context_injection: &InitialContextInjection,
    compaction_item: &codex_protocol::items::TurnItem,
    state: &mut SessionMemoryState,
) -> CodexResult<String> {
    wait_for_running_extraction(store, state).await?;

    let summary = store.read_summary().await?;
    validate_summary(&summary, turn_context.session_memory_template())?;
    let history_snapshot = sess.clone_history().await;
    let history_items = history_snapshot.raw_items();
    let tail = raw_tail_after_summary_boundary(history_items, state)?;
    validate_tail_budget(&tail)?;

    let (summary_for_compact, was_truncated_for_compact) = truncate_summary_for_compact(&summary);
    let transcript_path = sess.current_rollout_path().await.ok().flatten();
    let summary_text = format_session_memory_summary(
        &summary_for_compact,
        was_truncated_for_compact,
        transcript_path.as_deref(),
        &store.summary_path,
    );
    let mut new_history = build_session_memory_compacted_history(tail, summary_text.clone());
    let (initial_context, world_state_baseline) = crate::compact::build_compaction_initial_context(
        sess.as_ref(),
        turn_context.as_ref(),
        initial_context_injection,
    )
    .await;
    if !initial_context.is_empty() {
        new_history = crate::compact::insert_initial_context_before_last_real_user_or_summary(
            new_history,
            initial_context,
        );
    }
    let reference_context_item = match initial_context_injection {
        InitialContextInjection::DoNotInject => None,
        InitialContextInjection::BeforeLastUserMessage(_) => {
            Some(turn_context.to_turn_context_item())
        }
    };

    validate_post_compact_budget(
        &new_history,
        post_compact_token_limit(turn_context.as_ref()),
    )?;
    let post_compact_baseline_tool_calls = count_tool_calls(&new_history);

    let compacted_item = CompactedItem {
        message: summary_text.clone(),
        replacement_history: Some(new_history.clone()),
    };
    sess.replace_compacted_history(
        new_history,
        reference_context_item,
        world_state_baseline,
        compacted_item,
    )
    .await;
    sess.recompute_token_usage(turn_context.as_ref()).await;
    let post_compact_baseline_tokens = sess.get_total_token_usage().await;
    sess.emit_turn_item_completed(turn_context.as_ref(), compaction_item.clone())
        .await;
    state.record_post_compact_baseline(
        post_compact_baseline_tokens,
        post_compact_baseline_tool_calls,
    );

    Ok(summary_text)
}

pub(crate) async fn record_post_legacy_compact_baseline(
    sess: &Session,
    turn_context: &TurnContext,
    post_compact_history: &[TranscriptItem],
) {
    if !turn_context.config.experimental_session_memory_compact {
        return;
    }

    let store = SessionMemoryStore::new(turn_context, sess);
    if let Err(err) = record_post_legacy_compact_baseline_for_store(
        sess,
        turn_context,
        &store,
        post_compact_history,
    )
    .await
    {
        warn!("failed to record session memory post-legacy-compact baseline: {err:#}");
    }
}

async fn record_post_legacy_compact_baseline_for_store(
    sess: &Session,
    turn_context: &TurnContext,
    store: &SessionMemoryStore,
    post_compact_history: &[TranscriptItem],
) -> CodexResult<()> {
    store.ensure(turn_context.session_memory_template()).await?;
    let mut state = store.read_state().await?;
    state.record_post_compact_baseline(
        sess.get_total_token_usage().await,
        count_tool_calls(post_compact_history),
    );
    state.last_error = None;
    store.write_state(&state).await
}

fn build_session_memory_compacted_history(
    tail: Vec<TranscriptItem>,
    summary_text: String,
) -> Vec<TranscriptItem> {
    let mut history = vec![TranscriptItem::Compaction {
        encrypted_content: summary_text,
    }];
    history.extend(tail);
    history
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExtractionThresholds {
    minimum_message_tokens_to_init: i64,
    minimum_tokens_between_update: i64,
    tool_calls_between_updates: usize,
}

impl ExtractionThresholds {
    fn from_config(config: &Config) -> Self {
        Self {
            minimum_message_tokens_to_init: config.session_memory_minimum_message_tokens_to_init,
            minimum_tokens_between_update: config.session_memory_minimum_tokens_between_update,
            tool_calls_between_updates: config.session_memory_tool_calls_between_updates,
        }
    }
}

fn should_extract(
    state: &SessionMemoryState,
    candidate: &ExtractionCandidate,
    thresholds: ExtractionThresholds,
) -> bool {
    let total_tokens = candidate
        .active_context_tokens
        .max(estimate_prompt_tokens(&candidate.prompt));
    let tool_calls = count_tool_calls(&candidate.prompt.input);

    let Some(last_tokens) = state.last_summary_tokens else {
        if total_tokens < thresholds.minimum_message_tokens_to_init {
            return false;
        }
        return candidate.natural_break || tool_calls >= thresholds.tool_calls_between_updates;
    };

    let token_delta = total_tokens.saturating_sub(last_tokens);
    if token_delta < thresholds.minimum_tokens_between_update {
        return false;
    }

    let last_tool_calls = state.last_summary_tool_calls.unwrap_or_default();
    tool_calls.saturating_sub(last_tool_calls) >= thresholds.tool_calls_between_updates
        || candidate.natural_break
}

fn post_compact_token_limit(turn_context: &TurnContext) -> i64 {
    let auto_compact_limit = turn_context
        .config
        .model_auto_compact_token_limit
        .or_else(|| turn_context.model_info.auto_compact_token_limit())
        .unwrap_or(i64::MAX);
    let scoped_limit = match turn_context.config.model_auto_compact_token_limit_scope {
        AutoCompactTokenLimitScope::Total | AutoCompactTokenLimitScope::BodyAfterPrefix => {
            auto_compact_limit
        }
    };
    turn_context
        .model_context_window()
        .map_or(scoped_limit, |context_window| {
            scoped_limit.min(context_window)
        })
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
    wait_for_running_extraction_with_timeout(store, state, EXTRACTION_WAIT_TIMEOUT).await
}

async fn wait_for_running_extraction_with_timeout(
    store: &SessionMemoryStore,
    state: &mut SessionMemoryState,
    timeout: Duration,
) -> CodexResult<()> {
    let Some(started_at) = state.extraction_started_at_unix else {
        return Ok(());
    };
    if now_unix_seconds().saturating_sub(started_at) > EXTRACTION_STALE_AFTER_SECS {
        state.extraction_started_at_unix = None;
        state.last_error = Some("session memory extraction was stale before compact".to_string());
        store.write_state(state).await?;
        warn!("session memory extraction was stale before compact; continuing compact");
        return Ok(());
    }

    match poll_for_extraction_completion(store, timeout).await? {
        true => {
            *state = store.read_state().await?;
        }
        false => {
            state.extraction_started_at_unix = None;
            state.last_error =
                Some("session memory extraction did not finish before compact timeout".to_string());
            store.write_state(state).await?;
            warn!(
                "session memory extraction did not finish before compact timeout; continuing compact"
            );
        }
    }
    Ok(())
}

async fn wait_for_extraction_completion(
    store: &SessionMemoryStore,
    timeout: Duration,
) -> CodexResult<()> {
    if poll_for_extraction_completion(store, timeout).await? {
        return Ok(());
    }

    let mut state = store.read_state().await.unwrap_or_default();
    if state.extraction_started_at_unix.is_some() {
        state.extraction_started_at_unix = None;
        state.last_error =
            Some("session memory extraction interrupted during shutdown".to_string());
        store.write_state(&state).await?;
    }
    Err(CodexErr::Fatal(
        "session memory extraction did not finish before shutdown timeout".to_string(),
    ))
}

async fn poll_for_extraction_completion(
    store: &SessionMemoryStore,
    timeout: Duration,
) -> CodexResult<bool> {
    let result = tokio::time::timeout(timeout, async {
        loop {
            let state = store.read_state().await?;
            if state.extraction_started_at_unix.is_none() {
                return Ok(true);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;

    match result {
        Ok(result) => result,
        Err(_) => Ok(false),
    }
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
