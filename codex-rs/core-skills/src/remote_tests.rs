use std::path::PathBuf;

use super::RemoteSkillProductSurface;
use super::RemoteSkillScope;
use super::export_remote_skill;
use super::list_remote_skills;

#[tokio::test]
async fn list_remote_skills_returns_control_plane_disabled_before_auth() {
    let err = list_remote_skills(
        "https://hosted.example/api".to_string(),
        /*auth*/ None,
        RemoteSkillScope::WorkspaceShared,
        RemoteSkillProductSurface::Codex,
        /*enabled*/ None,
    )
    .await
    .expect_err("hosted remote skills should be disabled");

    assert_eq!(
        err.to_string(),
        "legacy hosted remote skill control-plane is disabled in Astral"
    );
}

#[tokio::test]
async fn export_remote_skill_returns_control_plane_disabled_before_auth() {
    let err = export_remote_skill(
        "https://hosted.example/api".to_string(),
        PathBuf::from("/tmp/astral-disabled-skill-home"),
        /*auth*/ None,
        "skill_test",
    )
    .await
    .expect_err("hosted remote skill export should be disabled");

    assert_eq!(
        err.to_string(),
        "legacy hosted remote skill control-plane is disabled in Astral"
    );
}
