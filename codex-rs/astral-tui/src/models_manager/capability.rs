use std::collections::BTreeSet;

use crossterm::event::KeyEvent;

use super::ModelsManagerInput;
use super::ModelsManagerState;
use super::capability_form::CapabilityFormState;

impl ModelsManagerState {
    pub(super) fn capability_form_active(&self) -> bool {
        self.capability_form.is_some()
    }

    pub(super) fn handle_capability_key(&mut self, key: KeyEvent) -> ModelsManagerInput {
        let target = self.write_target.clone();
        let existing_ids = self.model_ids_for_capability_form();
        self.capability_form
            .as_mut()
            .map(|form| form.handle_key(key, target, &existing_ids))
            .unwrap_or(ModelsManagerInput::None)
    }

    pub(super) fn handle_capability_paste(&mut self, text: &str) -> ModelsManagerInput {
        if self
            .capability_form
            .as_mut()
            .is_some_and(|form| form.handle_paste(text))
        {
            ModelsManagerInput::Redraw
        } else {
            ModelsManagerInput::None
        }
    }

    pub(super) fn activate_capability_field(&mut self, index: usize) -> ModelsManagerInput {
        let target = self.write_target.clone();
        let existing_ids = self.model_ids_for_capability_form();
        let Some(form) = self.capability_form.as_mut() else {
            return ModelsManagerInput::None;
        };
        form.activate_pointer(index, target, &existing_ids)
    }

    pub(super) fn move_capability_field(&mut self, delta: isize) {
        if let Some(form) = self.capability_form.as_mut() {
            form.move_selection(delta);
        }
    }

    pub(super) fn detail_can_edit(&self) -> bool {
        self.detail.as_ref().is_some_and(|model| {
            self.providers
                .iter()
                .find(|provider| provider.id == model.model_provider)
                .is_some_and(|provider| provider.editable)
        })
    }

    pub(super) fn activate_detail(&mut self) -> ModelsManagerInput {
        let Some(model) = self.detail.clone() else {
            return ModelsManagerInput::None;
        };
        let Some(provider) = self
            .providers
            .iter()
            .find(|provider| provider.id == model.model_provider && provider.editable)
        else {
            return ModelsManagerInput::None;
        };
        let model_key = format!("{}/{}", model.model_provider, model.model);
        let raw = self
            .manual_capabilities
            .get(&model_key)
            .cloned()
            .unwrap_or_default();
        self.capability_form = Some(CapabilityFormState::edit(
            provider.id.clone(),
            provider.name.clone(),
            model.model,
            raw,
            model.capabilities,
        ));
        self.pointer.clear_hover();
        ModelsManagerInput::Redraw
    }

    fn model_ids_for_capability_form(&self) -> BTreeSet<String> {
        let Some(form) = self.capability_form.as_ref() else {
            return BTreeSet::new();
        };
        self.providers
            .iter()
            .find(|provider| provider.id == form.provider_id())
            .map(|provider| {
                provider
                    .models
                    .iter()
                    .map(|model| model.model.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}
