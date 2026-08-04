use super::LineJoiner;
use super::MarkdownLink;
use super::MarkdownStyle;
use super::MarkdownSyntaxTheme;
use super::highlight_fenced_code;
use super::render_literal_with_metadata;
use super::render_markdown;
use super::render_markdown_with_metadata;
use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use ratatui::style::Style;

const MARKDOWN_FIXTURE: &str = r#"# Astral Markdown

Paragraph with **bold**, *italic*, ~~removed~~, `inline code`, and an [Astral link](https://example.com).

> A quoted line with **structure**.

1. First ordered item
2. Second ordered item
   - Nested bullet
- [x] Finished task
- [ ] Pending task

| Feature | State |
|:--|--:|
| Markdown | **ready** |
| Width | `52` |

---

```rust
fn main() {
    println!("hello");
}
```
"#;

fn plain(lines: &[ratatui::text::Line<'_>]) -> String {
    lines
        .iter()
        .map(|line| line.to_string().trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn parses_grok_markdown_structure_without_rewriting_source_order() {
    let rendered = render_markdown(MARKDOWN_FIXTURE, 52, MarkdownStyle::default());

    assert_snapshot!(plain(&rendered), @r###"
    Astral Markdown

    Paragraph with bold, italic, removed, inline code,
    and an Astral link.

    │ A quoted line with structure.

    1. First ordered item
    2. Second ordered item
      • Nested bullet
    ☑ Finished task
    ☐ Pending task

    ┌──────────┬───────┐
    │ Feature  │ State │
    ├──────────┼───────┤
    │ Markdown │ ready │
    ├──────────┼───────┤
    │ Width    │    52 │
    └──────────┴───────┘

    ───

    fn main() {
        println!("hello");
    }
    "###);
}

#[test]
fn wrapped_links_keep_one_stable_target() {
    let rendered = render_markdown_with_metadata(
        "[alpha beta](https://example.com)",
        5,
        MarkdownStyle::default(),
    );

    assert_eq!(
        rendered
            .iter()
            .map(|line| (line.line.to_string(), line.links.clone()))
            .collect::<Vec<_>>(),
        vec![
            (
                "alpha".to_string(),
                vec![MarkdownLink {
                    id: 0,
                    columns: 0..5,
                    target: "https://example.com".to_string(),
                }],
            ),
            (
                "beta".to_string(),
                vec![MarkdownLink {
                    id: 0,
                    columns: 0..4,
                    target: "https://example.com".to_string(),
                }],
            ),
        ]
    );
}

#[test]
fn wrapped_bare_web_links_keep_one_stable_target() {
    let target = "https://example.com/a/very/long/path";
    let rendered = render_markdown_with_metadata(target, 12, MarkdownStyle::default());

    assert_eq!(
        rendered
            .iter()
            .flat_map(|line| line.links.iter())
            .map(|link| (link.id, link.target.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, target), (0, target), (0, target)]
    );
}

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
