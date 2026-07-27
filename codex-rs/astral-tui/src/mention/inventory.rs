use std::collections::HashSet;

use codex_app_server_protocol::PluginAvailability;
use codex_app_server_protocol::PluginListResponse;
use codex_app_server_protocol::SkillMetadata;
use codex_app_server_protocol::SkillsListResponse;

use crate::mention::MentionCandidate;
use crate::mention::MentionCatalog;
use crate::mention::MentionKind;
use crate::mention::MentionTarget;

impl MentionCatalog {
    pub(crate) fn from_inventory(
        skills: Option<&SkillsListResponse>,
        plugins: Option<&PluginListResponse>,
    ) -> Self {
        let mut candidates = Vec::new();
        if let Some(plugins) = plugins {
            for marketplace in &plugins.marketplaces {
                candidates.extend(
                    marketplace
                        .plugins
                        .iter()
                        .filter(|plugin| {
                            plugin.installed
                                && plugin.enabled
                                && plugin.availability != PluginAvailability::DisabledByAdmin
                        })
                        .map(|plugin| {
                            let display = plugin
                                .interface
                                .as_ref()
                                .and_then(|interface| interface.display_name.clone())
                                .unwrap_or_else(|| plugin.name.clone());
                            let description = plugin
                                .interface
                                .as_ref()
                                .and_then(|interface| interface.short_description.clone())
                                .unwrap_or_else(|| marketplace.name.clone());
                            let plugin_name = plugin
                                .id
                                .split_once('@')
                                .map_or(plugin.id.as_str(), |(name, _)| name);
                            let mention_name = plugin_mention_name(plugin_name, &display);
                            MentionCandidate {
                                kind: MentionKind::Plugin,
                                display: display.clone(),
                                description,
                                insert_text: format!("@{mention_name}"),
                                search_terms: vec![
                                    plugin.name.clone(),
                                    plugin.id.clone(),
                                    marketplace.name.clone(),
                                ],
                                target: MentionTarget::Plugin {
                                    name: display,
                                    path: format!("plugin://{}", plugin.id),
                                },
                            }
                        }),
                );
            }
        }
        if let Some(skills) = skills {
            candidates.extend(
                skills
                    .data
                    .iter()
                    .flat_map(|entry| &entry.skills)
                    .filter(|skill| skill.enabled)
                    .map(skill_candidate),
            );
        }

        let mut seen = HashSet::new();
        candidates.retain(|candidate| seen.insert(candidate.target.key().to_string()));
        Self { candidates }
    }
}

fn skill_candidate(skill: &SkillMetadata) -> MentionCandidate {
    let display = skill
        .interface
        .as_ref()
        .and_then(|interface| interface.display_name.clone())
        .unwrap_or_else(|| skill.name.clone());
    let description = skill
        .interface
        .as_ref()
        .and_then(|interface| interface.short_description.clone())
        .or_else(|| skill.short_description.clone())
        .unwrap_or_else(|| skill.description.clone());
    MentionCandidate {
        kind: MentionKind::Skill,
        display: display.clone(),
        description,
        insert_text: format!("${}", skill.name),
        search_terms: vec![skill.name.clone(), display],
        target: MentionTarget::Skill {
            name: skill.name.clone(),
            path: skill.path.to_path_buf(),
        },
    }
}

fn plugin_mention_name(plugin_name: &str, display_name: &str) -> String {
    let plugin_segments = split_plugin_name_segments(plugin_name);
    let display_segments = split_display_name_segments(display_name);
    if plugin_segments.len() == display_segments.len()
        && plugin_segments.iter().zip(&display_segments).all(
            |((plugin_segment, _), display_segment)| {
                plugin_segment.eq_ignore_ascii_case(display_segment)
            },
        )
    {
        let mut result = String::new();
        for ((_, separator), display_segment) in plugin_segments.into_iter().zip(display_segments) {
            result.push_str(&display_segment);
            if let Some(separator) = separator {
                result.push(separator);
            }
        }
        return result;
    }

    let mut result = String::with_capacity(plugin_name.len());
    let mut capitalize_next = true;
    for character in plugin_name.chars() {
        if matches!(character, '-' | '_') {
            capitalize_next = true;
            result.push(character);
        } else if capitalize_next && character.is_ascii_alphabetic() {
            result.push(character.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(character);
            capitalize_next = false;
        }
    }
    result
}

fn split_plugin_name_segments(plugin_name: &str) -> Vec<(String, Option<char>)> {
    let mut segments = Vec::new();
    let mut current = String::new();
    for character in plugin_name.chars() {
        if matches!(character, '-' | '_') {
            if !current.is_empty() {
                segments.push((std::mem::take(&mut current), Some(character)));
            }
        } else {
            current.push(character);
        }
    }
    if !current.is_empty() {
        segments.push((current, None));
    }
    segments
}

fn split_display_name_segments(display_name: &str) -> Vec<String> {
    display_name
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|segment| !segment.is_empty())
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
#[path = "../mention_inventory_tests.rs"]
mod tests;
