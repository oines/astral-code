//! Projection of app-server ecosystem inventory into bounded TUI rows.

use codex_app_server_protocol::AppsListResponse;
use codex_app_server_protocol::HooksListResponse;
use codex_app_server_protocol::ListMcpServerStatusResponse;
use codex_app_server_protocol::McpServerStatusDetail;
use codex_app_server_protocol::PluginListResponse;
use codex_app_server_protocol::SkillsListResponse;

use crate::modal::ModalRow;
use crate::modal::ModalState;

const MAX_ROWS: usize = 200;

pub(crate) fn mcp_panel(
    response: ListMcpServerStatusResponse,
    detail: McpServerStatusDetail,
) -> ModalState {
    let has_more = response.next_cursor.is_some();
    let server_count = response.data.len();
    let tool_count = response
        .data
        .iter()
        .map(|server| server.tools.len())
        .sum::<usize>();
    let mut rows = vec![ModalRow::new(
        "Summary",
        format!("{server_count} servers · {tool_count} tools"),
    )];
    for server in response.data {
        let auth = format!("{:?}", server.auth_status).to_lowercase();
        rows.push(ModalRow::new(
            server.name.clone(),
            format!("{} tools · {auth}", server.tools.len()),
        ));
        if detail == McpServerStatusDetail::Full {
            let mut tools = server.tools.into_keys().collect::<Vec<_>>();
            tools.sort();
            rows.extend(
                tools
                    .into_iter()
                    .map(|tool| ModalRow::new(format!("  {tool}"), server.name.clone())),
            );
        }
    }
    if has_more {
        rows.push(ModalRow::new("More", "additional servers available"));
    }
    ModalState::info("MCP servers", bounded(rows))
}

pub(crate) fn skills_panel(response: SkillsListResponse) -> ModalState {
    let skill_count = response
        .data
        .iter()
        .map(|entry| entry.skills.len())
        .sum::<usize>();
    let error_count = response
        .data
        .iter()
        .map(|entry| entry.errors.len())
        .sum::<usize>();
    let mut rows = vec![ModalRow::new(
        "Summary",
        format!("{skill_count} skills · {error_count} errors"),
    )];
    for entry in response.data {
        rows.extend(entry.skills.into_iter().map(|skill| {
            let scope = format!("{:?}", skill.scope).to_lowercase();
            let status = if skill.enabled { "enabled" } else { "disabled" };
            ModalRow::new(skill.name, format!("{scope} · {status}"))
        }));
        rows.extend(
            entry
                .errors
                .into_iter()
                .map(|error| ModalRow::new("Error", error.message)),
        );
    }
    ModalState::info("Skills", bounded(rows))
}

pub(crate) fn hooks_panel(response: HooksListResponse) -> ModalState {
    let hook_count = response
        .data
        .iter()
        .map(|entry| entry.hooks.len())
        .sum::<usize>();
    let error_count = response
        .data
        .iter()
        .map(|entry| entry.errors.len())
        .sum::<usize>();
    let mut rows = vec![ModalRow::new(
        "Summary",
        format!("{hook_count} hooks · {error_count} errors"),
    )];
    for entry in response.data {
        rows.extend(entry.hooks.into_iter().map(|hook| {
            let event = format!("{:?}", hook.event_name).to_lowercase();
            let handler = format!("{:?}", hook.handler_type).to_lowercase();
            let status = if hook.enabled { "enabled" } else { "disabled" };
            ModalRow::new(hook.key, format!("{event} · {handler} · {status}"))
        }));
        rows.extend(
            entry
                .warnings
                .into_iter()
                .map(|warning| ModalRow::new("Warning", warning)),
        );
        rows.extend(
            entry
                .errors
                .into_iter()
                .map(|error| ModalRow::new("Error", error.message)),
        );
    }
    ModalState::info("Hooks", bounded(rows))
}

pub(crate) fn apps_panel(response: AppsListResponse) -> ModalState {
    let has_more = response.next_cursor.is_some();
    let accessible = response.data.iter().filter(|app| app.is_accessible).count();
    let mut rows = vec![ModalRow::new(
        "Summary",
        format!("{} apps · {accessible} connected", response.data.len()),
    )];
    rows.extend(response.data.into_iter().map(|app| {
        let status = match (app.is_enabled, app.is_accessible) {
            (true, true) => "connected",
            (true, false) => "available",
            (false, _) => "disabled",
        };
        ModalRow::new(app.name, status)
    }));
    if has_more {
        rows.push(ModalRow::new("More", "additional apps available"));
    }
    ModalState::info("Apps", bounded(rows))
}

pub(crate) fn plugins_panel(response: PluginListResponse) -> ModalState {
    let plugin_count = response
        .marketplaces
        .iter()
        .map(|marketplace| marketplace.plugins.len())
        .sum::<usize>();
    let installed_count = response
        .marketplaces
        .iter()
        .flat_map(|marketplace| &marketplace.plugins)
        .filter(|plugin| plugin.installed)
        .count();
    let mut rows = vec![ModalRow::new(
        "Summary",
        format!(
            "{plugin_count} plugins · {installed_count} installed · {} errors",
            response.marketplace_load_errors.len()
        ),
    )];
    for marketplace in response.marketplaces {
        rows.extend(marketplace.plugins.into_iter().map(|plugin| {
            let status = match (plugin.installed, plugin.enabled) {
                (true, true) => "installed · enabled",
                (true, false) => "installed · disabled",
                (false, _) => "available",
            };
            ModalRow::new(plugin.name, format!("{} · {status}", marketplace.name))
        }));
    }
    rows.extend(
        response
            .marketplace_load_errors
            .into_iter()
            .map(|error| ModalRow::new("Error", error.message)),
    );
    ModalState::info("Plugins", bounded(rows))
}

fn bounded(mut rows: Vec<ModalRow>) -> Vec<ModalRow> {
    if rows.len() > MAX_ROWS {
        rows.truncate(MAX_ROWS - 1);
        rows.push(ModalRow::new("More", "results truncated"));
    }
    rows
}

#[cfg(test)]
#[path = "ecosystem_tests.rs"]
mod tests;
