use std::path::Path;
use std::sync::Arc;

use astral_tui::InlineHost;
use astral_tui::MarkdownLine;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::*;
use crate::history_cell::AgentMarkdownCell;
use crate::history_cell::AgentMessageCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::HistoryRenderMode;
use crate::history_cell::new_proposed_plan;
use crate::history_transcript::HistoryTranscript;

fn project_cell(cell: Arc<dyn HistoryCell>, width: u16) -> Vec<MarkdownLine> {
    let expected = materialize_terminal_rows(cell.transcript_hyperlink_lines(width), width);
    let transcript = HistoryTranscript::from(vec![cell]);
    let surface = materialize_history_surface(transcript.entries(), /*live_tail*/ None, width);
    let actual = surface
        .nodes()
        .first()
        .expect("source-backed cell should produce a surface node")
        .rendered()
        .lines()
        .to_vec();

    assert_eq!(actual, expected);
    actual
}

fn visible_text(lines: &[MarkdownLine]) -> String {
    lines
        .iter()
        .map(|line| {
            line.line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_complete_link_targets(lines: &[MarkdownLine], destination: &str) {
    let targets = lines
        .iter()
        .flat_map(|line| line.links.iter().map(|link| link.target.as_str()))
        .collect::<Vec<_>>();

    assert!(!targets.is_empty(), "expected projected web-link metadata");
    assert!(targets.iter().all(|target| *target == destination));
}

#[test]
fn agent_markdown_surface_preserves_authoritative_cell_reflow() {
    let destination = "https://example.com/a/very/long/path/to/an/artifact";
    let source = format!(
        "See [source](/workspace/astral/src/main.rs#L12).\n\n| Item | URL |\n| --- | --- |\n| report | {destination} |"
    );
    let cell: Arc<dyn HistoryCell> = Arc::new(AgentMarkdownCell::new(
        source,
        Path::new("/workspace/astral"),
    ));

    let narrow = project_cell(cell.clone(), /*width*/ 32);
    let wide = project_cell(cell, /*width*/ 72);

    assert_ne!(narrow, wide, "surface should reflow from raw cell source");
    let narrow_text = visible_text(&narrow);
    assert!(
        narrow_text.replace('\n', "").contains("src/main.rs:12"),
        "expected the cwd-relative local link after reflow, got: {narrow_text:?}"
    );
    assert_complete_link_targets(&narrow, destination);
    assert_complete_link_targets(&wide, destination);
}

#[test]
fn proposed_plan_surface_preserves_plan_chrome_and_reflow() {
    let destination = "https://example.com/a/very/long/path/to/a/plan";
    let source = format!(
        "## Implementation\n\n| Step | Reference |\n| --- | --- |\n| Build | {destination} |"
    );
    let cell: Arc<dyn HistoryCell> =
        Arc::new(new_proposed_plan(source, Path::new("/workspace/astral")));

    let narrow = project_cell(cell.clone(), /*width*/ 36);
    let wide = project_cell(cell, /*width*/ 76);

    assert_ne!(narrow, wide, "plan surface should reflow from raw source");
    assert!(visible_text(&narrow).starts_with("• Proposed Plan"));
    assert_complete_link_targets(&narrow, destination);
    assert_complete_link_targets(&wide, destination);
}

#[test]
fn inline_surface_commits_settled_history_and_keeps_running_tail_live() {
    let settled: Arc<dyn HistoryCell> = Arc::new(AgentMessageCell::new(
        vec![Line::from("settled assistant response")],
        /*is_first_line*/ true,
    ));
    let transcript = HistoryTranscript::from(vec![settled]);
    let live_lines = vec![HyperlinkLine::from("streaming assistant tail")];
    let outer_width = 44;
    let surface = materialize_history_display_surface(
        transcript.entries(),
        Some(HistorySurfaceTail {
            lines: &live_lines,
            is_stream_continuation: true,
        }),
        SurfaceRenderer::content_width(Rect::new(0, 0, outer_width, 1)),
        HistoryRenderMode::Rich,
    );
    let mut host = InlineHost::from_surface("thread-a", surface);
    let mut committed = Vec::new();

    let result = host
        .commit_with(|surface, rows| {
            committed.extend(render_history_surface_rows(surface, outer_width, rows));
            Ok(())
        })
        .expect("commit settled prefix");
    assert_eq!(result.committed_nodes, 1);

    let area = Rect::new(0, 0, outer_width, 3);
    let mut live = Buffer::empty(area);
    host.render_live_tail(area, &mut live);
    insta::assert_snapshot!(
        "inline_history_settled_prefix_and_live_tail",
        format!(
            "COMMITTED PREFIX\n{}\n\nPINNED LIVE TAIL\n{}",
            hyperlink_line_text(&committed),
            buffer_text(&live, area),
        )
    );
}

fn hyperlink_line_text(lines: &[HyperlinkLine]) -> String {
    lines
        .iter()
        .map(|line| {
            line.line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn buffer_text(buffer: &Buffer, area: Rect) -> String {
    (area.y..area.bottom())
        .map(|y| {
            let mut row = String::new();
            for x in area.x..area.right() {
                row.push_str(buffer.cell((x, y)).expect("cell in area").symbol());
            }
            row.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
