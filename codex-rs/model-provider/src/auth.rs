use std::sync::Arc;

use codex_api::AuthProvider;
use codex_api::SharedAuthProvider;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_model_provider_info::ModelProviderInfo;
use http::HeaderMap;

use crate::bearer_auth_provider::BearerAuthProvider;

// Some providers are meant to send no auth headers. Examples include local OSS
// providers and custom test providers with `requires_astral_auth = false`.
#[derive(Clone, Debug)]
struct UnauthenticatedAuthProvider;

impl AuthProvider for UnauthenticatedAuthProvider {
    fn add_auth_headers(&self, _headers: &mut HeaderMap) {}
}

pub fn unauthenticated_auth_provider() -> SharedAuthProvider {
    Arc::new(UnauthenticatedAuthProvider)
}

/// Returns the provider-scoped auth manager for legacy managed-auth providers.
///
/// Provider-local API keys and unauthenticated providers intentionally do not
/// inherit the caller-supplied base manager.
pub(crate) fn auth_manager_for_provider(
    auth_manager: Option<Arc<AuthManager>>,
    provider: &ModelProviderInfo,
) -> Option<Arc<AuthManager>> {
    match provider.auth.clone() {
        Some(config) => Some(AuthManager::external_bearer_only(config)),
        None if provider.requires_astral_auth => auth_manager,
        None => None,
    }
}

pub(crate) fn resolve_provider_auth(
    auth: Option<&CodexAuth>,
    provider: &ModelProviderInfo,
) -> codex_protocol::error::Result<SharedAuthProvider> {
    if let Some(auth) = bearer_auth_for_provider(provider)? {
        return Ok(Arc::new(auth));
    }

    Ok(match auth {
        Some(auth) => auth_provider_from_auth(auth),
        None => unauthenticated_auth_provider(),
    })
}

fn bearer_auth_for_provider(
    provider: &ModelProviderInfo,
) -> codex_protocol::error::Result<Option<BearerAuthProvider>> {
    if let Some(api_key) = provider.api_key()? {
        return Ok(Some(BearerAuthProvider::new(api_key)));
    }

    if let Some(token) = provider.experimental_bearer_token.clone() {
        return Ok(Some(BearerAuthProvider::new(token)));
    }

    Ok(None)
}

/// Builds request-header auth for an Astral API-key auth snapshot.
pub fn auth_provider_from_auth(auth: &CodexAuth) -> SharedAuthProvider {
    Arc::new(BearerAuthProvider {
        token: auth.get_token().ok(),
        account_id: None,
        is_fedramp_account: false,
    })
}

#[cfg(test)]
mod tests {
    use codex_model_provider_info::WireApi;
    use codex_model_provider_info::create_oss_provider_with_base_url;

    use super::*;

    #[test]
    fn unauthenticated_auth_provider_adds_no_headers() {
        let provider = create_oss_provider_with_base_url(
            "http://localhost:11434/v1",
            WireApi::ChatCompletions,
        );
        let auth = resolve_provider_auth(/*auth*/ None, &provider).expect("auth should resolve");

        assert!(auth.to_auth_headers().is_empty());
    }
}
