use astral_tui_scrollback::DisplayMode;
use astral_tui_scrollback::EntryRenderOptions;
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
use super::SurfaceNodeId;
use super::SurfaceNodeKind;
use crate::ConversationState;
use crate::VerbGroupDisplayAction;

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
        }
    );
    assert_contiguous(&collapsed);

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
        }
    );
    assert_contiguous(&expanded);
}

fn assert_contiguous(surface: &ConversationSurface) {
    let mut row = 0usize;
    for node in surface.nodes() {
        assert_eq!(node.rows().start, row);
        for node_row in node.rows() {
            assert_eq!(
                surface.node_at_row(node_row).map(super::SurfaceNode::id),
                Some(node.id())
            );
        }
        row = node.rows().end;
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
