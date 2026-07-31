use codex_app_server_protocol::Model;

use crate::models_manager::ProviderModelsRequest;
use crate::settings::SettingsData;
use crate::settings::SettingsFocus;
use crate::settings::SettingsState;

use super::SurfaceState;

impl SurfaceState {
    pub(crate) fn settings(&self) -> Option<&SettingsState> {
        self.settings.as_ref()
    }

    pub(crate) fn settings_mut(&mut self) -> Option<&mut SettingsState> {
        self.settings.as_mut()
    }

    pub(crate) fn open_settings(
        &mut self,
        data: SettingsData,
        current_provider: String,
        current_model: String,
        focus: SettingsFocus,
    ) {
        self.settings_generation = self.settings_generation.saturating_add(1);
        let mut settings = SettingsState::new(
            self.settings_generation,
            data,
            current_provider,
            current_model,
            self.theme,
        );
        settings.apply_focus(focus);
        self.settings = Some(settings);
    }

    pub(crate) fn close_settings(&mut self) {
        self.settings = None;
    }

    pub(crate) fn take_provider_models_request(&mut self) -> Option<ProviderModelsRequest> {
        self.settings.as_mut()?.models_mut().take_request()
    }

    pub(crate) fn apply_provider_models(
        &mut self,
        generation: u64,
        provider_id: &str,
        result: Result<Vec<Model>, String>,
    ) -> bool {
        let Some(settings) = self.settings.as_mut() else {
            return false;
        };
        if settings.generation() != generation {
            return false;
        }
        settings.models_mut().apply_models(provider_id, result)
    }
}
