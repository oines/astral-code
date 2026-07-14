use super::*;
use crate::tools::handlers::AstralFileToolKind;
use crate::tools::handlers::astral_file_tools::FileReadStateStore;
use crate::tools::sandboxing::SandboxAttempt;
use codex_protocol::config_types::WindowsSandboxLevel;
use codex_protocol::models::AdditionalPermissionProfile;
use codex_protocol::models::FileSystemPermissions;
use codex_protocol::models::PermissionProfile;
use codex_protocol::permissions::FileSystemSandboxPolicy;
use codex_protocol::permissions::NetworkSandboxPolicy;
use codex_sandboxing::SandboxManager;
use codex_sandboxing::SandboxType;
use codex_sandboxing::policy_transforms::effective_file_system_sandbox_policy;
use codex_sandboxing::policy_transforms::effective_network_sandbox_policy;
use codex_utils_path_uri::PathUri;
use core_test_support::PathBufExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::Arc;

fn test_turn_environment(environment_id: &str) -> crate::session::turn_context::TurnEnvironment {
    crate::session::turn_context::TurnEnvironment::new(
        environment_id.to_string(),
        Arc::new(codex_exec_server::Environment::default_for_tests()),
        std::env::temp_dir().abs(),
        /*shell*/ None,
    )
}

fn test_request(
    cwd: &AbsolutePathBuf,
    additional_permissions: Option<AdditionalPermissionProfile>,
) -> AstralFileToolRequest {
    AstralFileToolRequest {
        kind: AstralFileToolKind::Read,
        arguments: json!({ "file_path": "file.txt" }).to_string(),
        approval_command: vec![
            "Read".to_string(),
            cwd.join("file.txt").display().to_string(),
        ],
        hook_input: json!({ "file_path": "file.txt" }),
        turn_environment: test_turn_environment(codex_exec_server::LOCAL_ENVIRONMENT_ID),
        cwd: cwd.clone(),
        environment_id: codex_exec_server::LOCAL_ENVIRONMENT_ID.to_string(),
        read_state: Arc::new(FileReadStateStore::default()),
        sandbox_permissions: SandboxPermissions::WithAdditionalPermissions,
        additional_permissions,
        permissions_preapproved: false,
        exec_approval_requirement: ExecApprovalRequirement::NeedsApproval {
            reason: None,
            proposed_execpolicy_amendment: None,
        },
    }
}

#[tokio::test]
async fn permission_request_payload_uses_astral_tool_name_and_input() {
    let runtime = AstralFileToolRuntime::new();
    let cwd = std::env::temp_dir().abs();
    let req = test_request(&cwd, None);

    let payload = runtime
        .permission_request_payload(&req)
        .expect("permission request payload");

    assert_eq!(payload.tool_name.name(), "Read");
    assert_eq!(payload.tool_name.matcher_aliases(), &[] as &[String]);
    assert_eq!(payload.tool_input, json!({ "file_path": "file.txt" }));
}

#[tokio::test]
async fn file_system_sandbox_context_uses_active_attempt() {
    let cwd = std::env::temp_dir().join("astral-file-runtime-cwd").abs();
    let extra_read = cwd.join("outside.txt");
    let additional_permissions = AdditionalPermissionProfile {
        file_system: Some(FileSystemPermissions::from_read_write_roots(
            Some(vec![extra_read]),
            Some(vec![]),
        )),
        ..Default::default()
    };
    let req = test_request(&cwd, Some(additional_permissions.clone()));
    let file_system_policy = FileSystemSandboxPolicy::read_only();
    let network_policy = NetworkSandboxPolicy::Restricted;
    let permissions =
        PermissionProfile::from_runtime_permissions(&file_system_policy, network_policy);
    let manager = SandboxManager::new();
    let sandbox_policy_cwd = PathUri::from_abs_path(&cwd);
    let attempt = SandboxAttempt {
        sandbox: SandboxType::MacosSeatbelt,
        permissions: &permissions,
        enforce_managed_network: false,
        manager: &manager,
        sandbox_cwd: &sandbox_policy_cwd,
        workspace_roots: &[],
        codex_linux_sandbox_exe: None,
        use_legacy_landlock: true,
        windows_sandbox_level: WindowsSandboxLevel::RestrictedToken,
        windows_sandbox_private_desktop: true,
        network_denial_cancellation_token: None,
    };

    let sandbox = AstralFileToolRuntime::file_system_sandbox_context_for_attempt(&req, &attempt)
        .expect("sandbox context");

    let expected_file_system_policy =
        effective_file_system_sandbox_policy(&file_system_policy, Some(&additional_permissions));
    let expected_network_policy =
        effective_network_sandbox_policy(network_policy, Some(&additional_permissions));
    let expected_permissions = PermissionProfile::from_runtime_permissions(
        &expected_file_system_policy,
        expected_network_policy,
    );
    assert_eq!(sandbox.permissions, expected_permissions);
    assert_eq!(sandbox.cwd, Some(PathUri::from_abs_path(&cwd)));
    assert_eq!(
        sandbox.windows_sandbox_level,
        WindowsSandboxLevel::RestrictedToken
    );
    assert_eq!(sandbox.windows_sandbox_private_desktop, true);
    assert_eq!(sandbox.use_legacy_landlock, true);
}

#[tokio::test]
async fn file_system_sandbox_context_does_not_merge_unapproved_permissions() {
    let cwd = std::env::temp_dir()
        .join("astral-file-runtime-unapproved")
        .abs();
    let extra_read = cwd.join("outside.txt");
    let additional_permissions = AdditionalPermissionProfile {
        file_system: Some(FileSystemPermissions::from_read_write_roots(
            Some(vec![extra_read]),
            Some(vec![]),
        )),
        ..Default::default()
    };
    let mut req = test_request(&cwd, Some(additional_permissions));
    req.exec_approval_requirement = ExecApprovalRequirement::Skip {
        bypass_sandbox: false,
        proposed_execpolicy_amendment: None,
    };
    let file_system_policy = FileSystemSandboxPolicy::read_only();
    let network_policy = NetworkSandboxPolicy::Restricted;
    let permissions =
        PermissionProfile::from_runtime_permissions(&file_system_policy, network_policy);
    let manager = SandboxManager::new();
    let sandbox_policy_cwd = PathUri::from_abs_path(&cwd);
    let attempt = SandboxAttempt {
        sandbox: SandboxType::MacosSeatbelt,
        permissions: &permissions,
        enforce_managed_network: false,
        manager: &manager,
        sandbox_cwd: &sandbox_policy_cwd,
        workspace_roots: &[],
        codex_linux_sandbox_exe: None,
        use_legacy_landlock: true,
        windows_sandbox_level: WindowsSandboxLevel::RestrictedToken,
        windows_sandbox_private_desktop: true,
        network_denial_cancellation_token: None,
    };

    let sandbox = AstralFileToolRuntime::file_system_sandbox_context_for_attempt(&req, &attempt)
        .expect("sandbox context");

    assert_eq!(sandbox.permissions, permissions);
}

#[tokio::test]
async fn no_sandbox_attempt_has_no_file_system_context() {
    let cwd = std::env::temp_dir()
        .join("astral-file-runtime-no-sandbox")
        .abs();
    let req = test_request(&cwd, None);
    let permissions = PermissionProfile::Disabled;
    let manager = SandboxManager::new();
    let sandbox_policy_cwd = PathUri::from_abs_path(&cwd);
    let attempt = SandboxAttempt {
        sandbox: SandboxType::None,
        permissions: &permissions,
        enforce_managed_network: false,
        manager: &manager,
        sandbox_cwd: &sandbox_policy_cwd,
        workspace_roots: &[],
        codex_linux_sandbox_exe: None,
        use_legacy_landlock: false,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
        windows_sandbox_private_desktop: false,
        network_denial_cancellation_token: None,
    };

    assert_eq!(
        AstralFileToolRuntime::file_system_sandbox_context_for_attempt(&req, &attempt),
        None
    );
}
