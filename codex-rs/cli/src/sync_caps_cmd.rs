use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use clap::Parser;
use codex_core::config::find_codex_home;
use codex_login::default_client::build_reqwest_client;
use codex_models_manager::capabilities::LiteLlmModelHint;
use codex_models_manager::capabilities::MODEL_CAPABILITIES_FILE_NAME;
use codex_models_manager::capabilities::ModelCapabilitiesCache;

const LITELLM_REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

#[derive(Debug, Parser)]
pub(crate) struct SyncCapsCommand {
    /// Registry URL to fetch. Defaults to LiteLLM's public model registry.
    #[arg(long = "url", default_value = LITELLM_REGISTRY_URL)]
    registry_url: String,

    /// Output path. Defaults to $ASTRAL_HOME/model-capabilities.toml.
    #[arg(short = 'o', long = "output", value_name = "FILE")]
    output: Option<PathBuf>,

    /// Print the generated cache to stdout instead of writing it.
    #[arg(long = "dry-run", default_value_t = false)]
    dry_run: bool,
}

pub(crate) async fn run_sync_caps_command(cmd: SyncCapsCommand) -> anyhow::Result<()> {
    let registry = fetch_litellm_registry(&cmd.registry_url).await?;
    let generated_at_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| anyhow::anyhow!("system clock is before Unix epoch: {err}"))?
        .as_secs();
    let cache = ModelCapabilitiesCache::from_litellm_registry(
        cmd.registry_url.clone(),
        generated_at_unix_seconds,
        registry,
    );
    let body = toml::to_string_pretty(&cache)?;

    if cmd.dry_run {
        print!("{body}");
        return Ok(());
    }

    let output = match cmd.output {
        Some(output) => output,
        None => find_codex_home()?
            .join(MODEL_CAPABILITIES_FILE_NAME)
            .to_path_buf(),
    };
    if let Some(parent) = output.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&output, body).await?;
    println!(
        "Wrote {} model capability records to {}.",
        cache.models.len(),
        output.display()
    );
    Ok(())
}

async fn fetch_litellm_registry(
    registry_url: &str,
) -> anyhow::Result<BTreeMap<String, LiteLlmModelHint>> {
    let response = build_reqwest_client()
        .get(registry_url)
        .send()
        .await?
        .error_for_status()?;
    response
        .json::<BTreeMap<String, LiteLlmModelHint>>()
        .await
        .map_err(anyhow::Error::from)
}
