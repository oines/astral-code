use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Stylize;

use super::view::AgentViewLayout;
use super::view::AgentViewLayoutInput;
use super::view::AstralTheme;
use super::view::LayoutConfig;
use super::view::PaneHeights;
use super::view::PlanReviewPane;
use super::view::PromptChrome;
use super::view::ScrollbarConfig;
use super::view::ShortcutsBar;
use super::view::StatusBar;
use crate::PromptSubmission;
use crate::composer::ComposerElement;
use crate::plan_review::PlanReviewState;

#[test]
fn standard_agent_view_geometry_matches_the_ported_layout() {
    let actual = AgentViewLayout::compute(AgentViewLayoutInput {
        area: Rect::new(0, 0, 80, 24),
        layout: LayoutConfig::default(),
        scrollbar: ScrollbarConfig::default(),
        panes: PaneHeights {
            prompt: 3,
            turn_status: 1,
            prompt_gap: 1,
            shortcuts: 1,
            ..PaneHeights::default()
        },
        timeline_width: 0,
        compact: false,
    });
    assert_eq!(
        actual,
        AgentViewLayout {
            status_bar: Rect::new(2, 1, 76, 1),
            scrollback: Rect::new(2, 3, 76, 12),
            turn_status: Rect::new(2, 16, 76, 1),
            prompt: Rect::new(2, 18, 76, 3),
            shortcuts: Rect::new(2, 22, 76, 1),
            scrollback_content: Rect::new(2, 3, 76, 12),
            scrollbar_x: 79,
            timeline_x: 80,
            ..AgentViewLayout::default()
        }
    );
}

#[test]
fn short_terminal_keeps_prompt_and_shortcuts_visible() {
    let actual = AgentViewLayout::compute(AgentViewLayoutInput {
        area: Rect::new(0, 0, 48, 16),
        layout: LayoutConfig::default(),
        scrollbar: ScrollbarConfig::default(),
        panes: PaneHeights {
            prompt: 3,
            turn_status: 1,
            shortcuts: 1,
            ..PaneHeights::default()
        },
        timeline_width: 0,
        compact: false,
    });

    assert_eq!(actual.prompt, Rect::new(1, 12, 46, 3));
    assert_eq!(actual.shortcuts, Rect::new(1, 15, 46, 1));
    assert_eq!(actual.scrollback, Rect::new(1, 1, 46, 9));
}

#[test]
fn view_chrome_snapshot() {
    let theme = AstralTheme::default();
    let area = Rect::new(0, 0, 80, 6);
    let mut buffer = Buffer::empty(area);
    StatusBar {
        left: vec!["⎇ main".dim(), "  ".into(), "~/project/astral-code".dim()].into(),
        right: Some("9.2K / 500K".dim().into()),
    }
    .render(Rect::new(0, 0, 80, 1), &mut buffer, theme);
    let cursor = PromptChrome {
        text: "trace the projection",
        cursor_byte: "trace the projection".len(),
        title: Some("Astral session"),
        model: "claude-sonnet-4",
        flags: &["anthropic"],
        ghost: None,
        focused: true,
        selection: None,
        elements: &[],
    }
    .render(Rect::new(0, 1, 80, 3), &mut buffer, theme);
    ShortcutsBar {
        hints: &[("Enter", "send"), ("Ctrl+.", "shortcuts")],
        right: Some("claude-sonnet-4 · anthropic"),
    }
    .render(Rect::new(0, 5, 80, 1), &mut buffer, theme);

    assert_eq!(cursor, Some(Position::new(24, 2)));
    insta::assert_snapshot!(buffer_text(&buffer));
}

#[test]
fn prompt_wrap_and_mid_buffer_cursor_snapshot() {
    let theme = AstralTheme::default();
    let area = Rect::new(0, 0, 34, 6);
    let mut buffer = Buffer::empty(area);
    let text = "A long prompt wraps before this\nand keeps editing here";
    let cursor_byte = "A long prompt wraps before this\nand keeps ".len();

    let cursor = PromptChrome {
        text,
        cursor_byte,
        title: None,
        model: "gpt-5",
        flags: &["default"],
        ghost: None,
        focused: true,
        selection: None,
        elements: &[],
    }
    .render(area, &mut buffer, theme);

    assert_eq!(cursor, Some(Position::new(12, 3)));
    insta::assert_snapshot!(buffer_text(&buffer));
}

#[test]
fn wrapped_prompt_selection_snapshot() {
    let theme = AstralTheme::default();
    let area = Rect::new(0, 0, 24, 6);
    let mut buffer = Buffer::empty(area);
    let text = "alpha beta gamma delta";
    PromptChrome {
        text,
        cursor_byte: text.len(),
        title: None,
        model: "gpt-5",
        flags: &[],
        ghost: None,
        focused: true,
        selection: Some(6..16),
        elements: &[],
    }
    .render(area, &mut buffer, theme);

    insta::assert_snapshot!(format!(
        "{}\n\nselection mask:\n{}",
        buffer_text(&buffer),
        selection_mask(&buffer, theme.prompt_selection_background)
    ));
}

#[test]
fn paste_chip_prompt_snapshot() {
    let theme = AstralTheme::default();
    let area = Rect::new(0, 0, 52, 4);
    let mut buffer = Buffer::empty(area);
    let placeholder = "[Pasted: 4 lines]";
    let text = format!("inspect {placeholder} before submitting");
    let start = "inspect ".len();
    let elements = vec![ComposerElement::paste(
        start..start + placeholder.len(),
        placeholder.to_string(),
        "one\ntwo\nthree\nfour".to_string(),
    )];

    PromptChrome {
        text: &text,
        cursor_byte: start + placeholder.len(),
        title: None,
        model: "gpt-5",
        flags: &[],
        ghost: None,
        focused: true,
        selection: None,
        elements: &elements,
    }
    .render(area, &mut buffer, theme);

    insta::assert_snapshot!(format!(
        "{}\n\nchip mask:\n{}",
        buffer_text(&buffer),
        selection_mask(&buffer, theme.panel_selected)
    ));
}

#[test]
fn plan_review_controls_snapshot() {
    let theme = AstralTheme::default();
    let state = PlanReviewState::new(
        "# Plan\n- trace\n- implement".to_string(),
        PromptSubmission::text_only(""),
    );
    let pane = PlanReviewPane { state: &state };
    let area = Rect::new(0, 0, 96, pane.height());
    let mut buffer = Buffer::empty(area);

    pane.render(area, &mut buffer, theme);

    insta::assert_snapshot!(buffer_text(&buffer));
}

#[test]
fn plan_revision_controls_snapshot() {
    let theme = AstralTheme::default();
    let mut state = PlanReviewState::new(
        "# Plan\n- trace\n- implement".to_string(),
        PromptSubmission::text_only(""),
    );
    state.begin_revision();
    let pane = PlanReviewPane { state: &state };
    let area = Rect::new(0, 0, 72, pane.height());
    let mut buffer = Buffer::empty(area);

    pane.render(area, &mut buffer, theme);

    insta::assert_snapshot!(buffer_text(&buffer));
}

fn buffer_text(buffer: &Buffer) -> String {
    let area = buffer.area;
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

fn selection_mask(buffer: &Buffer, selection_background: Color) -> String {
    let area = buffer.area;
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| {
                    if buffer[(x, y)].bg == selection_background {
                        '^'
                    } else {
                        ' '
                    }
                })
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}
