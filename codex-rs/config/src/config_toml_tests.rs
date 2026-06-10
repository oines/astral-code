use super::validate_reserved_model_provider_ids;
use codex_model_provider_info::ModelProviderInfo;
use pretty_assertions::assert_eq;
use std::collections::HashMap;

#[test]
fn astral_provider_id_is_reserved() {
    let providers = HashMap::from([("astral".to_string(), ModelProviderInfo::default())]);

    assert_eq!(
        validate_reserved_model_provider_ids(&providers),
        Err(
            "model_providers contains reserved built-in provider IDs: `astral`. Built-in providers cannot be overridden. Rename your custom provider (for example, `provider-custom`)."
                .to_string()
        )
    );
}

#[test]
fn openai_provider_id_is_not_reserved() {
    let providers = HashMap::from([("openai".to_string(), ModelProviderInfo::default())]);

    assert_eq!(validate_reserved_model_provider_ids(&providers), Ok(()));
}
