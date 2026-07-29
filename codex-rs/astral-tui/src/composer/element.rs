//! Structured spans embedded in the editable prompt.
//!
//! Grok Build models paste, file, image, and mention chips as one family of
//! atomic textarea elements. Astral keeps the same view-state invariant while
//! retaining its own app-server `UserInput` semantics.

use std::ops::Range;
use std::sync::Arc;

use crate::mention::MentionTarget;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ComposerElementKind {
    Mention(MentionTarget),
    Paste { content: Arc<str> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComposerElement {
    pub(crate) range: Range<usize>,
    pub(crate) insert_text: String,
    pub(crate) kind: ComposerElementKind,
}

impl ComposerElement {
    pub(crate) fn mention(range: Range<usize>, insert_text: String, target: MentionTarget) -> Self {
        Self {
            range,
            insert_text,
            kind: ComposerElementKind::Mention(target),
        }
    }

    pub(crate) fn paste(range: Range<usize>, placeholder: String, content: String) -> Self {
        Self {
            range,
            insert_text: placeholder,
            kind: ComposerElementKind::Paste {
                content: content.into(),
            },
        }
    }

    pub(crate) fn matches_text(&self, text: &str) -> bool {
        if text.get(self.range.clone()) != Some(self.insert_text.as_str()) {
            return false;
        }
        match &self.kind {
            ComposerElementKind::Mention(_) => {
                (self.range.start == 0
                    || text[..self.range.start]
                        .chars()
                        .next_back()
                        .is_some_and(char::is_whitespace))
                    && (self.range.end == text.len()
                        || text[self.range.end..]
                            .chars()
                            .next()
                            .is_some_and(char::is_whitespace))
            }
            ComposerElementKind::Paste { .. } => true,
        }
    }

    pub(crate) fn mention_target(&self) -> Option<&MentionTarget> {
        match &self.kind {
            ComposerElementKind::Mention(target) => Some(target),
            ComposerElementKind::Paste { .. } => None,
        }
    }

    pub(crate) fn paste_content(&self) -> Option<&str> {
        match &self.kind {
            ComposerElementKind::Paste { content } => Some(content),
            ComposerElementKind::Mention(_) => None,
        }
    }

    pub(crate) fn submission_text(&self) -> &str {
        self.paste_content().unwrap_or(&self.insert_text)
    }

    pub(crate) fn is_paste(&self) -> bool {
        matches!(&self.kind, ComposerElementKind::Paste { .. })
    }

    pub(super) fn keep_after_boundary_insertion(&self, at_start: bool, inserted: &str) -> bool {
        match &self.kind {
            ComposerElementKind::Paste { .. } => true,
            ComposerElementKind::Mention(_) if at_start => inserted
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace),
            ComposerElementKind::Mention(_) => {
                inserted.chars().next().is_some_and(char::is_whitespace)
            }
        }
    }
}
