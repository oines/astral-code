use crate::mention::MentionCatalog;
use crate::mention::MentionSnapshot;
use crate::mention::PromptSubmission;

use super::SurfaceState;

impl SurfaceState {
    pub(crate) fn mentions(&self) -> &MentionSnapshot {
        self.mentions.snapshot()
    }

    pub(crate) fn set_mention_catalog(&mut self, catalog: MentionCatalog) {
        self.mentions.set_catalog(catalog);
        self.refresh_mentions();
    }

    pub(crate) fn refresh_composer_completions(&mut self) {
        self.refresh_slash();
        self.refresh_mentions();
    }

    pub(crate) fn refresh_mentions(&mut self) {
        self.mentions
            .refresh(self.composer.text(), self.composer.cursor());
    }

    pub(crate) fn move_mention_selection(&mut self, delta: isize) {
        self.mentions.move_selection(delta);
    }

    pub(crate) fn select_mention(&mut self, index: usize) {
        self.mentions.select(index);
    }

    pub(crate) fn dismiss_mentions(&mut self) {
        self.mentions.dismiss(self.composer.text());
    }

    pub(crate) fn accept_mention_selection(&mut self) -> bool {
        let Some((range, selection)) = self.mentions.selection() else {
            return false;
        };
        self.composer
            .insert_mention(range, selection.insert_text, selection.target);
        self.refresh_composer_completions();
        true
    }

    pub(crate) fn take_submission(&mut self) -> PromptSubmission {
        let submission = self.composer.take_submission();
        self.refresh_composer_completions();
        submission
    }

    pub(crate) fn restore_submission(&mut self, submission: PromptSubmission) {
        self.composer.restore_submission(submission);
        self.refresh_composer_completions();
    }
}
