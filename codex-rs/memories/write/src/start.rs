use crate::extensions::seed_extension_instructions;
use crate::guard;
use crate::memory_root;
use crate::metrics::MEMORY_STARTUP;
use crate::phase1;
use crate::phase2;
use crate::runtime::MemoryStartupContext;
use codex_config::types::CompactMemoryMode;
use codex_core::CodexThread;
use codex_core::ThreadManager;
use codex_core::config::Config;
use codex_features::Feature;
use codex_login::AuthManager;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionSource;
use std::sync::Arc;
use tracing::warn;

/// Starts the asynchronous startup memory pipeline for an eligible root session.
///
/// The pipeline is skipped for ephemeral sessions, disabled feature flags, and
/// subagent sessions.
pub fn start_memories_startup_task(
    thread_manager: Arc<ThreadManager>,
    auth_manager: Arc<AuthManager>,
    thread_id: ThreadId,
    thread: Arc<CodexThread>,
    config: Arc<Config>,
    source: &SessionSource,
) {
    if config.ephemeral
        || !config.features.enabled(Feature::MemoryTool)
        || source.is_non_root_agent()
    {
        return;
    }

    let context = Arc::new(MemoryStartupContext::new(
        thread_manager,
        Arc::clone(&auth_manager),
        thread_id,
        thread,
        config.as_ref(),
        source.clone(),
    ));

    if context.state_db().is_none() {
        warn!("state db unavailable for memories startup pipeline; skipping");
        return;
    }

    tokio::spawn(async move {
        let root = memory_root(&config.codex_home);
        if let Err(err) = tokio::fs::create_dir_all(&root).await {
            warn!("failed creating memories root: {err}");
            return;
        }
        if let Err(err) = seed_extension_instructions(&root).await {
            warn!("failed seeding memory extension instructions: {err}");
        }

        // Clean memories to make preserve DB size. This does not consume tokens so can be
        // done before the quota check.
        phase1::prune(context.as_ref(), &config).await;

        if !guard::rate_limits_ok(&auth_manager, &config).await {
            context.counter(
                MEMORY_STARTUP,
                /*inc*/ 1,
                &[("status", "skipped_rate_limit")],
            );
            return;
        }

        // Run phase 1.
        phase1::run(Arc::clone(&context), Arc::clone(&config)).await;
        // Run phase 2.
        phase2::run(context, config).await;
    });
}

/// Starts compact-triggered memory extraction for the current thread.
pub fn start_compact_memory_task(
    thread_manager: Arc<ThreadManager>,
    auth_manager: Arc<AuthManager>,
    thread_id: ThreadId,
) {
    tokio::spawn(async move {
        run_compact_memory_task(thread_manager, auth_manager, thread_id).await;
    });
}

/// Runs compact-triggered memory extraction for the current thread and then
/// waits for consolidation to finish.
pub async fn run_compact_memory_task(
    thread_manager: Arc<ThreadManager>,
    auth_manager: Arc<AuthManager>,
    thread_id: ThreadId,
) {
    let Some((context, config)) =
        prepare_compact_memory_task(thread_manager, auth_manager, thread_id).await
    else {
        return;
    };

    phase1::run_current_thread(Arc::clone(&context), Arc::clone(&config)).await;
    phase2::run_blocking_after_compact(context, config).await;
}

async fn prepare_compact_memory_task(
    thread_manager: Arc<ThreadManager>,
    auth_manager: Arc<AuthManager>,
    thread_id: ThreadId,
) -> Option<(Arc<MemoryStartupContext>, Arc<Config>)> {
    let thread = match thread_manager.get_thread(thread_id).await {
        Ok(thread) => thread,
        Err(err) => {
            warn!("compact memory skipped: failed to get thread {thread_id}: {err}");
            return None;
        }
    };
    let config_snapshot = thread.config_snapshot().await;
    let config = thread.config().await;

    if config.ephemeral
        || !config.features.enabled(Feature::MemoryTool)
        || !config.memories.generate_memories
        || matches!(config.memories.compact_memory, CompactMemoryMode::Off)
        || config_snapshot.session_source.is_non_root_agent()
    {
        return None;
    }

    let context = Arc::new(MemoryStartupContext::new(
        Arc::clone(&thread_manager),
        Arc::clone(&auth_manager),
        thread_id,
        thread,
        config.as_ref(),
        config_snapshot.session_source,
    ));

    if context.state_db().is_none() {
        warn!("state db unavailable for compact memory pipeline; skipping");
        return None;
    }

    let root = memory_root(&config.codex_home);
    if let Err(err) = tokio::fs::create_dir_all(&root).await {
        warn!("failed creating memories root for compact memory: {err}");
        return None;
    }
    if let Err(err) = seed_extension_instructions(&root).await {
        warn!("failed seeding memory extension instructions for compact memory: {err}");
    }

    phase1::prune(context.as_ref(), &config).await;

    if !guard::rate_limits_ok(&auth_manager, &config).await {
        context.counter(
            MEMORY_STARTUP,
            /*inc*/ 1,
            &[("status", "compact_skipped_rate_limit")],
        );
        return None;
    }

    Some((context, config))
}
