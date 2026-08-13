use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;

use codex_protocol::error::CodexErr;
use tokio::sync::Mutex;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use super::ExtractionCandidate;
use super::PromptTemplate;
use super::SessionMemoryStore;
use super::atomic_write;
use super::finish_extraction;
use super::mark_extraction_started;
use super::sidechain::run_extraction;
use super::tail::ExtractionBoundary;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;

static EXTRACTION_COORDINATORS: LazyLock<Mutex<HashMap<String, Arc<ExtractionCoordinator>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ExtractionKind {
    Routine,
    FinalRefresh,
}

pub(super) struct ExtractionWork {
    pub(super) kind: ExtractionKind,
    pub(super) sess: Arc<Session>,
    pub(super) turn_context: Arc<TurnContext>,
    pub(super) store: SessionMemoryStore,
    pub(super) candidate: ExtractionCandidate,
    pub(super) boundary: ExtractionBoundary,
}

#[derive(Clone, Debug)]
struct RunningExtraction {
    kind: ExtractionKind,
    generation: u64,
    cancellation_token: CancellationToken,
}

#[derive(Default)]
struct ExtractionCoordinatorState {
    generation: u64,
    running: Option<RunningExtraction>,
    pending_final: Option<ExtractionWork>,
    latest_template: Option<PromptTemplate>,
    final_refresh_generation: Option<u64>,
}

pub(super) struct ExtractionCoordinator {
    state: Mutex<ExtractionCoordinatorState>,
    changed: Notify,
}

impl ExtractionCoordinator {
    fn new() -> Self {
        Self {
            state: Mutex::new(ExtractionCoordinatorState::default()),
            changed: Notify::new(),
        }
    }
}

pub(super) async fn for_thread(thread_key: &str) -> Arc<ExtractionCoordinator> {
    let mut coordinators = EXTRACTION_COORDINATORS.lock().await;
    Arc::clone(
        coordinators
            .entry(thread_key.to_string())
            .or_insert_with(|| Arc::new(ExtractionCoordinator::new())),
    )
}

pub(super) async fn remove_thread(thread_key: &str) {
    EXTRACTION_COORDINATORS.lock().await.remove(thread_key);
}

pub(super) async fn remember_template(
    coordinator: &ExtractionCoordinator,
    template: PromptTemplate,
) {
    coordinator.state.lock().await.latest_template = Some(template);
}

pub(super) async fn latest_template(coordinator: &ExtractionCoordinator) -> Option<PromptTemplate> {
    coordinator.state.lock().await.latest_template.clone()
}

pub(super) async fn has_active_extraction(coordinator: &ExtractionCoordinator) -> bool {
    let state = coordinator.state.lock().await;
    state.running.is_some() || state.pending_final.is_some()
}

pub(super) async fn final_refresh_completed(coordinator: &ExtractionCoordinator) -> bool {
    let state = coordinator.state.lock().await;
    state.final_refresh_generation == Some(state.generation)
}

pub(super) async fn submit_background(
    coordinator: Arc<ExtractionCoordinator>,
    work: ExtractionWork,
) {
    let Some(work) = reserve_or_queue(&coordinator, work).await else {
        return;
    };
    let handle = work.sess.services.runtime_handle.clone();
    handle.spawn(run_queue(coordinator, work));
}

async fn reserve_or_queue(
    coordinator: &ExtractionCoordinator,
    work: ExtractionWork,
) -> Option<ExtractionWork> {
    let mut state = coordinator.state.lock().await;
    if let Some(running) = &state.running {
        if work.kind == ExtractionKind::FinalRefresh && running.kind == ExtractionKind::Routine {
            state.pending_final = Some(work);
        }
        return None;
    }

    state.running = Some(RunningExtraction {
        kind: work.kind,
        generation: state.generation,
        cancellation_token: CancellationToken::new(),
    });
    Some(work)
}

async fn run_queue(coordinator: Arc<ExtractionCoordinator>, mut work: ExtractionWork) {
    loop {
        let (generation, cancellation_token) = {
            let state = coordinator.state.lock().await;
            let Some(running) = state.running.as_ref() else {
                warn!("reserved session memory extraction was no longer running");
                return;
            };
            (running.generation, running.cancellation_token.clone())
        };
        let original_summary = work.store.read_summary().await.ok();
        let original_state = work.store.read_state().await.ok();
        let result = match mark_extraction_started(&work.store).await {
            Ok(()) => match original_summary.clone() {
                Some(current_summary) => tokio::select! {
                    result = run_extraction(
                    Arc::clone(&work.sess),
                    Arc::clone(&work.turn_context),
                    work.store.clone(),
                    work.candidate,
                    work.boundary,
                    current_summary,
                ) => result,
                    () = cancellation_token.cancelled() => Err(CodexErr::Interrupted),
                },
                None => Err(CodexErr::Fatal(
                    "session memory summary could not be read before extraction".to_string(),
                )),
            },
            Err(err) => Err(err),
        };

        let next = finish_work(
            &coordinator,
            &work.store,
            generation,
            result,
            original_summary,
            original_state,
        )
        .await;
        coordinator.changed.notify_waiters();
        let Some(next) = next else {
            break;
        };
        work = next;
    }
}

async fn finish_work(
    coordinator: &ExtractionCoordinator,
    store: &SessionMemoryStore,
    generation: u64,
    result: Result<ExtractionBoundary, CodexErr>,
    original_summary: Option<String>,
    original_state: Option<super::SessionMemoryState>,
) -> Option<ExtractionWork> {
    let extraction_succeeded = result.is_ok();
    if result.is_err()
        && let Some(original_summary) = original_summary.as_ref()
        && let Err(err) =
            atomic_write(&store.summary_path, original_summary.as_bytes().to_vec()).await
    {
        warn!("failed to restore session memory summary after extraction error: {err:#}");
    }
    let owns_current_generation = is_current_generation(coordinator, generation).await;
    let mut commit_succeeded = false;
    if owns_current_generation {
        commit_succeeded = match finish_extraction(store, result).await {
            Ok(()) => true,
            Err(err) => {
                warn!("failed to finish session memory extraction: {err:#}");
                false
            }
        };
    }

    let mut restored_stale_result = false;
    loop {
        let (must_restore, next) = {
            let mut state = coordinator.state.lock().await;
            let still_current = state.generation == generation
                && state
                    .running
                    .as_ref()
                    .is_some_and(|running| running.generation == generation);
            if !still_current && !restored_stale_result {
                (true, None)
            } else {
                if still_current
                    && extraction_succeeded
                    && commit_succeeded
                    && state
                        .running
                        .as_ref()
                        .is_some_and(|running| running.kind == ExtractionKind::FinalRefresh)
                {
                    state.final_refresh_generation = Some(generation);
                }
                if state
                    .running
                    .as_ref()
                    .is_some_and(|running| running.generation == generation)
                {
                    state.running = None;
                }
                let next = state.pending_final.take();
                if let Some(next) = &next {
                    state.running = Some(RunningExtraction {
                        kind: next.kind,
                        generation: state.generation,
                        cancellation_token: CancellationToken::new(),
                    });
                }
                (false, next)
            }
        };
        if must_restore {
            restore_stale_result(store, original_summary.as_ref(), original_state.as_ref()).await;
            restored_stale_result = true;
            continue;
        }
        return next;
    }
}

async fn is_current_generation(coordinator: &ExtractionCoordinator, generation: u64) -> bool {
    let state = coordinator.state.lock().await;
    state.generation == generation
        && state
            .running
            .as_ref()
            .is_some_and(|running| running.generation == generation)
}

async fn restore_stale_result(
    store: &SessionMemoryStore,
    original_summary: Option<&String>,
    original_state: Option<&super::SessionMemoryState>,
) {
    if let Some(original_summary) = original_summary
        && let Err(err) =
            atomic_write(&store.summary_path, original_summary.as_bytes().to_vec()).await
    {
        warn!("failed to restore stale session memory summary: {err:#}");
    }
    if let Some(original_state) = original_state
        && let Err(err) = store.write_state(original_state).await
    {
        warn!("failed to restore stale session memory state: {err:#}");
    }
}

pub(super) async fn wait_until_idle(
    coordinator: &ExtractionCoordinator,
    timeout: Duration,
) -> bool {
    tokio::time::timeout(timeout, async {
        loop {
            let changed = coordinator.changed.notified();
            if !has_active_extraction(coordinator).await {
                return;
            }
            changed.await;
        }
    })
    .await
    .is_ok()
}

pub(super) async fn invalidate_for_compact(coordinator: &ExtractionCoordinator) {
    {
        let mut state = coordinator.state.lock().await;
        if let Some(running) = &state.running {
            running.cancellation_token.cancel();
        }
        state.generation = state.generation.saturating_add(1);
        state.pending_final = None;
        state.final_refresh_generation = None;
    }
    loop {
        let changed = coordinator.changed.notified();
        if !has_active_extraction(coordinator).await {
            break;
        }
        changed.await;
    }
}

pub(super) async fn wait_for_shutdown(
    coordinator: &ExtractionCoordinator,
    timeout: Duration,
) -> Result<(), CodexErr> {
    if wait_until_idle(coordinator, timeout).await {
        return Ok(());
    }
    Err(CodexErr::Fatal(
        "session memory extraction did not finish before shutdown timeout".to_string(),
    ))
}
