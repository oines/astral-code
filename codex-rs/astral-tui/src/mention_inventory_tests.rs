use codex_app_server_protocol::PluginListResponse;
use codex_app_server_protocol::SkillsListResponse;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

#[test]
fn catalog_keeps_enabled_skills_and_usable_plugins() {
    let skills: SkillsListResponse = serde_json::from_value(json!({
        "data": [{
            "cwd": "/workspace",
            "skills": [
                {
                    "name": "review",
                    "description": "Review changes",
                    "shortDescription": null,
                    "interface": null,
                    "dependencies": null,
                    "path": "/workspace/.codex/skills/review/SKILL.md",
                    "scope": "repo",
                    "enabled": true
                },
                {
                    "name": "disabled",
                    "description": "Disabled skill",
                    "shortDescription": null,
                    "interface": null,
                    "dependencies": null,
                    "path": "/workspace/.codex/skills/disabled/SKILL.md",
                    "scope": "repo",
                    "enabled": false
                }
            ],
            "errors": []
        }]
    }))
    .expect("valid skills response");
    let plugins: PluginListResponse = serde_json::from_value(json!({
        "marketplaces": [{
            "name": "bundled",
            "path": null,
            "interface": null,
            "plugins": [
                {
                    "id": "browser-use@bundled",
                    "remotePluginId": null,
                    "localVersion": null,
                    "name": "Browser Use",
                    "shareContext": null,
                    "source": {"type": "remote"},
                    "installed": true,
                    "enabled": true,
                    "installPolicy": "AVAILABLE",
                    "authPolicy": "ON_INSTALL",
                    "availability": "AVAILABLE",
                    "interface": null,
                    "keywords": []
                },
                {
                    "id": "blocked@bundled",
                    "remotePluginId": null,
                    "localVersion": null,
                    "name": "Blocked",
                    "shareContext": null,
                    "source": {"type": "remote"},
                    "installed": true,
                    "enabled": true,
                    "installPolicy": "AVAILABLE",
                    "authPolicy": "ON_INSTALL",
                    "availability": "DISABLED_BY_ADMIN",
                    "interface": null,
                    "keywords": []
                }
            ]
        }],
        "marketplaceLoadErrors": [],
        "featuredPluginIds": []
    }))
    .expect("valid plugin response");

    let catalog = MentionCatalog::from_inventory(Some(&skills), Some(&plugins));

    assert_eq!(
        catalog
            .candidates
            .iter()
            .map(|candidate| candidate.insert_text.as_str())
            .collect::<Vec<_>>(),
        vec!["@Browser-Use", "$review"]
    );
}

#[test]
fn plugin_mention_name_matches_original_codex_casing_rules() {
    assert_eq!(
        plugin_mention_name("mcp-search", "MCP Search"),
        "MCP-Search"
    );
    assert_eq!(
        plugin_mention_name("browser-use", "Browser Use Plugin"),
        "Browser-Use"
    );
}
