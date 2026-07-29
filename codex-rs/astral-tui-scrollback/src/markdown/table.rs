//! Width-constrained box tables derived from Grok Build's Markdown table view.

use pulldown_cmark::Alignment;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;

use super::MarkdownStyle;
use super::Segment;
use super::wrapping::wrap_segments;

type Cell = Vec<Segment>;

#[derive(Debug)]
pub(super) struct TableState {
    alignments: Vec<Alignment>,
    header: Vec<Cell>,
    rows: Vec<Vec<Cell>>,
    current_row: Vec<Cell>,
    current_cell: Option<Cell>,
    in_header: bool,
}

impl TableState {
    pub(super) fn new(alignments: Vec<Alignment>) -> Self {
        Self {
            alignments,
            header: Vec::new(),
            rows: Vec::new(),
            current_row: Vec::new(),
            current_cell: None,
            in_header: false,
        }
    }

    pub(super) fn start_head(&mut self) {
        self.in_header = true;
        self.current_row.clear();
    }

    pub(super) fn end_head(&mut self) {
        self.finish_cell();
        self.header = std::mem::take(&mut self.current_row);
        self.in_header = false;
    }

    pub(super) fn start_row(&mut self) {
        self.current_row.clear();
    }

    pub(super) fn end_row(&mut self) {
        self.finish_cell();
        let row = std::mem::take(&mut self.current_row);
        if self.in_header {
            self.header = row;
        } else {
            self.rows.push(row);
        }
    }

    pub(super) fn start_cell(&mut self) {
        self.current_cell = Some(Vec::new());
    }

    pub(super) fn finish_cell(&mut self) {
        if let Some(cell) = self.current_cell.take() {
            self.current_row.push(cell);
        }
    }

    pub(super) fn push(&mut self, segment: Segment) -> bool {
        let Some(cell) = self.current_cell.as_mut() else {
            return false;
        };
        cell.push(segment);
        true
    }

    pub(super) fn render(mut self, width: u16, style: MarkdownStyle) -> Vec<Line<'static>> {
        self.finish_cell();
        if !self.current_row.is_empty() {
            self.end_row();
        }
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

    fn render_grid(&self, widths: &[usize], style: MarkdownStyle) -> Vec<Line<'static>> {
        let mut lines = vec![border_line("┌", "┬", "┐", widths, style.table_border)];
        if !self.header.is_empty() {
            lines.extend(render_row(
                &self.header,
                widths,
                &self.alignments,
                style.table_header,
                style.table_border,
            ));
            lines.push(border_line("├", "┼", "┤", widths, style.table_border));
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
                lines.push(border_line("├", "┼", "┤", widths, style.table_border));
            }
        }
        lines.push(border_line("└", "┴", "┘", widths, style.table_border));
        lines
    }

    fn render_records(&self, width: u16, style: MarkdownStyle) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        for (row_index, row) in self.rows.iter().enumerate() {
            if row_index > 0 {
                lines.push(Line::default());
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
                        .map(Line::from),
                );
            }
        }
        lines
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
    alignments: &[Alignment],
    cell_style: Style,
    border_style: Style,
) -> Vec<Line<'static>> {
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
                let alignment = alignments.get(column).copied().unwrap_or(Alignment::None);
                let left = match alignment {
                    Alignment::Center => extra / 2,
                    Alignment::Right => extra,
                    Alignment::None | Alignment::Left => 0,
                };
                spans.push(Span::raw(" ".repeat(left)));
                spans.append(&mut content);
                spans.push(Span::raw(" ".repeat(extra.saturating_sub(left))));
                spans.push(Span::raw(" "));
                spans.push(Span::styled("│", border_style));
            }
            Line::from(spans)
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
