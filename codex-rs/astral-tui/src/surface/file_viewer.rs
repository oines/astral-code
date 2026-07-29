//! File-viewer lifecycle and composer integration.

use codex_app_server_protocol::FuzzyFileSearchMatchType;

use crate::file_viewer::FileViewerRequest;
use crate::file_viewer::FileViewerState;

use super::SurfaceState;

impl SurfaceState {
    pub(crate) fn file_viewer(&self) -> Option<&FileViewerState> {
        self.file_viewer.as_ref()
    }

    pub(crate) fn file_viewer_mut(&mut self) -> Option<&mut FileViewerState> {
        self.file_viewer.as_mut()
    }

    pub(crate) fn open_file_search_viewer(&mut self) -> bool {
        let Some((range, result)) = self.file_search.selection() else {
            return false;
        };
        if result.match_type == FuzzyFileSearchMatchType::Directory {
            return false;
        }
        let path = result.path.strip_prefix("./").unwrap_or(&result.path);
        self.open_file_viewer(range, path.to_string(), None);
        true
    }

    pub(crate) fn open_file_reference_viewer(&mut self, boundary_only: bool) -> bool {
        let reference = if boundary_only {
            self.composer.file_reference_at_boundary()
        } else {
            self.composer.file_reference_at_cursor()
        };
        let Some(reference) = reference else {
            return false;
        };
        self.open_file_viewer(reference.range, reference.path, reference.line_range);
        true
    }

    pub(crate) fn take_file_viewer_request(&mut self) -> Option<FileViewerRequest> {
        self.pending_file_viewer_request.take()
    }

    pub(crate) fn apply_file_viewer_result(
        &mut self,
        generation: u64,
        result: Result<String, String>,
    ) -> bool {
        let Some(viewer) = self.file_viewer.as_mut() else {
            return false;
        };
        if viewer.generation() != generation {
            return false;
        }
        viewer.set_result(result);
        true
    }

    pub(crate) fn close_file_viewer(&mut self) {
        self.pending_file_viewer_request = None;
        self.file_viewer = None;
        self.refresh_composer_completions();
    }

    pub(crate) fn confirm_file_viewer(&mut self, include_range: bool) -> bool {
        let Some(viewer) = self.file_viewer.take() else {
            return false;
        };
        if !viewer.is_ready() {
            self.file_viewer = Some(viewer);
            return false;
        }
        self.pending_file_viewer_request = None;
        let range = viewer.replace_range();
        let reference = viewer.reference_path(include_range);
        self.composer.replace_file_reference(range, &reference);
        self.refresh_composer_completions();
        true
    }

    pub(crate) fn file_viewer_copy_text(&self) -> Option<String> {
        self.file_viewer()?.viewer().selected_text()
    }

    pub(crate) fn file_viewer_copy_path(&self) -> Option<String> {
        Some(self.file_viewer()?.path().to_string())
    }

    fn open_file_viewer(
        &mut self,
        replace_range: std::ops::Range<usize>,
        path: String,
        initial_range: Option<std::ops::Range<usize>>,
    ) {
        self.file_viewer_generation = self.file_viewer_generation.wrapping_add(1);
        let generation = self.file_viewer_generation;
        self.file_viewer = Some(FileViewerState::loading(
            generation,
            path.clone(),
            replace_range,
            initial_range,
        ));
        self.pending_file_viewer_request = Some(FileViewerRequest { generation, path });
    }
}
