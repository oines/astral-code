//! Skill and plugin mentions for the Astral composer.
//!
//! The visible token stays in the prompt while the canonical target is carried
//! separately as app-server `UserInput::Skill` or `UserInput::Mention`.

mod submission;

pub(crate) use submission::MentionBinding;
pub(crate) use submission::MentionTarget;
pub use submission::PromptSubmission;
