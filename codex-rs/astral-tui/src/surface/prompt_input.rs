use crate::PromptInputMode;
use crate::PromptSubmission;

use super::SurfaceState;

impl SurfaceState {
    pub(crate) fn prompt_input_mode(&self) -> PromptInputMode {
        self.prompt_input_mode
    }

    pub(crate) fn shell_input_mode(&self) -> bool {
        self.prompt_input_mode.is_shell()
    }

    pub(crate) fn enter_shell_input_mode(&mut self) -> bool {
        if self.prompt_input_mode != PromptInputMode::Normal
            || !self.composer.text().is_empty()
            || self.composer.has_structured_elements()
            || self.queue_editing()
            || self.plan_review.is_some()
        {
            return false;
        }
        self.prompt_input_mode = PromptInputMode::Shell;
        self.refresh_composer_completions();
        true
    }

    pub(crate) fn exit_shell_input_mode(&mut self) -> bool {
        if !self.prompt_input_mode.is_shell() {
            return false;
        }
        self.prompt_input_mode = PromptInputMode::Normal;
        self.refresh_composer_completions();
        true
    }

    pub(crate) fn take_shell_command(&mut self) -> Option<String> {
        let command = self.composer.text().trim().to_string();
        if command.is_empty() {
            return None;
        }
        self.composer.clear();
        self.prompt_input_mode = PromptInputMode::Normal;
        self.refresh_composer_completions();
        Some(command)
    }

    pub(crate) fn restore_shell_command(&mut self, command: String) {
        self.prompt_input_mode = PromptInputMode::Shell;
        self.composer
            .restore_submission(PromptSubmission::text_only(command));
        self.refresh_composer_completions();
    }
}
