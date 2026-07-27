use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;

use codex_app_server_protocol::UserInput;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MentionTarget {
    Skill { name: String, path: PathBuf },
    Plugin { name: String, path: String },
}

impl MentionTarget {
    pub(super) fn key(&self) -> &str {
        match self {
            Self::Skill { path, .. } => path.to_str().unwrap_or_default(),
            Self::Plugin { path, .. } => path,
        }
    }

    fn to_user_input(&self) -> UserInput {
        match self {
            Self::Skill { name, path } => UserInput::Skill {
                name: name.clone(),
                path: path.clone(),
            },
            Self::Plugin { name, path } => UserInput::Mention {
                name: name.clone(),
                path: path.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MentionBinding {
    pub(crate) range: Range<usize>,
    pub(crate) insert_text: String,
    pub(crate) target: MentionTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSubmission {
    pub(crate) text: String,
    pub(crate) mentions: Vec<MentionBinding>,
}

impl PromptSubmission {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn user_input(&self) -> Vec<UserInput> {
        let mut input = vec![UserInput::Text {
            text: self.text.clone(),
            text_elements: Vec::new(),
        }];
        let mut seen = HashSet::new();
        for binding in &self.mentions {
            if seen.insert(binding.target.key().to_string()) {
                input.push(binding.target.to_user_input());
            }
        }
        input
    }
}

#[cfg(test)]
#[path = "../mention_submission_tests.rs"]
mod tests;
