//! Skill and plugin mentions for the Astral composer.
//!
//! The visible token stays in the prompt while the canonical target is carried
//! separately as app-server `UserInput::Skill` or `UserInput::Mention`.

mod catalog;
mod inventory;
mod submission;

pub(crate) use catalog::MentionCandidate;
pub(crate) use catalog::MentionCatalog;
pub(crate) use catalog::MentionController;
pub(crate) use catalog::MentionKind;
pub(crate) use catalog::MentionSnapshot;
pub(crate) use submission::MentionTarget;
pub use submission::PromptSubmission;
