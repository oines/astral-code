//! Patch summaries and image-tool transcript helpers.

use super::*;

#[derive(Debug)]
pub(crate) struct PatchHistoryCell {
    changes: HashMap<PathBuf, FileChange>,
    cwd: PathBuf,
}

impl HistoryCell for PatchHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        create_diff_summary(&self.changes, &self.cwd, width as usize)
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        plain_lines(create_diff_summary(
            &self.changes,
            &self.cwd,
            RAW_DIFF_SUMMARY_WIDTH,
        ))
    }

    fn transcript_presentation(&self) -> HistoryCellPresentation {
        HistoryCellPresentation::two_state(astral_tui::DisplayMode::Collapsed).with_groupable()
    }

    fn transcript_hyperlink_lines_for_presentation(
        &self,
        width: u16,
        mode: astral_tui::DisplayMode,
    ) -> Vec<HyperlinkLine> {
        let mut lines = self.display_lines(width);
        if mode == astral_tui::DisplayMode::Collapsed {
            lines.truncate(1);
        }
        plain_hyperlink_lines(lines)
    }

    fn transcript_viewer_document(
        &self,
        width: u16,
        mode: astral_tui::BlockViewerMode,
    ) -> Option<astral_tui::BlockViewerDocument> {
        let astral_tui::BlockViewerMode::Rich = mode else {
            return None;
        };
        viewer_document_from_lines(
            self.viewer_title(),
            create_diff_summary(&self.changes, &self.cwd, usize::from(width.max(5))),
            width,
        )
    }
}

impl PatchHistoryCell {
    fn viewer_title(&self) -> String {
        if self.changes.len() != 1 {
            return format!("Edit {} files", self.changes.len());
        }
        let Some((path, change)) = self.changes.iter().next() else {
            return "Edit files".into();
        };
        let action = match change {
            FileChange::Add { .. } => "Create",
            FileChange::Delete { .. } => "Delete",
            FileChange::Update { .. } => "Edit",
        };
        format!("{action} {}", display_path_for(path, &self.cwd))
    }
}
/// Create a new `PendingPatch` cell that lists the file‑level summary of
/// a proposed patch. The summary lines should already be formatted (e.g.
/// "A path/to/file.rs").
pub(crate) fn new_patch_event(
    changes: HashMap<PathBuf, FileChange>,
    cwd: &Path,
) -> PatchHistoryCell {
    PatchHistoryCell {
        changes,
        cwd: cwd.to_path_buf(),
    }
}

pub(crate) fn new_patch_apply_failure(stderr: String) -> PlainHistoryCell {
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Failure title
    lines.push(Line::from("✘ Failed to apply patch".magenta().bold()));

    if !stderr.trim().is_empty() {
        let output = output_lines(
            Some(&CommandOutput {
                exit_code: 1,
                formatted_output: String::new(),
                aggregated_output: stderr,
            }),
            OutputLinesParams {
                line_limit: TOOL_CALL_MAX_LINES,
                only_err: true,
                include_angle_pipe: true,
                include_prefix: true,
            },
        );
        lines.extend(output.lines);
    }

    PlainHistoryCell { lines }
}

pub(crate) fn new_view_image_tool_call(path: AbsolutePathBuf, cwd: &Path) -> PlainHistoryCell {
    let display_path = display_path_for(path.as_path(), cwd);

    let lines: Vec<Line<'static>> = vec![
        vec!["• ".dim(), "Viewed Image".bold()].into(),
        vec!["  └ ".dim(), display_path.dim()].into(),
    ];

    PlainHistoryCell { lines }
}

pub(crate) fn new_image_generation_call(
    call_id: String,
    revised_prompt: Option<String>,
    saved_path: Option<AbsolutePathBuf>,
) -> PlainHistoryCell {
    let detail = revised_prompt.unwrap_or_else(|| call_id.clone());

    let mut lines: Vec<Line<'static>> = vec![
        vec!["• ".dim(), "Generated Image:".bold()].into(),
        vec!["  └ ".dim(), detail.dim()].into(),
    ];
    if let Some(saved_path) = saved_path {
        let saved_path = Url::from_file_path(saved_path.as_path())
            .map(|url| url.to_string())
            .unwrap_or_else(|_| saved_path.display().to_string());
        lines.push(vec!["  └ ".dim(), "Saved to: ".dim(), saved_path.into()].into());
    }

    PlainHistoryCell { lines }
}
