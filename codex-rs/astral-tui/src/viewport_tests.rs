use astral_tui_scrollback::EntryRenderOptions;
use astral_tui_scrollback::LineJoiner;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

use super::ScrollDirection;
use super::SurfaceViewport;
use crate::ConversationState;
use crate::ConversationSurface;
use crate::SurfaceNodeId;

#[test]
fn manual_anchor_survives_reflow_and_follow_bottom_resumes_only_at_end() {
    let conversation = ConversationState::from_thread(&thread(vec![turn(
        "turn-1",
        vec![
            assistant(
                "first",
                "first line\nsecond line that wraps at a narrow width",
            ),
            assistant(
                "second",
                "newest answer with enough trailing content to keep the anchor away from the bottom",
            ),
        ],
    )]));
    let narrow = ConversationSurface::render(&conversation, EntryRenderOptions::new(20));
    let first = conversation.transcript().turns()[0].entries()[0].id();
    let first_node = narrow
        .nodes()
        .iter()
        .find(|node| node.id() == SurfaceNodeId::Entry(first))
        .expect("first entry");
    let wrapped_subrow = first_node
        .rendered()
        .lines()
        .iter()
        .position(|line| line.joiner_to_previous != LineJoiner::HardBreak)
        .expect("first entry should wrap at narrow width");
    let anchored_row = first_node.rows().start + wrapped_subrow;
    let expected_anchor = narrow
        .anchor_at_row(anchored_row)
        .expect("wrapped row should have an anchor");

    let mut viewport = SurfaceViewport::default();
    viewport.prepare(&narrow, 2);
    assert!(viewport.is_following_bottom());
    assert!(viewport.scroll_rows(&narrow, ScrollDirection::Up, narrow.row_count()));
    assert!(!viewport.is_following_bottom());
    assert!(viewport.scroll_rows(&narrow, ScrollDirection::Down, anchored_row));
    assert_eq!(viewport.top(), anchored_row);
    assert!(expected_anchor.sub_rows() > 0);

    let wide = ConversationSurface::render(&conversation, EntryRenderOptions::new(40));
    viewport.prepare(&wide, 2);
    let anchor = wide
        .anchor_at_row(viewport.top())
        .expect("manual top should remain anchored");
    assert_eq!(anchor.node(), SurfaceNodeId::Entry(first));
    assert_eq!(anchor.logical_line(), expected_anchor.logical_line());
    assert!(!viewport.is_following_bottom());

    assert!(viewport.scroll_rows(&wide, ScrollDirection::Down, wide.row_count()));
    assert!(!viewport.is_following_bottom());
    assert!(viewport.scroll_rows(&wide, ScrollDirection::Down, 1));
    assert!(viewport.is_following_bottom());
    assert_eq!(viewport.top(), wide.row_count().saturating_sub(2));
}

#[test]
fn selection_and_hover_use_surface_nodes_and_keep_selection_visible() {
    let conversation = ConversationState::from_thread(&thread(vec![turn(
        "turn-1",
        vec![
            assistant("first", "alpha bravo charlie delta"),
            assistant("second", "echo foxtrot golf hotel"),
        ],
    )]));
    let surface = ConversationSurface::render(&conversation, EntryRenderOptions::new(10));
    let entries = conversation.transcript().turns()[0].entries();
    let first = SurfaceNodeId::Entry(entries[0].id());
    let second = SurfaceNodeId::Entry(entries[1].id());
    let mut viewport = SurfaceViewport::default();
    viewport.prepare(&surface, 2);

    assert!(viewport.move_selection(&surface, ScrollDirection::Down));
    assert_eq!(viewport.selected(), Some(first));
    assert_eq!(viewport.top(), 0);
    assert!(viewport.move_selection(&surface, ScrollDirection::Down));
    assert_eq!(viewport.selected(), Some(second));
    assert!(viewport.top() > 0);
    assert!(viewport.move_selection(&surface, ScrollDirection::Down));
    assert_eq!(viewport.selected(), Some(second));
    assert!(viewport.is_following_bottom());
    assert!(!viewport.move_selection(&surface, ScrollDirection::Down));
    assert_eq!(
        viewport.hover_screen_row(&surface, 0),
        surface
            .node_at_row(viewport.top())
            .map(crate::SurfaceNode::id)
    );
    viewport.clear_hover();
    assert_eq!(viewport.hovered(), None);
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

fn assistant(id: &str, text: &str) -> ThreadItem {
    ThreadItem::AgentMessage {
        id: id.to_string(),
        text: text.to_string(),
        phase: None,
        memory_citation: None,
    }
}
