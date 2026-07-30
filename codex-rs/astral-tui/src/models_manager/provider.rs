use std::collections::BTreeSet;

use crossterm::event::KeyEvent;

use super::BrowserRow;
use super::ModelsManagerInput;
use super::ModelsManagerState;
use super::ProviderLoad;
use super::ProviderModelsRequest;

impl ModelsManagerState {
    pub(super) fn provider_form_active(&self) -> bool {
        self.provider_form.is_some()
    }

    pub(super) fn close_panel(&mut self) -> bool {
        let closed = self.capability_form.take().is_some()
            || self.provider_form.take().is_some()
            || self.detail.take().is_some();
        if closed {
            self.pointer.clear_hover();
        }
        closed
    }

    pub(super) fn handle_provider_key(&mut self, key: KeyEvent) -> ModelsManagerInput {
        let target = self.write_target.clone();
        let existing_ids = self.provider_ids();
        self.provider_form
            .as_mut()
            .map(|form| form.handle_key(key, target, &existing_ids))
            .unwrap_or(ModelsManagerInput::None)
    }

    pub(super) fn handle_provider_paste(&mut self, text: &str) -> ModelsManagerInput {
        if self
            .provider_form
            .as_mut()
            .is_some_and(|form| form.handle_paste(text))
        {
            ModelsManagerInput::Redraw
        } else {
            ModelsManagerInput::None
        }
    }

    pub(super) fn activate_provider_field(&mut self, index: usize) -> ModelsManagerInput {
        let target = self.write_target.clone();
        let existing_ids = self.provider_ids();
        let Some(form) = self.provider_form.as_mut() else {
            return ModelsManagerInput::None;
        };
        form.activate_pointer(index, target, &existing_ids)
    }

    pub(super) fn move_provider_field(&mut self, delta: isize) {
        if let Some(form) = self.provider_form.as_mut() {
            form.move_selection(delta);
        }
    }

    pub(crate) fn set_form_error(&mut self, error: String) {
        if let Some(form) = self.capability_form.as_mut() {
            form.set_error(error);
        } else if let Some(form) = self.provider_form.as_mut() {
            form.set_error(error);
        }
    }

    pub(crate) fn focus_provider(&mut self, provider_id: &str) {
        let Some(provider_index) = self
            .providers
            .iter()
            .position(|provider| provider.id == provider_id)
        else {
            return;
        };
        let provider = &mut self.providers[provider_index];
        provider.expanded = true;
        if matches!(
            provider.load,
            ProviderLoad::NotLoaded | ProviderLoad::Failed(_)
        ) {
            provider.load = ProviderLoad::Loading;
            self.pending_request = Some(ProviderModelsRequest {
                generation: self.generation,
                provider_id: provider.id.clone(),
            });
        }
        if let Some(row_index) = self.rows().iter().position(|row| {
            matches!(
                row,
                BrowserRow::Provider {
                    provider_index: index
                } if *index == provider_index
            )
        }) {
            self.selected = row_index;
            self.browser_scroll = super::BrowserScroll::FollowSelection;
            self.pointer.clear_hover();
        }
    }

    fn provider_ids(&self) -> BTreeSet<String> {
        self.providers
            .iter()
            .map(|provider| provider.id.clone())
            .collect()
    }
}
