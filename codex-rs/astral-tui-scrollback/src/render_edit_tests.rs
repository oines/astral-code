use std::path::PathBuf;

use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::PatchChangeKind;
use codex_app_server_protocol::ThreadItem;
use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use ratatui::style::Color;

use super::DiffStyle;
use super::RenderOptions;
use super::render_block;
use crate::DisplayMode;
use crate::PresentationBlock;
use crate::TimelineStream;

fn file_change(changes: Vec<FileUpdateChange>) -> PresentationBlock {
    PresentationBlock::from_item(
        &ThreadItem::FileChange {
            id: "patch".to_string(),
            changes,
            status: PatchApplyStatus::Completed,
        },
        &TimelineStream::None,
    )
    .expect("file change should project")
}

fn rendered(block: &PresentationBlock, width: u16, mode: DisplayMode) -> String {
    render_block(block, RenderOptions::for_mode(width, mode))
        .lines
        .iter()
        .map(|line| line.to_string().trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn single_file_change_variants_snapshot() {
    let variants = [
        (
            "CREATE",
            FileUpdateChange {
                path: "src/new.rs".to_string(),
                kind: PatchChangeKind::Add,
                diff: "fn main() {\n\tprintln!(\"new\");\n}\n".to_string(),
            },
        ),
        (
            "EDIT",
            FileUpdateChange {
                path: "src/lib.rs".to_string(),
                kind: PatchChangeKind::Update { move_path: None },
                diff: concat!(
                    "--- a/src/lib.rs\n",
                    "+++ b/src/lib.rs\n",
                    "@@ -10,3 +10,3 @@\n",
                    " pub fn label() {\n",
                    "-    \"old\"\n",
                    "+    \"new\"\n",
                    " }\n",
                    "@@ -40,2 +40,3 @@\n",
                    " fn tail() {\n",
                    "+    finish();\n",
                    " }\n",
                )
                .to_string(),
            },
        ),
        (
            "DELETE",
            FileUpdateChange {
                path: "notes/obsolete.md".to_string(),
                kind: PatchChangeKind::Delete,
                diff: "# Old\n\nRemove this file.\n".to_string(),
            },
        ),
        (
            "MOVE",
            FileUpdateChange {
                path: "src/old.rs".to_string(),
                kind: PatchChangeKind::Update {
                    move_path: Some(PathBuf::from("src/new.rs")),
                },
                diff: String::new(),
            },
        ),
    ];
    let snapshot = variants
        .into_iter()
        .map(|(label, change)| {
            format!(
                "{label}\n{}",
                rendered(&file_change(vec![change]), 46, DisplayMode::Expanded)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    assert_snapshot!(snapshot);
}

#[test]
fn narrow_structured_diff_wraps_snapshot() {
    let block = file_change(vec![FileUpdateChange {
        path: "src/message.rs".to_string(),
        kind: PatchChangeKind::Update { move_path: None },
        diff: concat!(
            "@@ -98 +98 @@\n",
            "-let message = \"旧的长消息\";\n",
            "+let message = \"新的长消息会正确折行\";\n",
        )
        .to_string(),
    }]);

    assert_snapshot!(rendered(&block, 24, DisplayMode::Expanded));
}

#[test]
fn diff_background_starts_after_the_gutter() {
    let block = file_change(vec![FileUpdateChange {
        path: "src/new.rs".to_string(),
        kind: PatchChangeKind::Add,
        diff: "let answer = 42;\n".to_string(),
    }]);
    let insert_background = Color::Blue;
    let text = render_block(
        &block,
        RenderOptions::expanded(32).with_diff_style(DiffStyle {
            insert_background: Some(insert_background),
            ..DiffStyle::default()
        }),
    );
    let diff_line = &text.lines[2];
    let backgrounds = diff_line
        .spans
        .iter()
        .map(|span| span.style.bg)
        .collect::<Vec<_>>();

    assert_eq!(
        backgrounds,
        [
            vec![None, None, None],
            vec![Some(insert_background); backgrounds.len() - 3],
        ]
        .concat()
    );
}
