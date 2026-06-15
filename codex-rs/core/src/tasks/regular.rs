use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::session::TurnInput;
use crate::session::turn::run_turn;
use crate::session::turn_context::TurnContext;
use crate::state::TaskKind;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::TurnStartedEvent;
use tracing::Instrument;
use tracing::trace_span;

use super::SessionTask;
use super::SessionTaskContext;

#[derive(Default)]
pub(crate) struct RegularTask;

impl RegularTask {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl SessionTask for RegularTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Regular
    }

    fn span_name(&self) -> &'static str {
        "session_task.turn"
    }

    async fn run(
        self: Arc<Self>,
        session: Arc<SessionTaskContext>,
        ctx: Arc<TurnContext>,
        input: Vec<TurnInput>,
        cancellation_token: CancellationToken,
    ) -> Option<String> {
        let sess = session.clone_session();
        let turn_extension_data = session.turn_extension_data();
        let run_turn_span = trace_span!("run_turn");
        let event = EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: ctx.sub_id.clone(),
            trace_id: ctx.trace_id.clone(),
            started_at: ctx.turn_timing_state.started_at_unix_secs().await,
            model_context_window: ctx.model_context_window(),
            collaboration_mode_kind: ctx.collaboration_mode.mode,
        });
        sess.send_event(ctx.as_ref(), event).await;
        sess.set_server_reasoning_included(/*included*/ false).await;
        let mut next_input = input;
        loop {
            let last_agent_message = run_turn(
                Arc::clone(&sess),
                Arc::clone(&ctx),
                Arc::clone(&turn_extension_data),
                next_input,
                cancellation_token.child_token(),
            )
            .instrument(run_turn_span.clone())
            .await;
            if !sess.input_queue.has_pending_input(&sess.active_turn).await {
                return last_agent_message;
            }
            next_input = Vec::new();
        }
    }
}
