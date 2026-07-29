//! External editor lifecycle for the Astral prompt.
//!
//! The terminal handoff follows the original Codex TUI while command
//! resolution and the `vi` fallback match Grok Build minimal mode.

use std::env;
use std::fs;
use std::io::Read;
use std::process::Stdio;

use tempfile::Builder;

const MAX_DRAFT_BYTES: u64 = 4 * 1024 * 1024;

pub(super) async fn edit(seed: String) -> Result<String, String> {
    let editor = resolve_editor_command()?;
    tokio::task::spawn_blocking(move || run_editor(&seed, &editor))
        .await
        .map_err(|error| format!("external editor task failed: {error}"))?
}

fn resolve_editor_command() -> Result<Vec<String>, String> {
    let raw = env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("EDITOR")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| "vi".to_string());
    let parts = {
        #[cfg(windows)]
        {
            winsplit::split(&raw)
        }
        #[cfg(not(windows))]
        {
            shlex::split(&raw).ok_or_else(|| "could not parse $VISUAL or $EDITOR".to_string())?
        }
    };
    if parts.first().is_none_or(String::is_empty) {
        return Err("editor command is empty".to_string());
    }
    Ok(parts)
}

fn run_editor(seed: &str, editor: &[String]) -> Result<String, String> {
    let path = Builder::new()
        .prefix("astral-prompt-")
        .suffix(".md")
        .tempfile()
        .map_err(|error| format!("could not create editor draft: {error}"))?
        .into_temp_path();
    fs::write(&path, seed).map_err(|error| format!("could not write editor draft: {error}"))?;

    let mut command = {
        #[cfg(windows)]
        {
            let program =
                which::which(&editor[0]).unwrap_or_else(|_| std::path::PathBuf::from(&editor[0]));
            std::process::Command::new(program)
        }
        #[cfg(not(windows))]
        {
            std::process::Command::new(&editor[0])
        }
    };
    command
        .args(&editor[1..])
        .arg(&path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let status = command
        .status()
        .map_err(|error| format!("could not run external editor: {error}"))?;
    if !status.success() {
        return Err(format!("external editor exited with status {status}"));
    }

    let file =
        fs::File::open(&path).map_err(|error| format!("could not read editor draft: {error}"))?;
    if file
        .metadata()
        .map_err(|error| format!("could not inspect editor draft: {error}"))?
        .len()
        > MAX_DRAFT_BYTES
    {
        return Err("external editor saved a draft larger than 4 MiB".to_string());
    }
    let mut bytes = Vec::new();
    file.take(MAX_DRAFT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("could not read editor draft: {error}"))?;
    if bytes.len() as u64 > MAX_DRAFT_BYTES {
        return Err("external editor saved a draft larger than 4 MiB".to_string());
    }
    String::from_utf8(bytes)
        .map_err(|_| "external editor saved a draft that is not valid UTF-8".to_string())
}
