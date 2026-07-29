use std::collections::HashSet;
use std::path::PathBuf;

use codex_app_server_protocol::UserInput;

use crate::composer::ComposerElement;

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptSubmission {
    pub(crate) text: String,
    pub(crate) elements: Vec<ComposerElement>,
}

impl PromptSubmission {
    pub(crate) fn text_only(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            elements: Vec::new(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn user_input(&self) -> Vec<UserInput> {
        let mut paste_elements = self
            .elements
            .iter()
            .filter(|element| element.is_paste() && element.matches_text(&self.text))
            .collect::<Vec<_>>();
        paste_elements.sort_by_key(|element| element.range.start);
        let mut text = String::new();
        let mut cursor = 0;
        for element in paste_elements {
            if element.range.start < cursor {
                continue;
            }
            text.push_str(&self.text[cursor..element.range.start]);
            text.push_str(element.submission_text());
            cursor = element.range.end;
        }
        text.push_str(&self.text[cursor..]);

        let mut input = vec![UserInput::Text {
            text,
            text_elements: Vec::new(),
        }];
        let mut seen = HashSet::new();
        for element in &self.elements {
            if !element.matches_text(&self.text) {
                continue;
            }
            let Some(target) = element.mention_target() else {
                continue;
            };
            if seen.insert(target.key().to_string()) {
                input.push(target.to_user_input());
            }
        }
        input
    }

    pub(crate) fn into_slash_args(mut self, command: &str, args: String) -> Self {
        let command_end = 1usize.saturating_add(command.len()).min(self.text.len());
        let args_start = self
            .text
            .get(command_end..)
            .unwrap_or_default()
            .char_indices()
            .find_map(|(offset, character)| {
                (!character.is_whitespace()).then_some(command_end + offset)
            })
            .unwrap_or(self.text.len());
        let args_end = args_start.saturating_add(args.len()).min(self.text.len());
        self.elements.retain_mut(|element| {
            if element.range.start < args_start || element.range.end > args_end {
                return false;
            }
            element.range.start -= args_start;
            element.range.end -= args_start;
            true
        });
        self.text = args;
        self
    }
}

#[cfg(test)]
#[path = "../mention_submission_tests.rs"]
mod tests;
