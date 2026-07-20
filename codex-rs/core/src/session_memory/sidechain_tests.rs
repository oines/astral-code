use pretty_assertions::assert_eq;
use serde_json::json;

use super::editor::SummaryEditor;
use super::editor::apply_summary_patch;
use crate::tools::context::ToolPayload;

fn update_patch(path: &std::path::Path, before: &str, after: &str) -> String {
    format!(
        "*** Begin Patch\n*** Update File: {}\n@@\n-{before}\n+{after}\n*** End Patch",
        path.display()
    )
}

#[tokio::test]
async fn summary_apply_patch_accepts_function_envelope_and_updates_atomically() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let summary_path = temp.path().join("summary.md");
    tokio::fs::write(&summary_path, "before\n")
        .await
        .expect("write summary");
    let editor = SummaryEditor::new("before\n".to_string());
    let patch = update_patch(&summary_path, "before", "after");
    let payload = ToolPayload::Function {
        arguments: json!({ "input": patch }).to_string(),
    };

    let result = apply_summary_patch(&summary_path, &payload, &editor).await;

    assert!(result.edited_summary, "{}", result.text);
    assert_eq!(editor.content().await, "after\n");
    assert_eq!(editor.revision().await, 1);
    assert_eq!(
        tokio::fs::read_to_string(&summary_path)
            .await
            .expect("read summary"),
        "after\n"
    );
}

#[tokio::test]
async fn summary_apply_patch_rejects_changes_outside_single_in_place_update() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let summary_path = temp.path().join("summary.md");
    let other_path = temp.path().join("other.md");
    tokio::fs::write(&other_path, "other\n")
        .await
        .expect("write other file");
    let cases = [
        ("wrong path", update_patch(&other_path, "other", "changed")),
        (
            "add",
            format!(
                "*** Begin Patch\n*** Add File: {}\n+added\n*** End Patch",
                summary_path.display()
            ),
        ),
        (
            "delete",
            format!(
                "*** Begin Patch\n*** Delete File: {}\n*** End Patch",
                summary_path.display()
            ),
        ),
        (
            "move",
            format!(
                "*** Begin Patch\n*** Update File: {}\n*** Move to: {}\n@@\n-before\n+after\n*** End Patch",
                summary_path.display(),
                other_path.display()
            ),
        ),
        (
            "multiple files",
            format!(
                "{}\n*** Add File: {}\n+added\n*** End Patch",
                update_patch(&summary_path, "before", "after").trim_end_matches("\n*** End Patch"),
                other_path.display()
            ),
        ),
        ("no-op", update_patch(&summary_path, "before", "before")),
    ];

    for (label, patch) in cases {
        tokio::fs::write(&summary_path, "before\n")
            .await
            .expect("reset summary");
        let editor = SummaryEditor::new("before\n".to_string());
        let result = apply_summary_patch(
            &summary_path,
            &ToolPayload::Custom { input: patch },
            &editor,
        )
        .await;

        assert!(!result.edited_summary, "{label}: {}", result.text);
        assert_eq!(editor.content().await, "before\n", "{label}");
        assert_eq!(editor.revision().await, 0, "{label}");
        assert_eq!(
            tokio::fs::read_to_string(&summary_path)
                .await
                .expect("read summary"),
            "before\n",
            "{label}"
        );
    }
}

#[tokio::test]
async fn summary_apply_patch_rejects_stale_pre_read_content() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let summary_path = temp.path().join("summary.md");
    tokio::fs::write(&summary_path, "external change\n")
        .await
        .expect("write changed summary");
    let editor = SummaryEditor::new("before\n".to_string());
    let patch = update_patch(&summary_path, "before", "after");

    let result = apply_summary_patch(
        &summary_path,
        &ToolPayload::Custom { input: patch },
        &editor,
    )
    .await;

    assert!(!result.edited_summary);
    assert_eq!(editor.content().await, "before\n");
    assert_eq!(
        tokio::fs::read_to_string(&summary_path)
            .await
            .expect("read summary"),
        "external change\n"
    );
}
