use pretty_assertions::assert_eq;
use ratatui::style::Stylize;
use ratatui::text::Line;

use super::ToolCardHeader;
use super::ToolCardStatus;
use super::render_body;
use super::render_header;
use crate::DisplayMode;
use crate::LineJoiner;
use crate::MarkdownLine;
use crate::render::EntryRenderOptions;

#[test]
fn status_title_and_failure_body_keep_their_styles() {
    let options = EntryRenderOptions::new(/*width*/ 80);
    let rendered_headers = [
        ToolCardStatus::Running,
        ToolCardStatus::Succeeded,
        ToolCardStatus::Failed,
    ]
    .map(|status| {
        render_header(
            ToolCardHeader {
                title: Some("Search".to_string()),
                detail: "Find Docs".to_string(),
                status,
                duration_ms: None,
            },
            DisplayMode::Collapsed,
            options,
        )
    });
    let expected_headers = [
        header("◇ ".magenta()),
        header("◆ ".green()),
        header("× ".red()),
    ];
    assert_eq!(rendered_headers, expected_headers);

    assert_eq!(
        render_body(
            vec!["network timeout".to_string()],
            ToolCardStatus::Failed,
            options
        ),
        vec![markdown_line(Line::from(vec![
            "  │ ".dim(),
            "network timeout".red(),
        ]))],
    );
}

fn header(marker: ratatui::text::Span<'static>) -> Vec<MarkdownLine> {
    vec![markdown_line(Line::from(vec![
        marker,
        "Search".bold().dim(),
        " ".into(),
        "Find Docs".cyan(),
    ]))]
}

fn markdown_line(line: Line<'static>) -> MarkdownLine {
    MarkdownLine {
        line,
        joiner_to_previous: LineJoiner::HardBreak,
        links: Vec::new(),
    }
}
