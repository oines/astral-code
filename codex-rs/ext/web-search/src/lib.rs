mod codex_extension;
mod codex_tool;
mod extension;
mod fetch;
mod history;
mod output;
mod provider;
mod request;
mod schema;
mod tool;

use std::sync::Arc;

use codex_core::config::Config;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_login::AuthManager;

pub fn install(registry: &mut ExtensionRegistryBuilder<Config>, auth_manager: Arc<AuthManager>) {
    extension::install(registry, auth_manager.clone());
    codex_extension::install(registry, auth_manager);
}
