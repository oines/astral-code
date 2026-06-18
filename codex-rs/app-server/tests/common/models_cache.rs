use chrono::DateTime;
use chrono::Utc;
use codex_core::test_support::all_model_presets;
use codex_models_manager::client_version_to_whole;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelInstructionsVariables;
use codex_protocol::openai_models::ModelMessages;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ModelVisibility;
use codex_protocol::openai_models::TruncationPolicyConfig;
use codex_protocol::openai_models::default_input_modalities;
use serde_json::json;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::path::Path;
use std::path::PathBuf;

const LEGACY_MODELS_CACHE_FILE: &str = "models_cache.json";
const MODELS_CACHE_DIR: &str = "models_cache";

/// Convert a ModelPreset to ModelInfo for cache storage.
fn preset_to_info(preset: &ModelPreset, priority: i32) -> ModelInfo {
    ModelInfo {
        slug: preset.id.clone(),
        display_name: preset.display_name.clone(),
        description: Some(preset.description.clone()),
        default_reasoning_level: Some(preset.default_reasoning_effort.clone()),
        supported_reasoning_levels: preset.supported_reasoning_efforts.clone(),
        shell_type: ConfigShellToolType::ShellCommand,
        visibility: if preset.show_in_picker {
            ModelVisibility::List
        } else {
            ModelVisibility::Hide
        },
        supported_in_api: preset.supported_in_api,
        priority,
        additional_speed_tiers: preset.additional_speed_tiers.clone(),
        service_tiers: preset.service_tiers.clone(),
        default_service_tier: preset.default_service_tier.clone(),
        upgrade: preset.upgrade.as_ref().map(Into::into),
        base_instructions: "base instructions".to_string(),
        model_messages: Some(test_model_messages()),
        supports_reasoning_summaries: false,
        default_reasoning_summary: ReasoningSummary::Auto,
        support_verbosity: false,
        default_verbosity: None,
        availability_nux: preset.availability_nux.clone(),
        apply_patch_tool_type: None,
        web_search_tool_type: Default::default(),
        truncation_policy: TruncationPolicyConfig::bytes(/*limit*/ 10_000),
        supports_parallel_tool_calls: false,
        supports_image_detail_original: false,
        context_window: Some(272_000),
        max_context_window: None,
        auto_compact_token_limit: None,
        effective_context_window_percent: 95,
        experimental_supported_tools: Vec::new(),
        input_modalities: default_input_modalities(),
        used_fallback_model_metadata: false,
        supports_search_tool: false,
        use_responses_lite: false,
        auto_review_model_override: None,
        tool_mode: None,
        multi_agent_version: None,
    }
}

fn test_model_messages() -> ModelMessages {
    ModelMessages {
        instructions_template: Some("base instructions\n{{ personality }}".to_string()),
        instructions_variables: Some(ModelInstructionsVariables {
            personality_default: Some(String::new()),
            personality_friendly: Some("You are a patient and enjoyable collaborator.".to_string()),
            personality_pragmatic: Some(
                "You are a deeply pragmatic, effective software engineer.".to_string(),
            ),
        }),
    }
}

/// Write a models cache file to the codex home directory.
/// This prevents ModelsManager from making network requests to refresh models.
/// The cache will be treated as fresh (within TTL) and used instead of fetching from the network.
/// Uses bundled-catalog-derived presets, converted to ModelInfo format.
pub fn write_models_cache(codex_home: &Path) -> std::io::Result<()> {
    // Get a stable bundled-catalog-derived preset list and filter for picker-visible entries.
    let presets: Vec<&ModelPreset> = all_model_presets()
        .iter()
        .filter(|preset| preset.show_in_picker)
        .collect();
    // Convert presets to ModelInfo, assigning priorities (lower = earlier in list).
    // Priority is used for sorting, so the first model gets the lowest priority.
    let models: Vec<ModelInfo> = presets
        .iter()
        .enumerate()
        .map(|(idx, preset)| {
            // Lower priority = earlier in list.
            let priority = idx as i32;
            preset_to_info(preset, priority)
        })
        .collect();

    write_models_cache_with_models(codex_home, models)
}

/// Write a models cache file with specific models.
/// Useful when tests need specific models to be available.
pub fn write_models_cache_with_models(
    codex_home: &Path,
    models: Vec<ModelInfo>,
) -> std::io::Result<()> {
    // DateTime<Utc> serializes to RFC3339 format by default with serde
    let fetched_at: DateTime<Utc> = Utc::now();
    let client_version = client_version_to_whole();
    let cache = json!({
        "fetched_at": fetched_at,
        "etag": null,
        "client_version": client_version,
        "models": models
    });
    let contents = serde_json::to_vec_pretty(&cache)?;
    let legacy_path = codex_home.join(LEGACY_MODELS_CACHE_FILE);

    let Some(runtime_path) = runtime_models_cache_path(codex_home)? else {
        return std::fs::write(legacy_path, contents);
    };

    if let Some(parent) = runtime_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&runtime_path, &contents)?;

    let _ = std::fs::remove_file(&legacy_path);
    if std::fs::hard_link(&runtime_path, &legacy_path).is_err() {
        std::fs::write(legacy_path, contents)?;
    }

    Ok(())
}

fn runtime_models_cache_path(codex_home: &Path) -> std::io::Result<Option<PathBuf>> {
    let config_path = codex_home.join("config.toml");
    let config_toml = match std::fs::read_to_string(config_path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let Some(cache_key) = provider_cache_key(&config_toml) else {
        return Ok(None);
    };

    let mut hasher = DefaultHasher::new();
    cache_key.hash(&mut hasher);
    Ok(Some(
        codex_home
            .join(MODELS_CACHE_DIR)
            .join(format!("{:016x}.json", hasher.finish())),
    ))
}

fn provider_cache_key(config_toml: &str) -> Option<String> {
    let provider_id = read_top_level_string(config_toml, "model_provider")?;
    let provider_block = read_table_block(config_toml, &format!("model_providers.{provider_id}"))?;
    let name = read_block_string(provider_block, "name").unwrap_or_default();
    let base_url = read_block_string(provider_block, "base_url").unwrap_or_default();
    let wire_api = read_block_string(provider_block, "wire_api").unwrap_or_default();
    let env_key = read_block_string(provider_block, "env_key").unwrap_or_default();
    Some(format!(
        "name={name};base_url={base_url};wire_api={wire_api};env_key={env_key};auth=false;aws=false"
    ))
}

fn read_top_level_string(contents: &str, key: &str) -> Option<String> {
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            return None;
        }
        if let Some(value) = read_key_value(trimmed, key) {
            return Some(value);
        }
    }
    None
}

fn read_table_block<'a>(contents: &'a str, table: &str) -> Option<&'a str> {
    let header = format!("[{table}]");
    let mut start = None;
    let mut offset = 0;

    for line in contents.split_inclusive('\n') {
        let line_start = offset;
        let line_end = line_start + line.len();
        let trimmed = line.trim();

        if trimmed == header {
            start = Some(line_end);
        } else if start.is_some() && trimmed.starts_with('[') {
            return start.map(|start| &contents[start..line_start]);
        }

        offset = line_end;
    }

    start.map(|start| &contents[start..])
}

fn read_block_string(block: &str, key: &str) -> Option<String> {
    block
        .lines()
        .find_map(|line| read_key_value(line.trim(), key))
}

fn read_key_value(line: &str, key: &str) -> Option<String> {
    let (lhs, rhs) = line.split_once('=')?;
    if lhs.trim() != key {
        return None;
    }
    Some(rhs.trim().trim_matches('"').to_string())
}
