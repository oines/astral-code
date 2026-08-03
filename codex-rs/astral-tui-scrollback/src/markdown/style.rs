use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MarkdownSyntaxTheme {
    #[default]
    Night,
    Day,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkdownStyle {
    pub text: Style,
    pub headings: [Style; 6],
    pub strong: Style,
    pub emphasis: Style,
    pub strikethrough: Style,
    pub inline_code: Style,
    pub blockquote: Style,
    pub list_marker: Style,
    pub task_checked: Style,
    pub task_unchecked: Style,
    pub rule: Style,
    pub link_text: Style,
    pub link_url: Style,
    pub code: Style,
    pub code_background: Style,
    pub syntax_theme: MarkdownSyntaxTheme,
    pub table_border: Style,
    pub table_header: Style,
}

impl Default for MarkdownStyle {
    fn default() -> Self {
        Self {
            text: Style::default(),
            headings: [
                Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                Style::default().add_modifier(Modifier::BOLD),
                Style::default().add_modifier(Modifier::BOLD | Modifier::ITALIC),
                Style::default().add_modifier(Modifier::ITALIC),
                Style::default().add_modifier(Modifier::ITALIC),
                Style::default().add_modifier(Modifier::ITALIC),
            ],
            strong: Style::default().add_modifier(Modifier::BOLD),
            emphasis: Style::default().add_modifier(Modifier::ITALIC),
            strikethrough: Style::default().add_modifier(Modifier::CROSSED_OUT),
            inline_code: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            blockquote: Style::default().fg(Color::DarkGray),
            list_marker: Style::default().fg(Color::DarkGray),
            task_checked: Style::default().fg(Color::Green),
            task_unchecked: Style::default().fg(Color::DarkGray),
            rule: Style::default().fg(Color::DarkGray),
            link_text: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::UNDERLINED),
            link_url: Style::default().fg(Color::DarkGray),
            code: Style::default().fg(Color::Gray),
            code_background: Style::default().bg(Color::Black),
            syntax_theme: MarkdownSyntaxTheme::Night,
            table_border: Style::default().fg(Color::DarkGray),
            table_header: Style::default().add_modifier(Modifier::BOLD),
        }
    }
}
