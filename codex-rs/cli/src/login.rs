//! Provider-scoped Codex OAuth commands.

use anyhow::Context;
use codex_core::config::Config;
use codex_login::AuthManager;
use codex_login::CLIENT_ID;
use codex_login::ServerOptions;
use codex_login::load_codex_oauth_auth;
use codex_login::run_device_code_login;
use codex_login::run_login_server;
use codex_utils_cli::CliConfigOverrides;

pub async fn login_codex(
    cli_config_overrides: CliConfigOverrides,
    device_auth: bool,
) -> anyhow::Result<()> {
    let config = load_config(cli_config_overrides).await?;
    let options = ServerOptions::new(
        config.codex_home.to_path_buf(),
        CLIENT_ID.to_string(),
        /*forced_chatgpt_workspace_id*/ None,
        config.cli_auth_credentials_store_mode,
    );

    if device_auth {
        run_device_code_login(options)
            .await
            .context("Codex device authorization failed")?;
    } else {
        let server = run_login_server(options).context("failed to start Codex login server")?;
        eprintln!("Opening browser for Codex sign-in.");
        eprintln!("If it did not open, visit:\n{}", server.auth_url);
        server
            .block_until_done()
            .await
            .context("Codex browser sign-in failed")?;
    }

    eprintln!("Successfully logged in to Codex");
    Ok(())
}

pub async fn login_status(cli_config_overrides: CliConfigOverrides) -> ! {
    let config = match load_config(cli_config_overrides).await {
        Ok(config) => config,
        Err(err) => {
            eprintln!("Error checking Codex login status: {err}");
            std::process::exit(1);
        }
    };
    let auth =
        match load_codex_oauth_auth(&config.codex_home, config.cli_auth_credentials_store_mode)
            .context("failed to read Codex login")
        {
            Ok(Some(auth)) => auth,
            Ok(None) => {
                eprintln!("Not logged in to Codex");
                std::process::exit(1);
            }
            Err(err) => {
                eprintln!("Error checking Codex login status: {err}");
                std::process::exit(1);
            }
        };
    let Some(tokens) = auth.tokens else {
        eprintln!("Error checking Codex login status: stored login is missing OAuth tokens");
        std::process::exit(1);
    };
    let account_id = tokens
        .account_id
        .as_deref()
        .or(tokens.id_token.chatgpt_account_id.as_deref());
    eprintln!("Logged in to Codex");
    if let Some(email) = tokens.id_token.email.as_deref() {
        eprintln!("Email: {email}");
    }
    if let Some(plan) = tokens.id_token.get_chatgpt_plan_type() {
        eprintln!("Plan: {plan}");
    }
    if let Some(account_id) = account_id {
        eprintln!("Account: {}", redact_account_id(account_id));
    }
    std::process::exit(0)
}

pub async fn logout_codex(cli_config_overrides: CliConfigOverrides) -> anyhow::Result<()> {
    let config = load_config(cli_config_overrides).await?;
    let auth_manager =
        AuthManager::shared_from_config(&config, /*enable_astral_api_key_env*/ false).await;
    if auth_manager
        .logout_codex_oauth()
        .await
        .context("failed to log out of Codex")?
    {
        eprintln!("Successfully logged out of Codex");
    } else {
        eprintln!("Not logged in to Codex");
    }
    Ok(())
}

async fn load_config(cli_config_overrides: CliConfigOverrides) -> anyhow::Result<Config> {
    let overrides = cli_config_overrides
        .parse_overrides()
        .map_err(anyhow::Error::msg)?;
    Config::load_with_cli_overrides(overrides)
        .await
        .context("failed to load Astral configuration")
}

fn redact_account_id(account_id: &str) -> String {
    if account_id.len() <= 8 {
        return "***".to_string();
    }
    format!(
        "{}…{}",
        &account_id[..4],
        &account_id[account_id.len() - 4..]
    )
}
