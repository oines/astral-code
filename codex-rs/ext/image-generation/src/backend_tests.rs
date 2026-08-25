use std::error::Error;
use std::path::Path;
use std::sync::Arc;

use codex_api::ImageBackground;
use codex_api::ImageEditRequest;
use codex_api::ImageGenerationRequest;
use codex_api::ImageQuality;
use codex_api::ImageUrl;
use codex_login::AuthCredentialsStoreMode;
use codex_login::AuthDotJson;
use codex_login::AuthManager;
use codex_login::TokenData;
use codex_login::save_codex_oauth_auth;
use codex_login::token_data::parse_chatgpt_jwt_claims;
use codex_model_provider::create_model_provider;
use codex_model_provider_info::ModelProviderInfo;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::header_regex;
use wiremock::matchers::method;
use wiremock::matchers::path;

use super::CodexImagesBackend;

#[tokio::test]
async fn image_backend_uses_codex_oauth_for_generation_and_edit() -> Result<(), Box<dyn Error>> {
    let server = MockServer::start().await;
    for endpoint in ["/images/generations", "/images/edits"] {
        Mock::given(method("POST"))
            .and(path(endpoint))
            .and(header_regex("Authorization", "Bearer codex-access"))
            .and(header_regex("ChatGPT-Account-ID", "workspace-123"))
            .and(header_regex("originator", "codex_cli_rs"))
            .and(header_regex("x-codex-image-turn-id", "turn-image"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "created": 1778832973u64,
                "data": [{"b64_json": "cG5n"}],
            })))
            .expect(1)
            .mount(&server)
            .await;
    }
    let auth_home = tempfile::tempdir()?;
    let auth_manager = codex_oauth_auth_manager(auth_home.path()).await?;
    let mut provider = ModelProviderInfo::create_codex_provider();
    provider.base_url = Some(server.uri());
    let backend = CodexImagesBackend::new(create_model_provider(provider, Some(auth_manager)));

    let generated = backend
        .generate(
            ImageGenerationRequest {
                prompt: "a red fox".to_string(),
                background: Some(ImageBackground::Opaque),
                model: "gpt-image-2".to_string(),
                n: None,
                quality: Some(ImageQuality::Medium),
                size: Some("1024x1024".to_string()),
            },
            "turn-image",
        )
        .await?;
    let edited = backend
        .edit(
            ImageEditRequest {
                images: vec![ImageUrl {
                    image_url: "data:image/png;base64,cG5n".to_string(),
                }],
                prompt: "add a blue hat".to_string(),
                background: None,
                model: "gpt-image-2".to_string(),
                n: None,
                quality: None,
                size: None,
            },
            "turn-image",
        )
        .await?;

    assert_eq!(generated, edited);
    assert_eq!(generated.data[0].b64_json, "cG5n");
    let requests = server
        .received_requests()
        .await
        .ok_or("mock server did not record image requests")?;
    assert_eq!(requests.len(), 2);
    let bodies = requests
        .iter()
        .map(|request| serde_json::from_slice::<Value>(&request.body))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        bodies,
        vec![
            json!({
                "prompt": "a red fox",
                "background": "opaque",
                "model": "gpt-image-2",
                "quality": "medium",
                "size": "1024x1024",
            }),
            json!({
                "images": [{"image_url": "data:image/png;base64,cG5n"}],
                "prompt": "add a blue hat",
                "model": "gpt-image-2",
            }),
        ]
    );
    Ok(())
}

async fn codex_oauth_auth_manager(home: &Path) -> Result<Arc<AuthManager>, Box<dyn Error>> {
    let id_token = concat!(
        "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.",
        "eyJlbWFpbCI6InVzZXJAZXhhbXBsZS5jb20iLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgi",
        "OnsiY2hhdGdwdF9hY2NvdW50X2lkIjoid29ya3NwYWNlLTEyMyIsImNoYXRncHRfcGxhbl90eXBl",
        "IjoicGx1cyJ9fQ.sig"
    );
    save_codex_oauth_auth(
        home,
        &AuthDotJson {
            auth_mode: Some("chatgpt".to_string()),
            api_key: None,
            tokens: Some(TokenData {
                id_token: parse_chatgpt_jwt_claims(id_token)?,
                access_token: "codex-access".to_string(),
                refresh_token: "codex-refresh".to_string(),
                account_id: Some("workspace-123".to_string()),
            }),
            last_refresh: None,
        },
        AuthCredentialsStoreMode::File,
    )?;
    Ok(Arc::new(
        AuthManager::new(
            home.to_path_buf(),
            /*enable_astral_api_key_env*/ false,
            AuthCredentialsStoreMode::File,
        )
        .await,
    ))
}
