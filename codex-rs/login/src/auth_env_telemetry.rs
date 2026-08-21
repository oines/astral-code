use codex_model_provider_info::ModelProviderInfo;
use codex_otel::AuthEnvTelemetryMetadata;

use crate::ASTRAL_API_KEY_ENV_VAR;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthEnvTelemetry {
    pub astral_api_key_env_present: bool,
    pub astral_api_key_env_enabled: bool,
    pub provider_env_key_name: Option<String>,
    pub provider_env_key_present: Option<bool>,
    pub refresh_token_url_override_present: bool,
}

impl AuthEnvTelemetry {
    pub fn to_otel_metadata(&self) -> AuthEnvTelemetryMetadata {
        AuthEnvTelemetryMetadata {
            astral_api_key_env_present: self.astral_api_key_env_present,
            astral_api_key_env_enabled: self.astral_api_key_env_enabled,
            provider_env_key_name: self.provider_env_key_name.clone(),
            provider_env_key_present: self.provider_env_key_present,
            refresh_token_url_override_present: self.refresh_token_url_override_present,
        }
    }
}

pub fn collect_auth_env_telemetry(
    provider: &ModelProviderInfo,
    astral_api_key_env_enabled: bool,
) -> AuthEnvTelemetry {
    AuthEnvTelemetry {
        astral_api_key_env_present: env_var_present(ASTRAL_API_KEY_ENV_VAR),
        astral_api_key_env_enabled,
        provider_env_key_name: provider.env_key.as_ref().map(|_| "configured".to_string()),
        provider_env_key_present: provider.env_key.as_deref().map(env_var_present),
        refresh_token_url_override_present: false,
    }
}

fn env_var_present(name: &str) -> bool {
    match std::env::var(name) {
        Ok(value) => !value.trim().is_empty(),
        Err(std::env::VarError::NotUnicode(_)) => true,
        Err(std::env::VarError::NotPresent) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_model_provider_info::WireApi;
    use pretty_assertions::assert_eq;

    #[test]
    fn collect_auth_env_telemetry_buckets_provider_env_key_name() {
        let provider = ModelProviderInfo {
            name: "Custom".to_string(),
            base_url: None,
            env_key: Some("sk-should-not-leak".to_string()),
            env_key_instructions: None,
            experimental_bearer_token: None,
            auth: None,
            aws: None,
            managed_auth: None,
            wire_api: WireApi::ChatCompletions,
            responses_builtin_tools: Default::default(),
            provider_flavor: None,
            query_params: None,
            request_body: None,
            request_body_remove: Vec::new(),
            http_headers: None,
            env_http_headers: None,
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_astral_auth: false,
            supports_websockets: false,
        };

        let telemetry =
            collect_auth_env_telemetry(&provider, /*astral_api_key_env_enabled*/ false);

        assert_eq!(
            telemetry.provider_env_key_name,
            Some("configured".to_string())
        );
    }
}
