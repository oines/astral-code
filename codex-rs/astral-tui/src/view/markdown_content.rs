use astral_tui_scrollback::BlockTextMode;
use astral_tui_scrollback::MarkdownLine;
use astral_tui_scrollback::MarkdownStyle;
use astral_tui_scrollback::render_literal_with_metadata;
use astral_tui_scrollback::render_markdown_with_metadata;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Span;

use super::AstralTheme;

pub(super) fn render_markdown_content(
    text: &str,
    width: u16,
    theme: AstralTheme,
    mode: BlockTextMode,
    indent: &'static str,
) -> Vec<MarkdownLine> {
    let body_width = width.saturating_sub(indent.len() as u16).max(1);
    let style = markdown_style(theme);
    let mut lines = match mode {
        BlockTextMode::Rendered => render_markdown_with_metadata(text, body_width, style),
        BlockTextMode::Raw => render_literal_with_metadata(text, body_width, style.text),
    };
    if !indent.is_empty() {
        let indent_width = u16::try_from(Span::raw(indent).width()).unwrap_or(u16::MAX);
        for line in &mut lines {
            line.line.spans.insert(0, Span::styled(indent, style.text));
            for link in &mut line.links {
                link.columns = link.columns.start.saturating_add(indent_width)
                    ..link.columns.end.saturating_add(indent_width);
            }
        }
    }
    lines
}

fn markdown_style(theme: AstralTheme) -> MarkdownStyle {
    let primary = Style::default().fg(theme.text_primary);
    let secondary = Style::default().fg(theme.text_secondary);
    let gray = Style::default().fg(theme.gray);
    MarkdownStyle {
        text: primary,
        headings: [
            primary.add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            primary.add_modifier(Modifier::BOLD),
            primary.add_modifier(Modifier::BOLD | Modifier::ITALIC),
            secondary.add_modifier(Modifier::BOLD),
            secondary.add_modifier(Modifier::ITALIC),
            secondary.add_modifier(Modifier::ITALIC),
        ],
        strong: primary.add_modifier(Modifier::BOLD),
        emphasis: primary.add_modifier(Modifier::ITALIC),
        strikethrough: secondary.add_modifier(Modifier::CROSSED_OUT),
        inline_code: Style::default()
            .fg(theme.accent_running)
            .add_modifier(Modifier::BOLD),
        blockquote: gray,
        list_marker: gray,
        task_checked: Style::default().fg(theme.accent_running),
        task_unchecked: gray,
        rule: Style::default().fg(theme.gray_dim),
        link_text: Style::default()
            .fg(theme.accent_running)
            .add_modifier(Modifier::UNDERLINED),
        link_url: gray,
        code: secondary,
        code_background: Style::default().bg(theme.panel_background),
        syntax_theme: theme.syntax_theme,
        table_border: Style::default().fg(theme.gray_dim),
        table_header: primary.add_modifier(Modifier::BOLD),
    }
}
