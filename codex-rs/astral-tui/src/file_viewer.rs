//! File-reference line viewer state.
//!
//! File bytes still come from app-server's existing `fs/readFile` RPC. This
//! module only owns transient TUI state and the pending composer replacement.

use std::ops::Range;

use crate::block_viewer::ViewerState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileViewerRequest {
    pub(crate) generation: u64,
    pub(crate) path: String,
}

#[derive(Debug)]
pub(crate) enum FileViewerContent {
    Loading,
    Ready(String),
    Error(String),
}

#[derive(Debug)]
pub(crate) struct FileViewerState {
    generation: u64,
    path: String,
    replace_range: Range<usize>,
    initial_selection: Option<Range<usize>>,
    viewer: ViewerState,
    content: FileViewerContent,
}

impl FileViewerState {
    pub(crate) fn loading(
        generation: u64,
        path: String,
        replace_range: Range<usize>,
        initial_range: Option<Range<usize>>,
    ) -> Self {
        Self {
            generation,
            path,
            replace_range,
            initial_selection: initial_range
                .map(|range| range.start.saturating_sub(1)..range.end.saturating_sub(1)),
            viewer: ViewerState::new(false),
            content: FileViewerContent::Loading,
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn replace_range(&self) -> Range<usize> {
        self.replace_range.clone()
    }

    pub(crate) fn viewer(&self) -> &ViewerState {
        &self.viewer
    }

    pub(crate) fn viewer_mut(&mut self) -> &mut ViewerState {
        &mut self.viewer
    }

    pub(crate) fn content(&self) -> &FileViewerContent {
        &self.content
    }

    pub(crate) fn take_initial_selection(&mut self) -> Option<Range<usize>> {
        self.initial_selection.take()
    }

    pub(crate) fn set_result(&mut self, result: Result<String, String>) {
        self.content = match result {
            Ok(source) => FileViewerContent::Ready(source),
            Err(error) => FileViewerContent::Error(error),
        };
    }

    pub(crate) fn is_ready(&self) -> bool {
        matches!(self.content, FileViewerContent::Ready(_))
    }

    pub(crate) fn reference_path(&self, include_range: bool) -> String {
        if !include_range {
            return self.path.clone();
        }
        let Some(range) = self.viewer.selected_physical_range() else {
            return self.path.clone();
        };
        let start = range.start.saturating_add(1);
        let end = range.end;
        if start == end {
            format!("{}:{start}", self.path)
        } else {
            format!("{}:{start}-{end}", self.path)
        }
    }
}
