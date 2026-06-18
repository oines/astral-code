#![allow(clippy::expect_used)]
use codex_login::ASTRAL_API_KEY_ENV_VAR;
use codex_model_provider_info::ASTRAL_BASE_URL_ENV_VAR;
use codex_protocol::openai_models::ApplyPatchToolType;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ModelsResponse;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use tempfile::TempDir;
use wiremock::MockServer;

pub const TEST_MODEL: &str = "astral-test-model";

pub struct TestCodexExecBuilder {
    home: TempDir,
    cwd: TempDir,
}

impl TestCodexExecBuilder {
    pub fn cmd(&self) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::new(
            codex_utils_cargo_bin::cargo_bin("codex-exec")
                .expect("should find binary for codex-exec"),
        );
        cmd.current_dir(self.cwd.path())
            .env("ASTRAL_HOME", self.home.path())
            .env("ASTRAL_SQLITE_HOME", self.home.path())
            .env(ASTRAL_API_KEY_ENV_VAR, "dummy");
        cmd
    }
    pub fn cmd_with_server(&self, server: &MockServer) -> assert_cmd::Command {
        let mut cmd = self.cmd();
        let base = format!("{}/v1", server.uri());
        cmd.env(ASTRAL_BASE_URL_ENV_VAR, base)
            .arg("-c")
            .arg(format!("model={}", toml_string_literal(TEST_MODEL)));
        cmd.arg("-c").arg(format!(
            "model_catalog_json={}",
            toml_string_literal(&self.write_test_model_catalog().display().to_string())
        ));
        cmd
    }

    pub fn cwd_path(&self) -> &Path {
        self.cwd.path()
    }
    pub fn home_path(&self) -> &Path {
        self.home.path()
    }

    fn write_test_model_catalog(&self) -> PathBuf {
        let catalog_path = self.home.path().join("exec-test-models.json");
        let response = exec_test_model_catalog();
        let contents =
            serde_json::to_vec(&response).expect("test model catalog should serialize to JSON");
        fs::write(&catalog_path, contents).expect("write exec test model catalog");
        catalog_path
    }
}

pub fn exec_test_model_catalog() -> ModelsResponse {
    let mut response =
        codex_models_manager::bundled_models_response().expect("bundled models.json should parse");
    let base_model = response
        .models
        .iter()
        .find(|model| model.slug == "gpt-5.2")
        .cloned()
        .expect("bundled models.json should contain gpt-5.2");
    response.models = [
        TEST_MODEL,
        "mock-model",
        "mock-model-collab",
        "mock-model-override",
        "mock-model-3",
        "mock-model-4",
        "gpt-5.2",
        "gpt-5.3-codex",
    ]
    .into_iter()
    .map(|slug| {
        let mut model = base_model.clone();
        model.slug = slug.to_string();
        model.display_name = slug.to_string();
        model.apply_patch_tool_type = Some(ApplyPatchToolType::Freeform);
        model.input_modalities = vec![InputModality::Text, InputModality::Image];
        model
    })
    .collect();
    response
}

fn toml_string_literal(value: &str) -> String {
    serde_json::to_string(value).expect("serialize TOML string literal")
}

pub fn test_codex_exec() -> TestCodexExecBuilder {
    TestCodexExecBuilder {
        home: TempDir::new().expect("create temp home"),
        cwd: TempDir::new().expect("create temp cwd"),
    }
}
