pub(crate) mod cache;
pub mod collaboration_mode_presets;
pub(crate) mod config;
pub mod manager;
pub mod model_info;
pub mod model_presets;
pub mod test_support;

pub use codex_app_server_protocol::AuthMode;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ReasoningEffort;

pub use config::ModelsManagerConfig;

/// Load the bundled model catalog shipped with `codex-models-manager`.
pub fn bundled_models_response() -> std::result::Result<ModelsResponse, serde_json::Error> {
    let mut response: ModelsResponse = serde_json::from_str(include_str!("../models.json"))?;
    if !response
        .models
        .iter()
        .any(|model| model.slug == "deepseek-v4-flash")
        && let Some(mut flash) = response
            .models
            .iter()
            .find(|model| model.slug == "deepseek-v4-pro")
            .cloned()
    {
        flash.slug = "deepseek-v4-flash".to_string();
        flash.display_name = "DeepSeek V4 Flash".to_string();
        flash.description =
            Some("Fast Astral model for quick coding turns and smoke tests.".to_string());
        flash.default_reasoning_level = Some(ReasoningEffort::Low);
        flash.priority = 1;
        response.models.push(flash);
    }
    Ok(response)
}

/// Convert the client version string to a whole version string (e.g. "1.2.3-alpha.4" -> "1.2.3").
pub fn client_version_to_whole() -> String {
    format!(
        "{}.{}.{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    )
}
