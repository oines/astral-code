use codex_app_server_protocol::AppInfo;

pub fn connector_display_label(connector: &AppInfo) -> String {
    connector.name.clone()
}

pub fn connector_mention_slug(connector: &AppInfo) -> String {
    connector_mention_slug_from_name(&connector_display_label(connector))
}

pub fn connector_mention_slug_from_name(name: &str) -> String {
    connector_name_slug(name)
}

pub fn sanitize_name(name: &str) -> String {
    connector_name_slug(name).replace("-", "_")
}

fn connector_name_slug(name: &str) -> String {
    let mut normalized = String::with_capacity(name.len());
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            normalized.push(character.to_ascii_lowercase());
        } else {
            normalized.push('-');
        }
    }
    let normalized = normalized.trim_matches('-');
    if normalized.is_empty() {
        "app".to_string()
    } else {
        normalized.to_string()
    }
}

pub(crate) fn sort_connectors_by_accessibility_and_name(connectors: &mut [AppInfo]) {
    connectors.sort_by(|left, right| {
        right
            .is_accessible
            .cmp(&left.is_accessible)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.id.cmp(&right.id))
    });
}
