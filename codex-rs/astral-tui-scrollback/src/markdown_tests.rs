use super::LineJoiner;
use super::MarkdownSyntaxTheme;
use super::highlight_fenced_code;
use super::render_literal_with_metadata;
use insta::assert_snapshot;
use ratatui::style::Style;

#[test]
fn literal_wrapping_preserves_source_joiners_for_selection() {
    let rendered = render_literal_with_metadata("alpha 中文 beta\nnext", 8, Style::default());
    let output = rendered
        .iter()
        .map(|line| {
            let text = line
                .line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>();
            format!("{:?}: {text}", line.joiner_to_previous)
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert_snapshot!(output, @r###"
    HardBreak: alpha
    Space: 中文
    Space: beta
    HardBreak: next
    "###);
    assert_eq!(rendered[0].joiner_to_previous, LineJoiner::HardBreak);
}

#[test]
fn fenced_highlighting_retains_multiple_token_styles() {
    let rendered = highlight_fenced_code(
        "fn main() { let answer = 42; }",
        "rust",
        MarkdownSyntaxTheme::Terminal,
    )
    .expect("Rust syntax");

    assert!(
        rendered
            .iter()
            .flatten()
            .filter_map(|span| span.style.fg)
            .count()
            > 1
    );
}
