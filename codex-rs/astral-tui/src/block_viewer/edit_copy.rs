// Derived from Grok Build's edit viewer patch-range copy behavior at commit
// 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Modified to support Astral's provider-neutral multi-file edit blocks.

use astral_tui_scrollback::EditCopyKind;
use astral_tui_scrollback::EditCopyLine;
use astral_tui_scrollback::PresentationBlock;
use astral_tui_scrollback::ToolKind;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::PatchChangeKind;

use super::BlockViewerState;

impl BlockViewerState {
    pub(crate) fn take_visual_selection_text(
        &mut self,
        block: &PresentationBlock,
    ) -> Option<String> {
        let text = self.visual_selection_text(block);
        self.clear_visual_selection();
        text
    }

    fn visual_selection_text(&self, block: &PresentationBlock) -> Option<String> {
        let range = self.visual_selection_range()?;
        let selected_items = range.filter_map(|item| self.visible_item_indices.get(item).copied());
        let PresentationBlock::Tool(tool) = block else {
            return Some(
                selected_items
                    .filter_map(|item| self.logical_lines.get(item))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        };
        if tool.kind != ToolKind::Edit {
            return Some(
                selected_items
                    .filter_map(|item| self.logical_lines.get(item))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }

        let mut selected_diff_lines = Vec::new();
        for item in selected_items {
            let Some(line) = self.edit_copy_lines.get(item).and_then(Option::as_ref) else {
                continue;
            };
            if selected_diff_lines.last() != Some(line) {
                selected_diff_lines.push(line.clone());
            }
        }
        patch_from_lines(&tool.changes, &selected_diff_lines)
    }
}

fn patch_from_lines(changes: &[FileUpdateChange], lines: &[EditCopyLine]) -> Option<String> {
    let mut groups: Vec<(usize, Vec<&EditCopyLine>)> = Vec::new();
    for line in lines {
        if let Some((change_index, group)) = groups.last_mut()
            && *change_index == line.change_index
        {
            group.push(line);
            continue;
        }
        groups.push((line.change_index, vec![line]));
    }

    let patches = groups
        .into_iter()
        .filter_map(|(change_index, lines)| {
            let change = changes.get(change_index)?;
            Some(change_patch(change, &lines))
        })
        .collect::<Vec<_>>();
    (!patches.is_empty()).then(|| patches.join("\n"))
}

fn change_patch(change: &FileUpdateChange, lines: &[&EditCopyLine]) -> String {
    let (old_path, new_path) = match &change.kind {
        PatchChangeKind::Add => (
            "/dev/null".to_string(),
            prefixed_patch_path("b", &change.path),
        ),
        PatchChangeKind::Delete => (
            prefixed_patch_path("a", &change.path),
            "/dev/null".to_string(),
        ),
        PatchChangeKind::Update { move_path } => {
            let new_path = move_path
                .as_ref()
                .map_or_else(|| change.path.clone(), |path| path.display().to_string());
            (
                prefixed_patch_path("a", &change.path),
                prefixed_patch_path("b", &new_path),
            )
        }
    };
    let old_start = if matches!(&change.kind, PatchChangeKind::Add) {
        0
    } else {
        lines
            .iter()
            .filter(|line| line.kind != EditCopyKind::Insert)
            .find_map(|line| line.old_line)
            .unwrap_or(1)
    };
    let new_start = if matches!(&change.kind, PatchChangeKind::Delete) {
        0
    } else {
        lines
            .iter()
            .filter(|line| line.kind != EditCopyKind::Delete)
            .find_map(|line| line.new_line)
            .unwrap_or(1)
    };
    let old_count = lines
        .iter()
        .filter(|line| line.kind != EditCopyKind::Insert)
        .count();
    let new_count = lines
        .iter()
        .filter(|line| line.kind != EditCopyKind::Delete)
        .count();

    let mut patch = format!(
        "--- {old_path}\n+++ {new_path}\n@@ -{old_start},{old_count} +{new_start},{new_count} @@\n"
    );
    for line in lines {
        patch.push(match line.kind {
            EditCopyKind::Context => ' ',
            EditCopyKind::Insert => '+',
            EditCopyKind::Delete => '-',
        });
        patch.push_str(&line.text);
        patch.push('\n');
    }
    patch
}

fn prefixed_patch_path(prefix: &str, path: &str) -> String {
    if path.starts_with(std::path::MAIN_SEPARATOR) {
        format!("{prefix}{path}")
    } else {
        format!("{prefix}/{path}")
    }
}
