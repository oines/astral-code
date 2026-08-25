use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::path::PathBuf;
use std::sync::Arc;

use codex_api::Provider;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_login::UnauthorizedRecovery;
use codex_model_provider_info::ModelProviderInfo;
use codex_models_manager::manager::OpenAiModelsManager;
use codex_models_manager::manager::SharedModelsManager;
use codex_models_manager::manager::StaticModelsManager;
use codex_protocol::account::ProviderAccount;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelsResponse;

use crate::auth::resolve_provider_auth;
use crate::models_endpoint::OpenAiModelsEndpoint;
use crate::provider::ModelProvider;
use crate::provider::ProviderAccountState;
use crate::provider::ProviderCapabilities;

/// Runtime provider for the reserved Codex OAuth backend.
#[derive(Clone, Debug)]
pub(crate) struct CodexModelProvider {
    info: ModelProviderInfo,
    auth_manager: Option<Arc<AuthManager>>,
}

impl CodexModelProvider {
    pub(crate) fn new(info: ModelProviderInfo, auth_manager: Option<Arc<AuthManager>>) -> Self {
        Self { info, auth_manager }
    }

    fn apply_catalog_defaults(model: &mut ModelInfo) {
        model.supports_web_search = true;
        model.supports_image_generation = model.input_modalities.contains(&InputModality::Image);
    }
}

#[async_trait::async_trait]
impl ModelProvider for CodexModelProvider {
    fn info(&self) -> &ModelProviderInfo {
        &self.info
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            namespace_tools: true,
            image_generation: true,
            web_search: true,
        }
    }

    fn auth_manager(&self) -> Option<Arc<AuthManager>> {
        self.auth_manager.clone()
    }

    fn unauthorized_recovery(&self) -> Option<UnauthorizedRecovery> {
        self.auth_manager
            .as_ref()
            .map(codex_login::AuthManager::codex_oauth_unauthorized_recovery)
    }

    async fn auth(&self) -> Option<CodexAuth> {
        match self.auth_manager.as_ref() {
            Some(manager) => manager.codex_oauth_auth().await,
            None => None,
        }
    }

    fn account_state(&self) -> ProviderAccountState {
        let account = self.auth_manager.as_ref().and_then(|manager| {
            let auth = manager.codex_oauth_auth_cached()?;
            if manager.refresh_failure_for_auth(&auth).is_some() {
                return None;
            }
            Some(ProviderAccount::Chatgpt {
                email: auth.account_email(),
                plan_type: auth.account_plan_type().unwrap_or_default(),
            })
        });
        ProviderAccountState {
            account,
            requires_astral_auth: false,
            requires_openai_auth: true,
        }
    }

    fn models_cache_identity(&self) -> Option<String> {
        Some(
            self.auth_manager
                .as_deref()
                .and_then(AuthManager::codex_oauth_auth_cached)
                .and_then(|auth| auth.get_account_id())
                .map(|account_id| opaque_hash(&account_id))
                .unwrap_or_else(|| "signed-out".to_string()),
        )
    }

    fn apply_model_capability_defaults(&self, model: &mut ModelInfo) {
        Self::apply_catalog_defaults(model);
    }

    async fn api_provider(&self) -> codex_protocol::error::Result<Provider> {
        let auth = self.auth().await;
        let mut info = self.info.clone();
        info.http_headers.get_or_insert_default().insert(
            "originator".to_string(),
            codex_login::default_client::codex_oauth_originator(),
        );
        info.to_api_provider(auth.as_ref().map(CodexAuth::auth_mode))
    }

    async fn api_auth(&self) -> codex_protocol::error::Result<codex_api::SharedAuthProvider> {
        let auth = self.auth().await;
        resolve_provider_auth(auth.as_ref(), &self.info)
    }

    fn models_manager(
        &self,
        codex_home: PathBuf,
        config_model_catalog: Option<ModelsResponse>,
    ) -> SharedModelsManager {
        if let Some(mut model_catalog) = config_model_catalog {
            for model in &mut model_catalog.models {
                Self::apply_catalog_defaults(model);
            }
            return Arc::new(StaticModelsManager::new(
                self.auth_manager.clone(),
                model_catalog,
            ));
        }

        let endpoint = Arc::new(OpenAiModelsEndpoint::new(Arc::new(self.clone())));
        Arc::new(OpenAiModelsManager::new(
            codex_home,
            endpoint,
            self.auth_manager.clone(),
        ))
    }
}

fn opaque_hash(value: &impl Hash) -> String {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
