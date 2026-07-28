use crate::PromptSubmission;

pub(crate) const IMPLEMENT_PLAN_MESSAGE: &str = "Implement the plan.";
pub(crate) const FRESH_IMPLEMENTATION_PREFIX: &str = concat!(
    "A previous agent produced the plan below to accomplish the user's task. ",
    "Implement the plan in a fresh context. Treat the plan as the source of ",
    "user intent, re-read files as needed, and carry the work through ",
    "implementation and verification."
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompletedPlan {
    turn_id: String,
    text: String,
}

impl CompletedPlan {
    pub(crate) fn new(turn_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            turn_id: turn_id.into(),
            text: text.into(),
        }
    }

    pub(crate) fn belongs_to(&self, turn_id: &str) -> bool {
        self.turn_id == turn_id
    }

    pub(crate) fn into_text(self) -> String {
        self.text
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanReviewFocus {
    Decision,
    Revision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanReviewAction {
    Implement,
    ImplementFresh { plan: String },
    Revise { feedback: PromptSubmission },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanReviewState {
    plan: String,
    focus: PlanReviewFocus,
    stashed_submission: PromptSubmission,
}

impl PlanReviewState {
    pub(crate) fn new(plan: String, stashed_submission: PromptSubmission) -> Self {
        Self {
            plan,
            focus: PlanReviewFocus::Decision,
            stashed_submission,
        }
    }

    pub(crate) fn plan(&self) -> &str {
        &self.plan
    }

    pub(crate) fn has_plan(&self) -> bool {
        !self.plan.trim().is_empty()
    }

    pub(crate) fn focus(&self) -> PlanReviewFocus {
        self.focus
    }

    pub(crate) fn begin_revision(&mut self) {
        self.focus = PlanReviewFocus::Revision;
    }

    pub(crate) fn return_to_decision(&mut self) {
        self.focus = PlanReviewFocus::Decision;
    }

    pub(crate) fn stashed_submission(&self) -> &PromptSubmission {
        &self.stashed_submission
    }
}

pub(crate) fn fresh_implementation_prompt(plan: &str) -> String {
    format!("{FRESH_IMPLEMENTATION_PREFIX}\n\n{plan}")
}
