use astral_tui_scrollback::DisplayMode;
use astral_tui_scrollback::EntryLifecycle;
use astral_tui_scrollback::EntryRenderOptions;
use astral_tui_scrollback::LineJoiner;
use astral_tui_scrollback::MarkdownLine;
use astral_tui_scrollback::MarkdownLink;
use astral_tui_scrollback::TranscriptEntryId;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::WebSearchAction;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

use super::ConversationSurface;
use super::MaterializedSurfaceEntry;
use super::SurfaceEntryPresentation;
use super::SurfaceEntrySpacing;
use super::SurfaceNodeId;
use super::SurfaceNodeKind;
use crate::ConversationState;
use crate::VerbGroupDisplayAction;
use ratatui::style::Stylize;
use ratatui::text::Line;

#[test]
fn materialized_surface_preserves_external_order_and_identity() {
    let first_presentation = SurfaceEntryPresentation {
        lifecycle: EntryLifecycle::Running {
            started_at_ms: Some(17),
        },
        mode: DisplayMode::Collapsed,
        foldable: true,
        groupable: false,
        turn_settled: false,
        presentation_stable: true,
    };
    let second_presentation = SurfaceEntryPresentation {
        lifecycle: EntryLifecycle::Restored,
        mode: DisplayMode::Expanded,
        foldable: false,
        groupable: true,
        turn_settled: true,
        presentation_stable: false,
    };
    let first = TranscriptEntryId::new(41);
    let second = TranscriptEntryId::new(7);
    let first_lines = vec![
        MarkdownLine {
            line: vec!["first ".into(), "link".cyan().underlined()].into(),
            joiner_to_previous: LineJoiner::HardBreak,
            links: vec![MarkdownLink {
                id: 9,
                columns: 6..10,
                target: "https://example.com/first".to_string(),
            }],
        },
        MarkdownLine {
            line: Line::from("continued".italic()),
            joiner_to_previous: LineJoiner::Space,
            links: Vec::new(),
        },
    ];
    let second_lines = vec![MarkdownLine {
        line: Line::from("second".bold()),
        joiner_to_previous: LineJoiner::None,
        links: Vec::new(),
    }];
    let surface = ConversationSurface::from_materialized(
        30,
        [
            MaterializedSurfaceEntry::new(
                first,
                "turn-a",
                SurfaceEntrySpacing::Separate,
                first_presentation,
                first_lines.clone(),
            ),
            MaterializedSurfaceEntry::new(
                second,
                "turn-b",
                SurfaceEntrySpacing::Continue,
                second_presentation,
                second_lines.clone(),
            ),
        ],
    );

    assert_eq!(
        surface
            .nodes()
            .iter()
            .map(super::SurfaceNode::id)
            .collect::<Vec<_>>(),
        vec![SurfaceNodeId::Entry(first), SurfaceNodeId::Entry(second)]
    );
    assert_eq!(surface.nodes()[0].presentation_group(), Some("turn-a"));
    assert_eq!(surface.nodes()[1].presentation_group(), Some("turn-b"));
    assert_eq!(surface.nodes()[0].gap_after(), 0);
    assert_eq!(
        surface.nodes()[0].kind(),
        &SurfaceNodeKind::Entry {
            lifecycle: EntryLifecycle::Running {
                started_at_ms: Some(17),
            },
            mode: DisplayMode::Collapsed,
            foldable: true,
            groupable: false,
            turn_settled: false,
            presentation_stable: true,
        }
    );
    assert_eq!(
        surface.nodes()[1].kind(),
        &SurfaceNodeKind::Entry {
            lifecycle: EntryLifecycle::Restored,
            mode: DisplayMode::Expanded,
            foldable: false,
            groupable: true,
            turn_settled: true,
            presentation_stable: false,
        }
    );
    assert_eq!(surface.nodes()[0].rendered().lines(), first_lines);
    assert_eq!(surface.nodes()[1].rendered().lines(), second_lines);
    insta::assert_snapshot!(surface_text(&surface), @"\
first link
continued
second
");
}

#[test]
fn ungrouped_materialized_spacing_does_not_invent_grouping_semantics() {
    let presentation = SurfaceEntryPresentation {
        lifecycle: EntryLifecycle::Restored,
        mode: DisplayMode::Collapsed,
        foldable: false,
        groupable: true,
        turn_settled: true,
        presentation_stable: true,
    };
    let surface = ConversationSurface::from_materialized(
        20,
        [
            MaterializedSurfaceEntry::ungrouped(
                TranscriptEntryId::new(1),
                SurfaceEntrySpacing::Separate,
                presentation,
                vec![MarkdownLine {
                    line: Line::from("first"),
                    joiner_to_previous: LineJoiner::HardBreak,
                    links: Vec::new(),
                }],
            ),
            MaterializedSurfaceEntry::ungrouped(
                TranscriptEntryId::new(2),
                SurfaceEntrySpacing::Separate,
                presentation,
                vec![MarkdownLine {
                    line: Line::from("second"),
                    joiner_to_previous: LineJoiner::HardBreak,
                    links: Vec::new(),
                }],
            ),
        ],
    );

    assert_eq!(surface.nodes()[0].presentation_group(), None);
    assert_eq!(surface.nodes()[1].presentation_group(), None);
    assert_eq!(surface.nodes()[0].gap_after(), 1);
}

fn surface_text(surface: &ConversationSurface) -> String {
    surface
        .lines()
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

#[test]
fn one_surface_preserves_source_order_and_group_hit_geometry() {
    let mut conversation = ConversationState::from_thread(&thread(vec![turn(
        "turn-1",
        vec![
            web_search("search-1", "rust"),
            web_search("search-2", "ratatui"),
            assistant("answer", "Done."),
        ],
    )]));
    let turn = &conversation.transcript().turns()[0];
    let search_1 = turn.entries()[0].id();
    let search_2 = turn.entries()[1].id();
    let answer = turn.entries()[2].id();
    let group = conversation.verb_groups("turn-1")[0].clone();

    let collapsed = ConversationSurface::render(&conversation, EntryRenderOptions::new(50));
    assert_eq!(
        collapsed
            .nodes()
            .iter()
            .map(super::SurfaceNode::id)
            .collect::<Vec<_>>(),
        vec![
            SurfaceNodeId::VerbGroup(search_1),
            SurfaceNodeId::Entry(answer),
        ]
    );
    assert_eq!(
        collapsed.nodes()[0].kind(),
        &SurfaceNodeKind::VerbGroup {
            mode: DisplayMode::Collapsed,
            members: vec![search_1, search_2],
            running: false,
            turn_settled: true,
            presentation_stable: true,
        }
    );
    assert_exact_gaps(&collapsed, &[1, 1]);

    assert_eq!(
        conversation.apply_verb_group_display_action(
            "turn-1",
            group.anchor(),
            VerbGroupDisplayAction::Expand,
        ),
        Some(DisplayMode::Expanded)
    );
    let expanded = ConversationSurface::render(&conversation, EntryRenderOptions::new(50));
    assert_eq!(
        expanded
            .nodes()
            .iter()
            .map(super::SurfaceNode::id)
            .collect::<Vec<_>>(),
        vec![
            SurfaceNodeId::VerbGroup(search_1),
            SurfaceNodeId::Entry(search_1),
            SurfaceNodeId::Entry(search_2),
            SurfaceNodeId::Entry(answer),
        ]
    );
    assert_eq!(
        expanded.nodes()[0].kind(),
        &SurfaceNodeKind::VerbGroup {
            mode: DisplayMode::Expanded,
            members: vec![search_1, search_2],
            running: false,
            turn_settled: true,
            presentation_stable: true,
        }
    );
    assert_exact_gaps(&expanded, &[1, 0, 1, 1]);
}

fn assert_exact_gaps(surface: &ConversationSurface, expected_gaps: &[usize]) {
    assert_eq!(
        surface
            .nodes()
            .iter()
            .map(super::SurfaceNode::gap_after)
            .collect::<Vec<_>>(),
        expected_gaps
    );
    let mut row = 0usize;
    for node in surface.nodes() {
        assert_eq!(node.rows().start, row);
        for node_row in node.rows() {
            assert_eq!(
                surface.node_at_row(node_row).map(super::SurfaceNode::id),
                Some(node.id())
            );
        }
        for gap_row in node.rows().end..node.rows().end + node.gap_after() {
            assert_eq!(surface.node_at_row(gap_row), None);
            assert_eq!(
                surface
                    .anchor_at_row(gap_row)
                    .map(super::SurfaceAnchor::node),
                Some(node.id())
            );
        }
        row = node.rows().end + node.gap_after();
    }
    assert_eq!(surface.row_count(), row);
    assert_eq!(surface.lines().count(), row);
    assert_eq!(surface.node_at_row(row), None);
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

fn turn(id: &str, items: Vec<ThreadItem>) -> Turn {
    Turn {
        id: id.to_string(),
        items,
        items_view: TurnItemsView::Full,
        status: TurnStatus::Completed,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
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
