//! App-server-backed orchestration for custom-provider settings.

use super::*;
use crate::chatwidget::provider_settings::ProviderSettingsAction;
use codex_app_server_protocol::WriteStatus;

impl App {
    pub(super) async fn handle_provider_settings_action(
        &mut self,
        app_server: &mut AppServerSession,
        action: ProviderSettingsAction,
    ) {
        match action {
            ProviderSettingsAction::OpenModelPicker => self.chat_widget.open_model_popup(),
            ProviderSettingsAction::OpenList => {
                self.open_custom_provider_settings(app_server).await;
            }
            ProviderSettingsAction::Edit(draft) => {
                self.chat_widget.open_provider_editor(draft);
            }
            ProviderSettingsAction::EditText { draft, field } => {
                self.chat_widget.open_provider_text_prompt(draft, field);
            }
            ProviderSettingsAction::EditWire(draft) => {
                self.chat_widget.open_provider_wire_picker(draft);
            }
            ProviderSettingsAction::Save(draft) => {
                let (edit, file_path, expected_version) = match draft.config_write() {
                    Ok(write) => write,
                    Err(error) => {
                        self.chat_widget
                            .open_provider_editor(draft.with_error(error));
                        return;
                    }
                };
                match crate::config_update::write_config_batch_to_file(
                    app_server.request_handle(),
                    vec![edit],
                    file_path,
                    expected_version,
                )
                .await
                {
                    Ok(response) => {
                        if response.status == WriteStatus::OkOverridden {
                            let message = response
                                .overridden_metadata
                                .as_ref()
                                .map(|metadata| metadata.message.as_str())
                                .unwrap_or("a higher-priority config layer overrides it");
                            self.chat_widget.add_error_message(format!(
                                "Provider was saved but is not effective: {message}"
                            ));
                        } else {
                            self.chat_widget.add_info_message(
                                "Custom provider saved.".to_string(),
                                /*hint*/ None,
                            );
                        }
                        self.refresh_in_memory_config_from_disk_best_effort(
                            "saving a custom provider",
                        )
                        .await;
                        self.open_custom_provider_settings(app_server).await;
                    }
                    Err(err) => {
                        let error = crate::config_update::format_config_error(&err);
                        self.chat_widget
                            .open_provider_editor(draft.with_error(error));
                    }
                }
            }
        }
    }

    async fn open_custom_provider_settings(&mut self, app_server: &mut AppServerSession) {
        let cwd = self.chat_widget.config_ref().cwd.display().to_string();
        match crate::config_update::read_effective_config_with_layers(
            app_server.request_handle(),
            cwd,
        )
        .await
        {
            Ok(response) => self.chat_widget.open_custom_providers_popup(response),
            Err(err) => self.chat_widget.add_error_message(format!(
                "Failed to load custom providers: {}",
                crate::config_update::format_config_error(&err)
            )),
        }
    }
}
