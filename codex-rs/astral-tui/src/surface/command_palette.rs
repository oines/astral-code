use crate::command_palette::CommandPaletteState;

use super::SurfaceState;

impl SurfaceState {
    pub(crate) fn command_palette(&self) -> Option<&CommandPaletteState> {
        self.command_palette.as_ref()
    }

    pub(crate) fn command_palette_mut(&mut self) -> Option<&mut CommandPaletteState> {
        self.command_palette.as_mut()
    }

    pub(crate) fn open_command_palette(&mut self) {
        self.restore_palette_draft();
        self.command_palette = Some(CommandPaletteState::new(
            self.slash.palette_entries(self.slash_command_state()),
        ));
    }

    pub(crate) fn close_command_palette(&mut self) {
        self.command_palette = None;
    }

    pub(crate) fn begin_palette_slash(&mut self, insert_text: String) {
        self.close_command_palette();
        self.restore_palette_draft();
        self.palette_stashed_submission = Some(self.take_submission());
        self.set_composer(insert_text);
        self.focus_prompt();
    }

    pub(crate) fn palette_draft_pending(&self) -> bool {
        self.palette_stashed_submission.is_some()
    }

    pub(crate) fn restore_palette_draft(&mut self) -> bool {
        let Some(submission) = self.palette_stashed_submission.take() else {
            return false;
        };
        self.restore_submission(submission);
        true
    }

    pub(crate) fn discard_palette_draft(&mut self) {
        self.palette_stashed_submission = None;
    }
}
