use std::collections::HashSet;
use std::path::PathBuf;

use codex_app_server_protocol::ByteRange;
use codex_app_server_protocol::TextElement;
use codex_app_server_protocol::UserInput;

use crate::composer::ComposerElement;
use crate::composer::LocalImage;

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
        let mut elements = self
            .elements
            .iter()
            .filter(|element| element.matches_text(&self.text))
            .collect::<Vec<_>>();
        elements.sort_by_key(|element| element.range.start);
        let mut text = String::new();
        let mut cursor = 0;
        let mut projected = Vec::new();
        for element in elements {
            if element.range.start < cursor {
                continue;
            }
            text.push_str(&self.text[cursor..element.range.start]);
            let output_start = text.len();
            text.push_str(element.submission_text());
            let output_end = text.len();
            projected.push((element, output_start..output_end));
            cursor = element.range.end;
        }
        text.push_str(&self.text[cursor..]);

        let text_elements = projected
            .iter()
            .filter_map(|(element, range)| {
                element.local_image_data().map(|_| {
                    TextElement::new(
                        ByteRange {
                            start: range.start,
                            end: range.end,
                        },
                        Some(element.insert_text.clone()),
                    )
                })
            })
            .collect();
        let mut input = vec![UserInput::Text {
            text,
            text_elements,
        }];
        let mut seen = HashSet::new();
        for (element, _) in projected {
            if let Some(image) = element.local_image_data() {
                input.push(UserInput::LocalImage {
                    detail: None,
                    path: image.path.clone(),
                });
            }
            if let Some(target) = element.mention_target()
                && seen.insert(target.key().to_string())
            {
                input.push(target.to_user_input());
            }
        }
        input
    }

    pub(crate) fn from_user_input(content: &[UserInput]) -> Self {
        let mut text = String::new();
        let mut image_elements = Vec::new();
        let mut image_paths = content.iter().filter_map(|input| match input {
            UserInput::LocalImage { path, .. } => Some(path.clone()),
            UserInput::Text { .. }
            | UserInput::Image { .. }
            | UserInput::Skill { .. }
            | UserInput::Mention { .. } => None,
        });

        for input in content {
            let UserInput::Text {
                text: segment,
                text_elements,
            } = input
            else {
                continue;
            };
            if !text.is_empty() {
                text.push('\n');
            }
            let offset = text.len();
            text.push_str(segment);
            for element in text_elements {
                let Some(placeholder) = element.placeholder() else {
                    continue;
                };
                let Some(display_number) = image_display_number(placeholder) else {
                    continue;
                };
                let range = offset.saturating_add(element.byte_range.start)
                    ..offset.saturating_add(element.byte_range.end);
                if text.get(range.clone()) != Some(placeholder) {
                    continue;
                }
                let Some(path) = image_paths.next() else {
                    break;
                };
                image_elements.push(ComposerElement::local_image(
                    range,
                    LocalImage {
                        path,
                        display_number,
                        dimensions: None,
                        byte_len: None,
                    },
                ));
            }
        }

        Self {
            text,
            elements: image_elements,
        }
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

fn image_display_number(placeholder: &str) -> Option<usize> {
    placeholder
        .strip_prefix("[Image #")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

#[cfg(test)]
#[path = "../mention_submission_tests.rs"]
mod tests;
