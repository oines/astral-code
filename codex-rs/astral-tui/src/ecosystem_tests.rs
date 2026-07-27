use codex_app_server_protocol::AppsListResponse;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::apps_panel;
use super::bounded;
use crate::modal::ModalRow;

#[test]
fn apps_panel_preserves_connection_state() {
    let response: AppsListResponse = serde_json::from_value(json!({
        "data": [
            {
                "id": "github",
                "name": "GitHub",
                "description": null,
                "logoUrl": null,
                "logoUrlDark": null,
                "distributionChannel": null,
                "branding": null,
                "appMetadata": null,
                "labels": null,
                "installUrl": null,
                "isAccessible": true,
                "isEnabled": true,
                "pluginDisplayNames": []
            },
            {
                "id": "linear",
                "name": "Linear",
                "description": null,
                "logoUrl": null,
                "logoUrlDark": null,
                "distributionChannel": null,
                "branding": null,
                "appMetadata": null,
                "labels": null,
                "installUrl": null,
                "isAccessible": false,
                "isEnabled": false,
                "pluginDisplayNames": []
            }
        ],
        "nextCursor": null
    }))
    .expect("valid apps response");

    assert_eq!(
        apps_panel(response).rows,
        vec![
            ModalRow::new("Summary", "2 apps · 1 connected"),
            ModalRow::new("GitHub", "connected"),
            ModalRow::new("Linear", "disabled"),
        ]
    );
}

#[test]
fn ecosystem_rows_are_bounded_for_terminal_rendering() {
    let rows = (0..250)
        .map(|index| ModalRow::new(format!("Item {index}"), "available"))
        .collect();
    let bounded = bounded(rows);

    assert_eq!(bounded.len(), 200);
    assert_eq!(
        bounded.last(),
        Some(&ModalRow::new("More", "results truncated"))
    );
}
