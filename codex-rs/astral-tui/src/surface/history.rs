use crate::PromptSubmission;
use crate::history::HistorySnapshot;

use super::SurfaceState;

impl SurfaceState {
    pub(crate) fn history(&self) -> &HistorySnapshot {
        self.history.snapshot()
    }

    pub(crate) fn open_history_browse(&mut self) -> bool {
        let saved = self.composer.submission();
        let Some(submission) = self.history.activate_browse(saved) else {
            return false;
        };
        self.replace_composer_for_history(submission);
        true
    }

    pub(crate) fn open_history_search(&mut self) {
        let saved = self.composer.submission();
        self.history.activate_search(saved);
        self.slash.close();
        self.mentions.dismiss(self.composer.text());
    }

    pub(crate) fn update_history_query(&mut self) {
        self.history.update_query(self.composer.text());
        self.slash.close();
        self.mentions.dismiss(self.composer.text());
    }

    pub(crate) fn move_history_selection(&mut self, delta: isize) -> bool {
        let moved = self.history.move_selection(delta);
        if moved && self.history.snapshot().browse {
            self.populate_selected_history();
        }
        moved
    }

    pub(crate) fn page_history_selection(&mut self, delta: isize) -> bool {
        let visible_rows = self.completion_visible_rows().unwrap_or(8);
        let moved = self.history.page_selection(delta, visible_rows);
        if moved && self.history.snapshot().browse {
            self.populate_selected_history();
        }
        moved
    }

    pub(crate) fn select_history(&mut self, index: usize) {
        self.history.select(index);
        if self.history.snapshot().browse {
            self.populate_selected_history();
        }
    }

    pub(crate) fn accept_history_selection(&mut self) {
        let submission = self.history.accept();
        self.restore_submission(submission);
    }

    pub(crate) fn cancel_history(&mut self) -> bool {
        if !self.history.snapshot().open {
            return false;
        }
        let saved = self.history.cancel();
        self.restore_submission(saved);
        true
    }

    pub(crate) fn detach_history(&mut self) {
        self.history.detach();
    }

    pub(crate) fn record_submission(&mut self, submission: &PromptSubmission) {
        self.history.record(submission);
    }

    fn populate_selected_history(&mut self) {
        if let Some(submission) = self.history.selected_submission().cloned() {
            self.replace_composer_for_history(submission);
        }
    }

    fn replace_composer_for_history(&mut self, submission: PromptSubmission) {
        self.composer.restore_submission(submission);
        self.slash.close();
        self.mentions.dismiss(self.composer.text());
    }
}
