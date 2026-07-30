use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::Model;

use crate::models_manager::ModelsManagerState;
use crate::models_manager::ProviderModelsRequest;

use super::SurfaceState;

impl SurfaceState {
    pub(crate) fn models_manager(&self) -> Option<&ModelsManagerState> {
        self.models_manager.as_ref()
    }

    pub(crate) fn models_manager_mut(&mut self) -> Option<&mut ModelsManagerState> {
        self.models_manager.as_mut()
    }

    pub(crate) fn open_models_manager(
        &mut self,
        config: ConfigReadResponse,
        models: Vec<Model>,
        current_provider: String,
        current_model: String,
    ) {
        self.models_manager_generation = self.models_manager_generation.saturating_add(1);
        self.models_manager = Some(ModelsManagerState::new(
            self.models_manager_generation,
            config,
            models,
            current_provider,
            current_model,
        ));
    }

    pub(crate) fn close_models_manager(&mut self) {
        self.models_manager = None;
    }

    pub(crate) fn take_provider_models_request(&mut self) -> Option<ProviderModelsRequest> {
        self.models_manager.as_mut()?.take_request()
    }

    pub(crate) fn apply_provider_models(
        &mut self,
        generation: u64,
        provider_id: &str,
        result: Result<Vec<Model>, String>,
    ) -> bool {
        let Some(manager) = self.models_manager.as_mut() else {
            return false;
        };
        if manager.generation() != generation {
            return false;
        }
        manager.apply_models(provider_id, result)
    }
}
