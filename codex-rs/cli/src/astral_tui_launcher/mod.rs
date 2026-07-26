use std::io;

use astral_tui::LaunchOptions;
use astral_tui::RunExitReason;
use astral_tui::RunOptions;
use codex_app_server_client::RemoteAppServerEndpoint;
use codex_app_server_protocol::UserInput;
use codex_arg0::Arg0DispatchPaths;
use codex_config::LoaderOverrides;
use codex_config::types::UiVariant;
use codex_protocol::ThreadId;
use codex_tui::AppExitInfo;
use codex_tui::Cli;
use codex_tui::ExitReason;
use codex_tui::TokenUsage;

mod config;
mod thread;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub(crate) async fn run_main(
    cli: Cli,
    arg0_paths: Arg0DispatchPaths,
    loader_overrides: LoaderOverrides,
    explicit_remote_endpoint: Option<RemoteAppServerEndpoint>,
    explicit_ui: Option<UiVariant>,
) -> io::Result<AppExitInfo> {
    if explicit_ui == Some(UiVariant::Classic) {
        return codex_tui::run_main(cli, arg0_paths, loader_overrides, explicit_remote_endpoint)
            .await;
    }
    let prompt = cli.prompt.clone();
    let images = cli.images.clone();
    let prepared = config::prepare_launch(
        &cli,
        arg0_paths.clone(),
        loader_overrides.clone(),
        explicit_remote_endpoint.clone(),
    )
    .await?;
    if selected_ui_variant(explicit_ui, prepared.configured_ui()) == UiVariant::Classic {
        return codex_tui::run_main(cli, arg0_paths, loader_overrides, explicit_remote_endpoint)
            .await;
    }
    let context = prepared.start().await?;
    let thread = thread::resolve_launch(
        &context.client,
        &cli,
        context.config.as_ref(),
        context.target,
    )
    .await?;
    let mut options = LaunchOptions::new(thread);
    options.initial_input = initial_input(prompt, images);
    options.runtime = RunOptions::default();

    let exit = astral_tui::run_main(context.client, options)
        .await
        .map_err(io::Error::other)?;
    let thread_id = ThreadId::from_string(&exit.thread_id).ok();
    let exit_reason = match exit.reason {
        RunExitReason::UserRequested => ExitReason::UserRequested,
        RunExitReason::Disconnected => {
            ExitReason::Fatal("Astral app-server disconnected".to_string())
        }
    };
    Ok(AppExitInfo {
        token_usage: TokenUsage::default(),
        thread_id,
        thread_name: None,
        update_action: None,
        exit_reason,
    })
}

fn selected_ui_variant(explicit: Option<UiVariant>, configured: UiVariant) -> UiVariant {
    explicit.unwrap_or(configured)
}

fn initial_input(prompt: Option<String>, images: Vec<std::path::PathBuf>) -> Vec<UserInput> {
    let mut input = images
        .into_iter()
        .map(|path| UserInput::LocalImage { path, detail: None })
        .collect::<Vec<_>>();
    if let Some(text) = prompt {
        input.push(UserInput::Text {
            text,
            text_elements: Vec::new(),
        });
    }
    input
}
