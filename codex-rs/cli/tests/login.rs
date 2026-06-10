use std::path::Path;

use anyhow::Result;
use predicates::str::contains;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

fn codex_command(codex_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("astral")?);
    cmd.env("ASTRAL_HOME", codex_home);
    Ok(cmd)
}

fn write_file_auth_config(codex_home: &Path) -> Result<()> {
    std::fs::write(
        codex_home.join("config.toml"),
        "cli_auth_credentials_store = \"file\"\n",
    )?;
    Ok(())
}

fn read_auth_json(codex_home: &Path) -> Result<Value> {
    let auth_json = std::fs::read_to_string(codex_home.join("auth.json"))?;
    Ok(serde_json::from_str(&auth_json)?)
}

fn write_chatgpt_auth_json(codex_home: &Path) -> Result<()> {
    let auth_json = json!({
        "tokens": {
            "id_token": "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9wbGFuX3R5cGUiOiJwcm8iLCJjaGF0Z3B0X3VzZXJfaWQiOiJ1c2VyLTEyMzQ1In19.c2ln",
            "access_token": "test-access-token",
            "refresh_token": "test-refresh-token"
        },
        "last_refresh": "2026-06-10T00:00:00Z"
    });
    std::fs::write(
        codex_home.join("auth.json"),
        serde_json::to_string_pretty(&auth_json)?,
    )?;
    Ok(())
}

#[test]
fn login_with_api_key_reads_stdin_and_writes_auth_json() -> Result<()> {
    let codex_home = TempDir::new()?;
    write_file_auth_config(codex_home.path())?;

    let mut cmd = codex_command(codex_home.path())?;
    cmd.args(["login", "--with-api-key"])
        .write_stdin("sk-test\n")
        .assert()
        .success()
        .stderr(contains("Successfully logged in"));

    let auth = read_auth_json(codex_home.path())?;
    assert_eq!(auth["ASTRAL_API_KEY"], "sk-test");
    assert!(auth.get("tokens").is_none());
    assert!(auth.get("agent_identity").is_none());

    Ok(())
}

#[test]
fn login_status_accepts_api_key_auth() -> Result<()> {
    let codex_home = TempDir::new()?;
    write_file_auth_config(codex_home.path())?;

    let mut login_cmd = codex_command(codex_home.path())?;
    login_cmd
        .args(["login", "--with-api-key"])
        .write_stdin("sk-test-1234567890\n")
        .assert()
        .success();

    let mut status_cmd = codex_command(codex_home.path())?;
    status_cmd
        .args(["login", "status"])
        .assert()
        .success()
        .stderr(contains("Logged in using an API key - sk-test-***67890"));

    Ok(())
}

#[test]
fn login_status_rejects_chatgpt_auth() -> Result<()> {
    let codex_home = TempDir::new()?;
    write_file_auth_config(codex_home.path())?;
    write_chatgpt_auth_json(codex_home.path())?;

    let mut cmd = codex_command(codex_home.path())?;
    cmd.args(["login", "status"])
        .assert()
        .failure()
        .stderr(contains(
            "Stored OpenAI/ChatGPT credentials are not supported by Astral",
        ));

    Ok(())
}

#[test]
fn login_without_flags_rejects_chatgpt_flow() -> Result<()> {
    let codex_home = TempDir::new()?;
    write_file_auth_config(codex_home.path())?;

    let mut cmd = codex_command(codex_home.path())?;
    cmd.arg("login").assert().failure().stderr(contains(
        "Browser/device ChatGPT login is not available in Astral",
    ));

    Ok(())
}

#[test]
fn login_with_access_token_is_not_available() -> Result<()> {
    let codex_home = TempDir::new()?;
    write_file_auth_config(codex_home.path())?;

    let mut cmd = codex_command(codex_home.path())?;
    cmd.args(["login", "--with-access-token"])
        .write_stdin("not-a-jwt\n")
        .assert()
        .failure()
        .stderr(contains("Access token login is not available in Astral"));

    Ok(())
}

#[test]
fn login_with_device_auth_rejects_chatgpt_flow() -> Result<()> {
    let codex_home = TempDir::new()?;
    write_file_auth_config(codex_home.path())?;

    let mut cmd = codex_command(codex_home.path())?;
    cmd.args(["login", "--device-auth"])
        .assert()
        .failure()
        .stderr(contains(
            "Browser/device ChatGPT login is not available in Astral",
        ));

    Ok(())
}
