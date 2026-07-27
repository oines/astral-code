use pretty_assertions::assert_eq;
use ratatui::style::Color;

use super::MarkdownSyntaxTheme;
use super::find_syntax;
use super::highlight_code;

#[test]
fn resolves_language_aliases_and_line_citation_paths() {
    assert_eq!(
        find_syntax("rust").map(|syntax| syntax.name.as_str()),
        Some("Rust")
    );
    assert_eq!(
        find_syntax("python3").map(|syntax| syntax.name.as_str()),
        Some("Python")
    );
    assert_eq!(
        find_syntax("37:65:src/main.rs").map(|syntax| syntax.name.as_str()),
        Some("Rust")
    );
    assert!(find_syntax("not-a-real-language").is_none());
}

#[test]
fn highlighted_rust_has_multiple_token_colors() {
    let lines = highlight_code(
        "fn main() { let answer = 42; }",
        "rust",
        MarkdownSyntaxTheme::Night,
    )
    .expect("Rust syntax");
    let colors = lines
        .iter()
        .flatten()
        .filter_map(|segment| segment.style.fg)
        .collect::<std::collections::HashSet<_>>();

    assert!(colors.len() > 1);
}

#[test]
fn terminal_highlighting_uses_polarity_safe_colors() {
    let lines = highlight_code(
        "fn main() { let answer = 42; }",
        "rust",
        MarkdownSyntaxTheme::Terminal,
    )
    .expect("Rust syntax");
    let allowed = [
        Color::Reset,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
    ];

    assert!(lines.iter().flatten().all(|segment| {
        segment
            .style
            .fg
            .is_some_and(|color| allowed.contains(&color))
    }));
}
