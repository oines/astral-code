use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;

use super::MarkdownStyle;
use super::render_markdown;

const MARKDOWN_FIXTURE: &str = r#"# Astral Markdown

Paragraph with **bold**, *italic*, ~~removed~~, `inline code`, and an [Astral link](https://example.com).

> A quoted line with **structure**.

1. First ordered item
2. Second ordered item
   - Nested bullet
- [x] Finished task
- [ ] Pending task

---

```rust
fn main() {
    println!("hello");
}
```
"#;

fn plain(lines: &[Line<'_>]) -> String {
    lines
        .iter()
        .map(|line| line.to_string().trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn grok_markdown_semantics_snapshot() {
    let rendered = render_markdown(MARKDOWN_FIXTURE, 52, MarkdownStyle::default());

    assert_snapshot!(plain(&rendered));
}

#[test]
fn width_change_reflows_the_same_markdown_snapshot() {
    let narrow = render_markdown(MARKDOWN_FIXTURE, 32, MarkdownStyle::default());
    let wide = render_markdown(MARKDOWN_FIXTURE, 72, MarkdownStyle::default());

    assert_snapshot!("grok_markdown_narrow", plain(&narrow));
    assert_snapshot!("grok_markdown_wide", plain(&wide));
}

#[test]
fn inline_styles_survive_width_aware_wrapping() {
    let rendered = render_markdown(MARKDOWN_FIXTURE, 38, MarkdownStyle::default());
    let spans = rendered
        .iter()
        .flat_map(|line| line.spans.iter())
        .collect::<Vec<_>>();

    let bold = spans
        .iter()
        .find(|span| span.content.contains("bold"))
        .expect("bold span");
    let italic = spans
        .iter()
        .find(|span| span.content.contains("italic"))
        .expect("italic span");
    let removed = spans
        .iter()
        .find(|span| span.content.contains("removed"))
        .expect("strikethrough span");
    let link = spans
        .iter()
        .find(|span| span.content.contains("Astral link"))
        .expect("link span");

    assert!(bold.style.add_modifier.contains(Modifier::BOLD));
    assert!(italic.style.add_modifier.contains(Modifier::ITALIC));
    assert!(removed.style.add_modifier.contains(Modifier::CROSSED_OUT));
    assert!(link.style.add_modifier.contains(Modifier::UNDERLINED));
}

#[test]
fn code_block_background_fills_each_visual_line() {
    let background = Color::Blue;
    let style = MarkdownStyle {
        code_background: Style::default().bg(background),
        ..MarkdownStyle::default()
    };
    let rendered = render_markdown("```\nlet value = 1;\n\nvalue\n```", 24, style);

    assert_eq!(rendered.len(), 3);
    assert!(
        rendered
            .iter()
            .all(|line| line.style.bg == Some(background) && line.width() == 24)
    );
}

#[test]
fn cjk_content_is_not_rewritten_with_spaces() {
    let rendered = render_markdown(
        "你好，我是 Astral，可以帮你读写代码。",
        10,
        MarkdownStyle::default(),
    );

    assert_eq!(
        rendered
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>(),
        vec!["你好，我是", "Astral，可", "以帮你读写", "代码。"]
    );
}
