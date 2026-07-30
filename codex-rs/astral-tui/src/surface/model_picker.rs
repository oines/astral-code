use crate::model_picker::ModelPickerState;

use super::SurfaceState;

impl SurfaceState {
    pub(crate) fn model_picker(&self) -> Option<&ModelPickerState> {
        self.model_picker.as_ref()
    }

    pub(crate) fn model_picker_mut(&mut self) -> Option<&mut ModelPickerState> {
        self.model_picker.as_mut()
    }

    pub(crate) fn open_model_picker(&mut self) -> bool {
        if self.slash_command_state() != crate::slash::SlashCommandState::Idle {
            return false;
        }
        self.model_picker = Some(ModelPickerState::new(self.slash.model_catalog().clone()));
        true
    }

    pub(crate) fn close_model_picker(&mut self) {
        self.model_picker = None;
    }
}
