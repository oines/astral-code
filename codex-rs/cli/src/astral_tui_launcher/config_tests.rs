use codex_config::LoaderOverrides;

use super::can_reuse_daemon;

#[test]
fn daemon_reuse_requires_replayable_config() {
    let default_loader = LoaderOverrides::default();
    assert!(can_reuse_daemon(
        &[],
        &default_loader,
        /*strict_config*/ false,
        /*bypass_hook_trust*/ false,
    ));

    let custom_loader = LoaderOverrides {
        ignore_user_config: true,
        ..LoaderOverrides::default()
    };
    assert!(!can_reuse_daemon(
        &[],
        &custom_loader,
        /*strict_config*/ false,
        /*bypass_hook_trust*/ false,
    ));
    assert!(!can_reuse_daemon(
        &[("model".to_string(), "gpt-test".into())],
        &default_loader,
        /*strict_config*/ false,
        /*bypass_hook_trust*/ false,
    ));
}
