use codex_protocol::config_types::ModeKind;

use super::SurfaceState;
use crate::plan_review::CompletedPlan;
use crate::plan_review::PlanReviewFocus;
use crate::plan_review::PlanReviewState;

impl SurfaceState {
    pub(crate) fn note_completed_plan(
        &mut self,
        turn_id: impl Into<String>,
        text: impl Into<String>,
    ) {
        self.completed_plan = Some(CompletedPlan::new(turn_id, text));
    }

    pub(crate) fn maybe_open_plan_review(&mut self, turn_id: &str, mode: ModeKind) -> bool {
        let Some(plan) = self
            .completed_plan
            .take()
            .filter(|plan| plan.belongs_to(turn_id))
        else {
            return false;
        };
        if mode != ModeKind::Plan
            || !self.pending_requests.is_empty()
            || self.has_queued_follow_ups()
            || self.plan_review.is_some()
            || self.modal.is_some()
            || self.thread_picker.is_some()
            || self.permission_picker.is_some()
            || self.theme_picker.is_some()
        {
            return false;
        }

        let stashed_submission = self.take_submission();
        self.plan_review = Some(PlanReviewState::new(plan.into_text(), stashed_submission));
        self.focus_prompt();
        true
    }

    pub(crate) fn plan_review(&self) -> Option<&PlanReviewState> {
        self.plan_review.as_ref()
    }

    pub(crate) fn plan_review_mut(&mut self) -> Option<&mut PlanReviewState> {
        self.plan_review.as_mut()
    }

    pub(crate) fn close_plan_review(&mut self, restore_draft: bool) -> Option<PlanReviewState> {
        let review = self.plan_review.take()?;
        self.composer.clear();
        if restore_draft {
            self.restore_submission(review.stashed_submission().clone());
        } else {
            self.refresh_composer_completions();
        }
        Some(review)
    }

    pub(crate) fn cancel_plan_revision(&mut self) {
        self.composer.clear();
        self.refresh_composer_completions();
        if let Some(review) = self.plan_review_mut() {
            review.return_to_decision();
        }
    }

    pub(crate) fn plan_review_focus(&self) -> Option<PlanReviewFocus> {
        self.plan_review().map(PlanReviewState::focus)
    }
}
