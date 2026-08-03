use super::MarkdownStyle;
use super::MarkdownTable;
use super::MarkdownTableAlignment;
use insta::assert_snapshot;
use ratatui::text::Line;

#[test]
fn table_uses_a_grid_when_readable_and_records_when_narrow() {
    let render = |width| {
        let mut table = MarkdownTable::new(vec![
            MarkdownTableAlignment::Left,
            MarkdownTableAlignment::Right,
        ]);
        table.set_header(vec![Line::from("工具"), Line::from("状态")]);
        table.push_row(vec![Line::from("apply_patch"), Line::from("完成")]);
        table.push_row(vec![Line::from("web.run"), Line::from("等待")]);
        table
            .render(width, MarkdownStyle::default())
            .into_iter()
            .map(|line| {
                line.line
                    .spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let output = format!("wide:\n{}\n\nnarrow:\n{}", render(40), render(15));
    assert_snapshot!(output, @r###"
    wide:
    ┌─────────────┬──────┐
    │ 工具        │ 状态 │
    ├─────────────┼──────┤
    │ apply_patch │ 完成 │
    ├─────────────┼──────┤
    │ web.run     │ 等待 │
    └─────────────┴──────┘

    narrow:
    工具:
    apply_patch
    状态: 完成

    工具: web.run
    状态: 等待
    "###);
}
