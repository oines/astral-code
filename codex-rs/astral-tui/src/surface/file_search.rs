use codex_app_server_protocol::FuzzyFileSearchMatchType;
use codex_app_server_protocol::FuzzyFileSearchResult;

use crate::file_search::FileSearchRequest;
use crate::file_search::FileSearchSnapshot;

use super::SurfaceState;

impl SurfaceState {
    pub(crate) fn file_search(&self) -> &FileSearchSnapshot {
        self.file_search.snapshot()
    }

    pub(crate) fn refresh_file_search(&mut self) {
        self.file_search.refresh(
            self.composer.text(),
            self.composer.cursor(),
            self.composer.elements(),
        );
    }

    pub(crate) fn take_file_search_request(&mut self) -> Option<FileSearchRequest> {
        self.file_search.take_request()
    }

    pub(crate) fn apply_file_search_results(
        &mut self,
        generation: u64,
        query: &str,
        matches: Vec<FuzzyFileSearchResult>,
    ) -> bool {
        self.file_search.apply_results(generation, query, matches)
    }

    pub(crate) fn apply_file_search_error(
        &mut self,
        generation: u64,
        query: &str,
        error: String,
    ) -> bool {
        self.file_search.apply_error(generation, query, error)
    }

    pub(crate) fn move_file_search_selection(&mut self, delta: isize) {
        self.file_search.move_selection(delta);
    }

    pub(crate) fn page_file_search_selection(&mut self, delta: isize) {
        let distance = (self.completion_visible_rows().unwrap_or(8) / 2).max(1) as isize;
        self.file_search.move_selection(delta * distance);
    }

    pub(crate) fn select_file_search(&mut self, index: usize) {
        self.file_search.select(index);
    }

    pub(crate) fn dismiss_file_search(&mut self) {
        self.file_search.dismiss(self.composer.text());
    }

    pub(crate) fn accept_file_search_selection(&mut self) -> bool {
        self.accept_file_search_selection_with(FileAcceptance::Commit)
    }

    pub(crate) fn drill_into_file_search_selection(&mut self) -> bool {
        self.accept_file_search_selection_with(FileAcceptance::DrillDirectory)
    }

    fn accept_file_search_selection_with(&mut self, acceptance: FileAcceptance) -> bool {
        let Some((range, result)) = self.file_search.selection() else {
            return false;
        };
        let path = result.path.strip_prefix("./").unwrap_or(&result.path);
        let is_directory = result.match_type == FuzzyFileSearchMatchType::Directory;
        if is_directory
            && (acceptance == FileAcceptance::DrillDirectory
                || self.file_search.snapshot().is_directory_mode())
        {
            let suffix = if self.file_search.snapshot().is_directory_mode() {
                "/"
            } else {
                ""
            };
            self.composer
                .replace_file_reference_path(range, &format!("{path}{suffix}"));
        } else {
            self.composer.insert_file_reference(range, path.to_string());
        }
        self.refresh_composer_completions();
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileAcceptance {
    Commit,
    DrillDirectory,
}
