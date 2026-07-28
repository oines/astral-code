use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::config_types::ModeKind;

use super::reset_surface_after_switch;
use crate::AstralSession;
use crate::PromptSubmission;
use crate::SurfaceActivity;
use crate::SurfaceState;
use crate::plan_review::IMPLEMENT_PLAN_MESSAGE;
use crate::plan_review::PlanReviewAction;
use crate::plan_review::fresh_implementation_prompt;

pub(super) async fn apply_action(
    session: &mut AstralSession,
    surface: &mut SurfaceState,
    action: PlanReviewAction,
) {
    match action {
        PlanReviewAction::Implement => {
            start_submission(
                session,
                surface,
                PromptSubmission::text_only(IMPLEMENT_PLAN_MESSAGE),
                ModeKind::Default,
            )
            .await;
        }
        PlanReviewAction::ImplementFresh { plan } => match session.start_new().await {
            Ok(outcome) => {
                reset_surface_after_switch(session, surface, outcome).await;
                start_submission(
                    session,
                    surface,
                    PromptSubmission::text_only(fresh_implementation_prompt(&plan)),
                    ModeKind::Default,
                )
                .await;
            }
            Err(error) => surface.set_notice(error.to_string()),
        },
        PlanReviewAction::Revise { feedback } => {
            start_submission(session, surface, feedback, ModeKind::Plan).await;
        }
    }
}

pub(super) fn handle_notification(
    surface: &mut SurfaceState,
    notification: &ServerNotification,
    mode: ModeKind,
) {
    let active_thread_id = surface.conversation().thread_id().to_string();
    match notification {
        ServerNotification::ItemCompleted(params) if params.thread_id == active_thread_id => {
            if let ThreadItem::Plan { text, .. } = &params.item {
                surface.note_completed_plan(&params.turn_id, text);
            }
        }
        ServerNotification::TurnCompleted(params)
            if params.thread_id == active_thread_id
                && params.turn.status == TurnStatus::Completed =>
        {
            surface.maybe_open_plan_review(&params.turn.id, mode);
        }
        _ => {}
    }
}

async fn start_submission(
    session: &mut AstralSession,
    surface: &mut SurfaceState,
    submission: PromptSubmission,
    mode: ModeKind,
) {
    surface.set_activity(SurfaceActivity::Working);
    if let Err(error) = session
        .start_turn_in_mode(submission.user_input(), mode)
        .await
    {
        surface.set_activity(SurfaceActivity::Ready);
        surface.set_notice(error.to_string());
    }
}
