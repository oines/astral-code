//! File-drop classification and local-image prompt elements.
//!
//! The whole-paste classification rules and image-chip invariants follow Grok
//! Build's `prompt_images.rs` and `PromptWidget::insert_image` at commit
//! 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0). The resulting
//! element still projects to Astral's existing app-server `LocalImage` input.

use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;

use super::ComposerElement;
use super::ComposerState;
use super::LocalImage;
use super::history::MutationKind;

const DROP_CLASSIFIER_MAX_BYTES: usize = 10 * 1024 * 1024;
const IMAGE_CAP: usize = 10;
const MIN_IMAGE_SIDE: u32 = 8;
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tiff", "tif"];

enum DroppedPath {
    Image(LocalImage),
    NonImage(PathBuf),
}

impl ComposerState {
    /// Handle one terminal paste, promoting unambiguous dropped images to
    /// atomic chips while preserving all other payloads as editable text.
    ///
    /// Returns a user-facing notice when one or more image entries were
    /// rejected. A mixed paste still inserts every valid entry in source order.
    pub(crate) fn insert_paste_payload(&mut self, text: &str) -> Option<String> {
        let Some(dropped) = classify_dropped_paths(text) else {
            self.insert_paste(text);
            return None;
        };
        self.insert_dropped_paths(dropped)
    }

    pub(crate) fn local_image_at_cursor(&self) -> Option<LocalImage> {
        self.local_image_at_position(self.cursor)
    }

    pub(super) fn local_image_at_position(&self, position: usize) -> Option<LocalImage> {
        self.elements.iter().find_map(|element| {
            (position >= element.range.start
                && position < element.range.end
                && element.matches_text(&self.text))
            .then(|| element.local_image_data().cloned())
            .flatten()
        })
    }

    pub(super) fn restore_image_counter_high_water(&mut self) {
        let highest = self
            .elements
            .iter()
            .filter_map(ComposerElement::local_image_data)
            .map(|image| image.display_number)
            .max()
            .unwrap_or_default();
        self.image_counter = self.image_counter.max(highest);
    }

    fn insert_dropped_paths(&mut self, dropped: Vec<DroppedPath>) -> Option<String> {
        let range = self.selection_range().unwrap_or(self.cursor..self.cursor);
        let range = self.expand_range_to_element_boundaries(range);
        let retained_images = self
            .elements
            .iter()
            .filter(|element| {
                element.local_image_data().is_some()
                    && element.matches_text(&self.text)
                    && !ranges_overlap(&element.range, &range)
            })
            .count();
        let available_images = IMAGE_CAP.saturating_sub(retained_images);

        let mut replacement = String::new();
        let mut images = Vec::new();
        let mut rejected = None;
        for entry in dropped {
            match entry {
                DroppedPath::Image(mut image) => {
                    if let Some((width, height)) = image.dimensions
                        && (width < MIN_IMAGE_SIDE || height < MIN_IMAGE_SIDE)
                    {
                        rejected.get_or_insert_with(|| {
                            format!(
                                "Image too small ({width}×{height}); minimum is \
                                 {MIN_IMAGE_SIDE}×{MIN_IMAGE_SIDE}"
                            )
                        });
                        continue;
                    }
                    if images.len() >= available_images {
                        rejected.get_or_insert_with(|| {
                            format!("Image limit reached (max {IMAGE_CAP})")
                        });
                        continue;
                    }

                    self.image_counter = self.image_counter.saturating_add(1);
                    image.display_number = self.image_counter;
                    let offset = replacement.len();
                    let placeholder = image.placeholder();
                    replacement.push_str(&placeholder);
                    replacement.push(' ');
                    images.push((offset, placeholder.len(), image));
                }
                DroppedPath::NonImage(path) => {
                    replacement.push_str(&path.display().to_string());
                    replacement.push(' ');
                }
            }
        }

        if replacement.is_empty() {
            return rejected;
        }

        let start = range.start;
        self.replace_range(range, &replacement, MutationKind::Replace);
        self.elements
            .extend(images.into_iter().map(|(offset, placeholder_len, image)| {
                let element_start = start.saturating_add(offset);
                ComposerElement::local_image(
                    element_start..element_start.saturating_add(placeholder_len),
                    image,
                )
            }));
        self.elements.sort_by_key(|element| element.range.start);
        rejected
    }
}

fn ranges_overlap(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && left.end > right.start
}

fn classify_dropped_paths(text: &str) -> Option<Vec<DroppedPath>> {
    let trimmed = text.trim();
    if trimmed.is_empty() || text.len() >= DROP_CLASSIFIER_MAX_BYTES {
        return None;
    }

    let normalized = if trimmed.contains('\r') {
        trimmed.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        trimmed.to_string()
    };
    let mut dropped = Vec::new();
    for line in normalized.lines() {
        let tokens = space_split_line(line);
        if tokens.is_empty() {
            continue;
        }
        let resolved = tokens
            .iter()
            .filter_map(|token| classify_path_token(token))
            .collect::<Vec<_>>();
        if resolved.len() != tokens.len() {
            return None;
        }
        dropped.extend(resolved);
    }
    (!dropped.is_empty()).then_some(dropped)
}

fn classify_path_token(token: &str) -> Option<DroppedPath> {
    let unquoted = strip_matching_quotes(token.trim());
    let is_file_url = unquoted.starts_with("file://");
    if !is_file_url && !starts_with_path_anchor(unquoted) {
        return None;
    }

    let path = token_to_path(token)?;
    if path.as_os_str().is_empty()
        || path == Path::new("/")
        || path
            .to_string_lossy()
            .bytes()
            .any(|byte| matches!(byte, 0 | b'\r' | b'\n'))
    {
        return None;
    }

    if let Some(image) = read_image(&path) {
        return Some(DroppedPath::Image(image));
    }
    if !is_file_url && !path.exists() {
        return None;
    }
    Some(DroppedPath::NonImage(
        std::fs::canonicalize(&path).unwrap_or(path),
    ))
}

fn read_image(path: &Path) -> Option<LocalImage> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    if !IMAGE_EXTENSIONS.contains(&extension.as_str()) || !path.is_file() {
        return None;
    }
    let dimensions = image::image_dimensions(path).ok()?;
    let byte_len = std::fs::metadata(path).ok().map(|metadata| metadata.len());
    Some(LocalImage {
        path: std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()),
        display_number: 0,
        dimensions: Some(dimensions),
        byte_len,
    })
}

fn token_to_path(token: &str) -> Option<PathBuf> {
    let token = token.trim();
    let unquoted = strip_matching_quotes(token);
    if unquoted.starts_with("file://") {
        let url = url::Url::parse(unquoted).ok()?;
        return (url.scheme() == "file")
            .then(|| url.to_file_path().ok())
            .flatten();
    }

    let unescaped = shell_unescape(unquoted);
    if let Some(remainder) = unescaped.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return Some(PathBuf::from(home).join(remainder));
    }
    Some(PathBuf::from(unescaped))
}

fn shell_unescape(path: &str) -> String {
    if !path.contains('\\') || looks_like_windows_path(path) {
        return path.to_string();
    }
    let mut unescaped = String::with_capacity(path.len());
    let mut characters = path.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            unescaped.push(characters.next().unwrap_or(character));
        } else {
            unescaped.push(character);
        }
    }
    unescaped
}

fn strip_matching_quotes(text: &str) -> &str {
    let bytes = text.as_bytes();
    if bytes.len() >= 2
        && matches!(
            (bytes[0], bytes[bytes.len() - 1]),
            (b'"', b'"') | (b'\'', b'\'')
        )
    {
        &text[1..text.len() - 1]
    } else {
        text
    }
}

fn looks_like_windows_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/'))
        || bytes.starts_with(b"\\\\")
}

fn starts_with_path_anchor(text: &str) -> bool {
    text.starts_with('/') || text.starts_with("~/") || looks_like_windows_path(text)
}

fn starts_with_drop_anchor(text: &str) -> bool {
    if starts_with_path_anchor(text) || text.starts_with("file://") {
        return true;
    }
    let unquoted = strip_matching_quotes(text);
    unquoted.len() != text.len()
        && (starts_with_path_anchor(unquoted) || unquoted.starts_with("file://"))
}

fn split_space_before_path(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b' ' && starts_with_drop_anchor(&text[index + 1..]) {
            parts.push(&text[start..index]);
            start = index + 1;
        }
    }
    parts.push(&text[start..]);
    parts
}

fn space_split_line(line: &str) -> Vec<&str> {
    let line = line.trim();
    if line.is_empty() {
        return Vec::new();
    }
    let parts = split_space_before_path(line)
        .into_iter()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() > 1 && parts.iter().all(|part| starts_with_drop_anchor(part)) {
        parts
    } else {
        vec![line]
    }
}
