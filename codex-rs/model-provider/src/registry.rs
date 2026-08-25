use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use codex_login::AuthManager;
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
        let runtime_provider =
            create_model_provider(provider.clone(), Some(self.auth_manager.clone()));
        let provider_fingerprint = provider_fingerprint(runtime_provider.as_ref());
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

        let manager =
            runtime_provider.models_manager(self.codex_home.clone(), config_model_catalog);
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
}

fn provider_fingerprint(provider: &dyn crate::ModelProvider) -> String {
    let info = provider.info();
    let auth_identity = provider
        .models_cache_identity()
        .unwrap_or_else(|| "provider-auth".to_string());
    format!(
        "base={};wire={};env={};command={};aws={};managed={:?};auth={auth_identity}",
        info.base_url.as_deref().unwrap_or_default(),
        info.wire_api,
        info.env_key.as_deref().unwrap_or_default(),
        info.auth.is_some(),
        info.aws.is_some(),
        info.managed_auth,
    )
}

fn catalog_fingerprint(catalog: &ModelsResponse) -> String {
    opaque_hash(&format!("{catalog:?}"))
}

fn opaque_hash(value: &impl Hash) -> String {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
