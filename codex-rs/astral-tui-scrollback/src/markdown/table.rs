//! Width-constrained box tables derived from Grok Build's Markdown table view.

use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;

use super::LineJoiner;
use super::MarkdownLine;
use super::MarkdownStyle;
use super::Segment;
use super::wrapping::wrap_segments;

type Cell = Vec<Segment>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MarkdownTableAlignment {
    #[default]
    None,
    Left,
    Center,
    Right,
}

/// A source-ordered Markdown table that chooses a grid or record layout from
/// the available terminal width.
#[derive(Debug, Default)]
pub struct MarkdownTable {
    alignments: Vec<MarkdownTableAlignment>,
    header: Vec<Cell>,
    rows: Vec<Vec<Cell>>,
}

impl MarkdownTable {
    pub fn new(alignments: Vec<MarkdownTableAlignment>) -> Self {
        Self {
            alignments,
            ..Self::default()
        }
    }

    pub fn set_header(&mut self, cells: Vec<Line<'static>>) {
        self.header = cells.into_iter().map(line_to_cell).collect();
    }

    pub fn push_row(&mut self, cells: Vec<Line<'static>>) {
        self.rows
            .push(cells.into_iter().map(line_to_cell).collect());
    }

    pub fn render(mut self, width: u16, style: MarkdownStyle) -> Vec<MarkdownLine> {
        let column_count = std::iter::once(&self.header)
            .chain(&self.rows)
            .map(Vec::len)
            .max()
            .unwrap_or(0);
        if column_count == 0 {
            return Vec::new();
        }
        normalize_row(&mut self.header, column_count);
        for row in &mut self.rows {
            normalize_row(row, column_count);
        }
        let overhead = column_count.saturating_mul(3).saturating_add(1);
        const MINIMUM_READABLE_COLUMN_WIDTH: usize = 4;
        if usize::from(width)
            <= overhead.saturating_add(column_count.saturating_mul(MINIMUM_READABLE_COLUMN_WIDTH))
        {
            return self.render_records(width, style);
        }
        let mut widths = natural_widths(&self.header, &self.rows, column_count);
        constrain_widths(&mut widths, usize::from(width).saturating_sub(overhead));
        self.render_grid(&widths, style)
    }

    fn render_grid(&self, widths: &[usize], style: MarkdownStyle) -> Vec<MarkdownLine> {
        let mut lines = vec![markdown_line(border_line(
            "┌",
            "┬",
            "┐",
            widths,
            style.table_border,
        ))];
        if !self.header.is_empty() {
            lines.extend(render_row(
                &self.header,
                widths,
                &self.alignments,
                style.table_header,
                style.table_border,
            ));
            lines.push(markdown_line(border_line(
                "├",
                "┼",
                "┤",
                widths,
                style.table_border,
            )));
        }
        for (index, row) in self.rows.iter().enumerate() {
            lines.extend(render_row(
                row,
                widths,
                &self.alignments,
                Style::default(),
                style.table_border,
            ));
            if index + 1 < self.rows.len() {
                lines.push(markdown_line(border_line(
                    "├",
                    "┼",
                    "┤",
                    widths,
                    style.table_border,
                )));
            }
        }
        lines.push(markdown_line(border_line(
            "└",
            "┴",
            "┘",
            widths,
            style.table_border,
        )));
        lines
    }

    fn render_records(&self, width: u16, style: MarkdownStyle) -> Vec<MarkdownLine> {
        let mut lines = Vec::new();
        for (row_index, row) in self.rows.iter().enumerate() {
            if row_index > 0 {
                lines.push(markdown_line(Line::default()));
            }
            for (column, cell) in row.iter().enumerate() {
                let label = self
                    .header
                    .get(column)
                    .map(plain_cell)
                    .filter(|label| !label.is_empty())
                    .unwrap_or_else(|| format!("Column {}", column + 1));
                let mut segments = vec![
                    Segment {
                        text: label,
                        style: style.table_header,
                        link: None,
                    },
                    Segment {
                        text: ": ".to_string(),
                        style: style.table_border,
                        link: None,
                    },
                ];
                segments.extend(cell.iter().cloned());
                lines.extend(
                    wrap_segments(&segments, usize::from(width))
                        .into_iter()
                        .map(Line::from)
                        .map(markdown_line),
                );
            }
        }
        lines
    }
}

fn line_to_cell(line: Line<'static>) -> Cell {
    line.spans
        .into_iter()
        .map(|span| Segment {
            text: span.content.into_owned(),
            style: line.style.patch(span.style),
            link: None,
        })
        .collect()
}

fn markdown_line(line: Line<'static>) -> MarkdownLine {
    MarkdownLine {
        line,
        joiner_to_previous: LineJoiner::HardBreak,
        links: Vec::new(),
    }
}

fn normalize_row(row: &mut Vec<Cell>, column_count: usize) {
    row.resize_with(column_count, Vec::new);
    row.truncate(column_count);
}

fn natural_widths(header: &[Cell], rows: &[Vec<Cell>], column_count: usize) -> Vec<usize> {
    let mut widths = vec![1; column_count];
    for row in std::iter::once(header).chain(rows.iter().map(Vec::as_slice)) {
        for (column, cell) in row.iter().enumerate() {
            widths[column] = widths[column].max(cell_width(cell));
        }
    }
    widths
}

fn constrain_widths(widths: &mut [usize], budget: usize) {
    while widths.iter().sum::<usize>() > budget {
        let Some((index, _)) = widths
            .iter()
            .enumerate()
            .filter(|(_, width)| **width > 1)
            .max_by_key(|(_, width)| **width)
        else {
            break;
        };
        widths[index] -= 1;
    }
}

fn render_row(
    cells: &[Cell],
    widths: &[usize],
    alignments: &[MarkdownTableAlignment],
    cell_style: Style,
    border_style: Style,
) -> Vec<MarkdownLine> {
    let wrapped = cells
        .iter()
        .zip(widths)
        .map(|(cell, width)| wrap_segments(cell, *width))
        .collect::<Vec<_>>();
    let row_height = wrapped.iter().map(Vec::len).max().unwrap_or(1).max(1);
    (0..row_height)
        .map(|line_index| {
            let mut spans = vec![Span::styled("│", border_style)];
            for (column, width) in widths.iter().copied().enumerate() {
                spans.push(Span::raw(" "));
                let mut content = wrapped
                    .get(column)
                    .and_then(|lines| lines.get(line_index))
                    .cloned()
                    .unwrap_or_default();
                for span in &mut content {
                    span.style = span.style.patch(cell_style);
                }
                let content_width = Line::from(content.clone()).width();
                let extra = width.saturating_sub(content_width);
                let alignment = alignments.get(column).copied().unwrap_or_default();
                let left = match alignment {
                    MarkdownTableAlignment::Center => extra / 2,
                    MarkdownTableAlignment::Right => extra,
                    MarkdownTableAlignment::None | MarkdownTableAlignment::Left => 0,
                };
                spans.push(Span::raw(" ".repeat(left)));
                spans.append(&mut content);
                spans.push(Span::raw(" ".repeat(extra.saturating_sub(left))));
                spans.push(Span::raw(" "));
                spans.push(Span::styled("│", border_style));
            }
            markdown_line(Line::from(spans))
        })
        .collect()
}

fn border_line(
    left: &str,
    joint: &str,
    right: &str,
    widths: &[usize],
    style: Style,
) -> Line<'static> {
    let mut text = left.to_string();
    for (index, width) in widths.iter().copied().enumerate() {
        text.push_str(&"─".repeat(width + 2));
        text.push_str(if index + 1 == widths.len() {
            right
        } else {
            joint
        });
    }
    Line::from(Span::styled(text, style))
}

fn cell_width(cell: &Cell) -> usize {
    Line::from(
        cell.iter()
            .map(|segment| Span::styled(segment.text.clone(), segment.style))
            .collect::<Vec<_>>(),
    )
    .width()
}

fn plain_cell(cell: &Cell) -> String {
    cell.iter()
        .map(|segment| segment.text.as_str())
        .collect::<String>()
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
