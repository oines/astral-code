//! Structured spans embedded in the editable prompt.
//!
//! Grok Build models paste, file, image, and mention chips as one family of
//! atomic textarea elements. Astral keeps the same view-state invariant while
//! retaining its own app-server `UserInput` semantics.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use crate::mention::MentionTarget;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ComposerElementKind {
    Mention(MentionTarget),
    Paste { content: Arc<str> },
    LocalImage(LocalImage),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalImage {
    pub(crate) path: PathBuf,
    pub(crate) display_number: usize,
    pub(crate) dimensions: Option<(u32, u32)>,
    pub(crate) byte_len: Option<u64>,
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

    pub(crate) fn local_image(range: Range<usize>, image: LocalImage) -> Self {
        let insert_text = image.placeholder();
        Self {
            range,
            insert_text,
            kind: ComposerElementKind::LocalImage(image),
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
            ComposerElementKind::Paste { .. } | ComposerElementKind::LocalImage(_) => true,
        }
    }

    pub(crate) fn mention_target(&self) -> Option<&MentionTarget> {
        match &self.kind {
            ComposerElementKind::Mention(target) => Some(target),
            ComposerElementKind::Paste { .. } | ComposerElementKind::LocalImage(_) => None,
        }
    }

    pub(crate) fn paste_content(&self) -> Option<&str> {
        match &self.kind {
            ComposerElementKind::Paste { content } => Some(content),
            ComposerElementKind::Mention(_) | ComposerElementKind::LocalImage(_) => None,
        }
    }

    pub(crate) fn local_image_data(&self) -> Option<&LocalImage> {
        match &self.kind {
            ComposerElementKind::LocalImage(image) => Some(image),
            ComposerElementKind::Mention(_) | ComposerElementKind::Paste { .. } => None,
        }
    }

    pub(crate) fn submission_text(&self) -> &str {
        self.paste_content().unwrap_or(&self.insert_text)
    }

    pub(crate) fn is_paste(&self) -> bool {
        matches!(&self.kind, ComposerElementKind::Paste { .. })
    }

    pub(crate) fn is_bracketed_chip(&self) -> bool {
        matches!(
            &self.kind,
            ComposerElementKind::Paste { .. } | ComposerElementKind::LocalImage(_)
        )
    }

    pub(super) fn keep_after_boundary_insertion(&self, at_start: bool, inserted: &str) -> bool {
        match &self.kind {
            ComposerElementKind::Paste { .. } | ComposerElementKind::LocalImage(_) => true,
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

impl LocalImage {
    pub(crate) fn placeholder(&self) -> String {
        format!("[Image #{}]", self.display_number)
    }
}
