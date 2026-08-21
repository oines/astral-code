use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use codex_login::AuthManager;
use codex_model_provider_info::ManagedAuthKind;
use codex_model_provider_info::ModelProviderInfo;
use codex_models_manager::ModelsManagerConfig;
use codex_models_manager::manager::RefreshStrategy;
use codex_models_manager::manager::SharedModelsManager;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ModelsResponse;

use crate::create_model_provider;

#[derive(Clone, Debug)]
struct RegistryEntry {
    provider_fingerprint: String,
    catalog_fingerprint: Option<String>,
    manager: SharedModelsManager,
}

/// Process-scoped model manager registry keyed by provider identity.
#[derive(Debug)]
pub struct ProviderModelsRegistry {
    codex_home: PathBuf,
    auth_manager: Arc<AuthManager>,
    managers: RwLock<HashMap<String, RegistryEntry>>,
}

impl ProviderModelsRegistry {
    pub fn new(codex_home: PathBuf, auth_manager: Arc<AuthManager>) -> Self {
        Self {
            codex_home,
            auth_manager,
            managers: RwLock::new(HashMap::new()),
        }
    }

    pub fn manager_for(
        &self,
        provider_id: &str,
        provider: &ModelProviderInfo,
        config_model_catalog: Option<ModelsResponse>,
    ) -> SharedModelsManager {
        let provider_fingerprint = self.provider_fingerprint(provider);
        let catalog_fingerprint = config_model_catalog.as_ref().map(catalog_fingerprint);
        if let Ok(managers) = self.managers.read()
            && let Some(entry) = managers.get(provider_id)
            && entry.provider_fingerprint == provider_fingerprint
            && catalog_fingerprint
                .as_ref()
                .is_none_or(|fingerprint| entry.catalog_fingerprint.as_ref() == Some(fingerprint))
        {
            return Arc::clone(&entry.manager);
        }

        let manager = create_model_provider(provider.clone(), Some(self.auth_manager.clone()))
            .models_manager(self.codex_home.clone(), config_model_catalog);
        if let Ok(mut managers) = self.managers.write() {
            managers.insert(
                provider_id.to_string(),
                RegistryEntry {
                    provider_fingerprint,
                    catalog_fingerprint,
                    manager: Arc::clone(&manager),
                },
            );
        }
        manager
    }

    pub async fn list_models_for(
        &self,
        provider_id: &str,
        provider: &ModelProviderInfo,
        config_model_catalog: Option<ModelsResponse>,
        refresh_strategy: RefreshStrategy,
    ) -> Vec<ModelPreset> {
        self.manager_for(provider_id, provider, config_model_catalog)
            .list_models(refresh_strategy)
            .await
    }

    pub async fn get_model_info_for(
        &self,
        provider_id: &str,
        provider: &ModelProviderInfo,
        config_model_catalog: Option<ModelsResponse>,
        model: &str,
        config: &ModelsManagerConfig,
    ) -> ModelInfo {
        self.manager_for(provider_id, provider, config_model_catalog)
            .get_model_info(model, config)
            .await
    }

    pub fn try_list_cached_models_for_all(&self) -> HashMap<String, Vec<ModelPreset>> {
        let Ok(managers) = self.managers.read() else {
            return HashMap::new();
        };
        managers
            .iter()
            .filter_map(|(provider_id, entry)| {
                entry
                    .manager
                    .try_list_models()
                    .ok()
                    .map(|models| (provider_id.clone(), models))
            })
            .collect()
    }

    pub fn invalidate_provider(&self, provider_id: &str) {
        if let Ok(mut managers) = self.managers.write() {
            managers.remove(provider_id);
        }
    }

    fn provider_fingerprint(&self, provider: &ModelProviderInfo) -> String {
        let auth_identity = if provider.managed_auth == Some(ManagedAuthKind::CodexOAuth) {
            self.auth_manager
                .codex_oauth_auth_cached()
                .and_then(|auth| auth.get_account_id())
                .map(|account_id| opaque_hash(&account_id))
                .unwrap_or_else(|| "signed-out".to_string())
        } else {
            "provider-auth".to_string()
        };
        format!(
            "base={};wire={};env={};command={};aws={};managed={:?};auth={auth_identity}",
            provider.base_url.as_deref().unwrap_or_default(),
            provider.wire_api,
            provider.env_key.as_deref().unwrap_or_default(),
            provider.auth.is_some(),
            provider.aws.is_some(),
            provider.managed_auth,
        )
    }
}

fn catalog_fingerprint(catalog: &ModelsResponse) -> String {
    opaque_hash(&format!("{catalog:?}"))
}

fn opaque_hash(value: &impl Hash) -> String {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
