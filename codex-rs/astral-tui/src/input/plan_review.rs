use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use super::InputAction;
use crate::SurfaceState;
use crate::plan_review::PlanReviewAction;
use crate::plan_review::PlanReviewFocus;

pub(super) fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    match state.plan_review_focus() {
        Some(PlanReviewFocus::Decision) => handle_decision_key(state, key),
        Some(PlanReviewFocus::Revision) => handle_revision_key(state, key),
        None => InputAction::None,
    }
}

pub(super) fn handle_paste(state: &mut SurfaceState, text: &str) -> InputAction {
    if state.plan_review_focus() != Some(PlanReviewFocus::Revision) {
        return InputAction::None;
    }
    state.composer_state_mut().insert_text(text);
    state.refresh_composer_completions();
    InputAction::Redraw
}

fn handle_decision_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    match (key.code, key.modifiers) {
        (KeyCode::Enter | KeyCode::Char('a'), KeyModifiers::NONE) => {
            state.close_plan_review(/*restore_draft*/ true);
            InputAction::Plan(PlanReviewAction::Implement)
        }
        (KeyCode::Char('c'), KeyModifiers::NONE) => {
            let Some(plan) = state.plan_review().map(|review| review.plan().to_string()) else {
                return InputAction::None;
            };
            if plan.trim().is_empty() {
                return InputAction::Notice("No approved plan is available".to_string());
            }
            state.close_plan_review(/*restore_draft*/ false);
            InputAction::Plan(PlanReviewAction::ImplementFresh { plan })
        }
        (KeyCode::Char('s') | KeyCode::Tab, KeyModifiers::NONE) => {
            if let Some(review) = state.plan_review_mut() {
                review.begin_revision();
            }
            InputAction::Redraw
        }
        (KeyCode::Char('q') | KeyCode::Esc, KeyModifiers::NONE) => {
            state.close_plan_review(/*restore_draft*/ true);
            InputAction::Redraw
        }
        (KeyCode::PageUp, _) => InputAction::ScrollUp,
        (KeyCode::PageDown, _) => InputAction::ScrollDown,
        _ => InputAction::None,
    }
}

fn handle_revision_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    match (key.code, key.modifiers) {
        (KeyCode::Esc | KeyCode::Tab, KeyModifiers::NONE) => {
            state.cancel_plan_revision();
            InputAction::Redraw
        }
        (KeyCode::Enter, modifiers)
            if !modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
        {
            if state.composer().trim().is_empty() {
                state.close_plan_review(/*restore_draft*/ true);
                return InputAction::Plan(PlanReviewAction::Implement);
            }
            let feedback = state.take_submission();
            state.close_plan_review(/*restore_draft*/ true);
            InputAction::Plan(PlanReviewAction::Revise { feedback })
        }
        (KeyCode::Enter, _) => {
            state.composer_state_mut().insert_char('\n');
            state.refresh_composer_completions();
            InputAction::Redraw
        }
        _ if state.composer_state_mut().edit_key(key) => {
            state.refresh_composer_completions();
            InputAction::Redraw
        }
        _ => InputAction::None,
    }
}
