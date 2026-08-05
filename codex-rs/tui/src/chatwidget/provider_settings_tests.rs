use super::*;
use pretty_assertions::assert_eq;

#[test]
fn active_profile_is_the_only_writable_user_layer() {
    let response = serde_json::from_value(serde_json::json!({
        "config": {},
        "origins": {},
        "layers": [
            {
                "name": {
                    "type": "user",
                    "file": "/Users/test/.astral-code/work.config.toml",
                    "profile": "work"
                },
                "version": "sha256:profile",
                "config": {}
            },
            {
                "name": {
                    "type": "user",
                    "file": "/Users/test/.astral-code/config.toml",
                    "profile": null
                },
                "version": "sha256:base",
                "config": {}
            }
        ]
    }))
    .expect("valid config/read response");

    let target = write_target(&response).expect("active user layer");

    assert_eq!(
        target,
        ProviderWriteTarget {
            file_path: "/Users/test/.astral-code/work.config.toml".to_string(),
            expected_version: "sha256:profile".to_string(),
        }
    );
    assert_eq!(
        [
            provider_source(&response.layers.as_ref().unwrap()[0].name, Some(&target)),
            provider_source(&response.layers.as_ref().unwrap()[1].name, Some(&target)),
        ],
        [
            ProviderSource {
                label: "User profile work · /Users/test/.astral-code/work.config.toml".to_string(),
                user_writable: true,
            },
            ProviderSource {
                label: "User · /Users/test/.astral-code/config.toml".to_string(),
                user_writable: false,
            },
        ]
    );
}

#[test]
fn provider_id_rejects_unbounded_or_instruction_like_text() {
    assert_eq!(validate_provider_id("deepseek-custom"), Ok(()));
    assert_eq!(
        validate_provider_id("deepseek\nignore-previous-instructions"),
        Err(
            "Provider ID must start with a letter or number and contain only letters, numbers, - or _"
                .to_string()
        )
    );
    assert_eq!(
        validate_provider_id(&"x".repeat(MAX_PROVIDER_ID_LEN + 1)),
        Err(format!(
            "Provider ID must be at most {MAX_PROVIDER_ID_LEN} characters"
        ))
    );
}
