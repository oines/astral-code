use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_app_server_protocol::WebSearchAction;
use codex_utils_absolute_path::AbsolutePathBuf;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::Line;

use super::SurfaceRenderer;
use crate::ConversationState;
use crate::ConversationSurface;
use crate::MaterializedSurfaceEntry;
use crate::ScrollDirection;
use crate::SurfaceEntryPresentation;
use crate::SurfaceEntrySpacing;
use crate::SurfaceNodeId;
use crate::SurfaceViewport;
use astral_tui_scrollback::DisplayMode;
use astral_tui_scrollback::EntryLifecycle;
use astral_tui_scrollback::EntryRenderOptions;
use astral_tui_scrollback::LineJoiner;
use astral_tui_scrollback::MarkdownLine;
use astral_tui_scrollback::TranscriptEntryId;

#[test]
fn clipped_surface_chrome_matches_grok_layering() {
    let conversation = ConversationState::from_thread(&thread(vec![turn(vec![
        user("Inspect the renderer"),
        reasoning("reasoning", "Reading the relevant layout code."),
        web_search("search-1", "ratatui retained viewport"),
        web_search("search-2", "terminal selection box"),
        assistant(
            "answer",
            "The surface now keeps one geometry for:\n\n- scrolling\n- selection\n- mouse hit testing\n- clipped rendering",
        ),
    ])]));
    let area = Rect::new(0, 0, 60, 8);
    let content_width = SurfaceRenderer::content_width(area);
    let surface =
        ConversationSurface::render(&conversation, EntryRenderOptions::new(content_width));
    let entries = conversation.transcript().turns()[0].entries();
    let reasoning = SurfaceNodeId::VerbGroup(entries[1].id());
    let answer = SurfaceNodeId::Entry(entries[4].id());
    let answer_row = surface.node(answer).expect("answer node").rows().start;
    let mut viewport = SurfaceViewport::default();
    viewport.prepare(&surface, area.height);
    assert!(viewport.scroll_to_top(&surface));
    assert!(viewport.move_selection(&surface, ScrollDirection::Down));
    assert!(viewport.move_selection(&surface, ScrollDirection::Down));
    assert_eq!(viewport.selected(), Some(reasoning));
    assert_eq!(
        viewport.hover_screen_row(&surface, answer_row as u16),
        Some(answer)
    );

    let mut buffer = Buffer::empty(area);
    SurfaceRenderer::default().render(area, &mut buffer, &surface, &viewport);

    insta::assert_snapshot!(buffer_text(&buffer, area));
    assert!(
        buffer
            .content()
            .iter()
            .all(|cell| { cell.style().bg.is_none() || cell.style().bg == Some(Color::Reset) })
    );
}

#[test]
fn ungrouped_entries_do_not_share_selected_chrome() {
    let presentation = SurfaceEntryPresentation {
        lifecycle: EntryLifecycle::Restored,
        mode: DisplayMode::Collapsed,
        foldable: false,
        groupable: true,
        turn_settled: true,
        presentation_stable: true,
    };
    let entries = ["first", "selected", "last"]
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            MaterializedSurfaceEntry::ungrouped(
                TranscriptEntryId::new(index as u64),
                SurfaceEntrySpacing::Continue,
                presentation,
                vec![MarkdownLine {
                    line: Line::from(text),
                    joiner_to_previous: LineJoiner::HardBreak,
                    links: Vec::new(),
                }],
            )
        });
    let area = Rect::new(0, 0, 24, 5);
    let surface = ConversationSurface::from_materialized(21, entries);
    let selected = SurfaceNodeId::Entry(TranscriptEntryId::new(1));
    let mut viewport = SurfaceViewport::default();
    viewport.prepare(&surface, area.height);
    assert!(viewport.select_node(&surface, Some(selected)));

    let mut buffer = Buffer::empty(area);
    SurfaceRenderer::default().render(area, &mut buffer, &surface, &viewport);

    insta::assert_snapshot!(buffer_text(&buffer, area), @"
 ❙ first
│❙ selected           │
 ❙ last
");
}

#[test]
fn selected_group_chrome_stops_at_adjacent_group_boundary() {
    let presentation = SurfaceEntryPresentation {
        lifecycle: EntryLifecycle::Restored,
        mode: DisplayMode::Collapsed,
        foldable: false,
        groupable: true,
        turn_settled: true,
        presentation_stable: true,
    };
    let entries = [("A", "first"), ("A", "selected"), ("B", "last")]
        .into_iter()
        .enumerate()
        .map(|(index, (group, text))| {
            MaterializedSurfaceEntry::new(
                TranscriptEntryId::new(index as u64),
                group,
                SurfaceEntrySpacing::Continue,
                presentation,
                vec![MarkdownLine {
                    line: Line::from(text),
                    joiner_to_previous: LineJoiner::HardBreak,
                    links: Vec::new(),
                }],
            )
        });
    let area = Rect::new(0, 0, 24, 5);
    let surface = ConversationSurface::from_materialized(21, entries);
    let selected = SurfaceNodeId::Entry(TranscriptEntryId::new(1));
    let mut viewport = SurfaceViewport::default();
    viewport.prepare(&surface, area.height);
    assert!(viewport.select_node(&surface, Some(selected)));

    let mut buffer = Buffer::empty(area);
    SurfaceRenderer::default().render(area, &mut buffer, &surface, &viewport);

    insta::assert_snapshot!(buffer_text(&buffer, area), @"
│❙ first              │
│❙ selected           │
 ❙ last
");
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

fn thread(turns: Vec<Turn>) -> Thread {
    Thread {
        id: "thread-1".to_string(),
        session_id: "session-1".to_string(),
        forked_from_id: None,
        parent_thread_id: None,
        preview: String::new(),
        ephemeral: false,
        model_provider: "astral".to_string(),
        created_at: 1,
        updated_at: 1,
        status: ThreadStatus::Idle,
        path: None,
        cwd: AbsolutePathBuf::current_dir().expect("current directory should be absolute"),
        cli_version: "0.0.0".to_string(),
        source: SessionSource::Exec,
        thread_source: None,
        agent_nickname: None,
        agent_role: None,
        git_info: None,
        name: None,
        turns,
    }
}

fn turn(items: Vec<ThreadItem>) -> Turn {
    Turn {
        id: "turn-1".to_string(),
        items,
        items_view: TurnItemsView::Full,
        status: TurnStatus::Completed,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
}

fn user(text: &str) -> ThreadItem {
    ThreadItem::UserMessage {
        id: "user".to_string(),
        client_id: None,
        content: vec![UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }],
    }
}

fn reasoning(id: &str, summary: &str) -> ThreadItem {
    ThreadItem::Reasoning {
        id: id.to_string(),
        summary: vec![summary.to_string()],
        content: Vec::new(),
    }
}

fn web_search(id: &str, query: &str) -> ThreadItem {
    ThreadItem::WebSearch {
        id: id.to_string(),
        query: query.to_string(),
        action: Some(WebSearchAction::Search {
            query: Some(query.to_string()),
            queries: None,
        }),
    }
}

fn assistant(id: &str, text: &str) -> ThreadItem {
    ThreadItem::AgentMessage {
        id: id.to_string(),
        text: text.to_string(),
        phase: None,
        memory_citation: None,
    }
}
