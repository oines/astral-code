//! Prompt image preview overlay.
//!
//! This is the no-pixel branch of Grok Build's prompt image preview matrix:
//! while the prompt cursor is on or immediately after an image chip, display
//! its metadata and source path above the prompt. Terminal pixel protocols can
//! later sit behind this view without changing composer or app-server
//! semantics.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::AstralTheme;
use crate::composer::LocalImage;

const MIN_BOX_WIDTH: u16 = 28;
const BOX_HEIGHT: u16 = 6;

pub(crate) struct ImagePreviewOverlay<'a> {
    pub(crate) image: &'a LocalImage,
}

impl ImagePreviewOverlay<'_> {
    pub(crate) fn render(self, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
        if area.width < MIN_BOX_WIDTH || area.height < BOX_HEIGHT {
            return;
        }

        let width = ((area.width as f32) * 0.75) as u16;
        let width = width.clamp(MIN_BOX_WIDTH, area.width);
        let overlay = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.bottom().saturating_sub(BOX_HEIGHT),
            width,
            BOX_HEIGHT,
        );
        dim_area(buffer, area, theme);
        Clear.render(overlay, buffer);
        buffer.set_style(
            overlay,
            Style::default()
                .fg(theme.text_primary)
                .bg(theme.panel_background),
        );

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(
                Style::default()
                    .fg(theme.gray_dim)
                    .bg(theme.panel_background),
            )
            .style(Style::default().bg(theme.panel_background));
        let inner = block.inner(overlay);
        block.render(overlay, buffer);

        let format = image_format(self.image);
        let dimensions = self.image.dimensions.map_or_else(
            || "unknown".to_string(),
            |(width, height)| format!("{width} × {height}"),
        );
        let size = self
            .image
            .byte_len
            .map_or_else(|| "unknown".to_string(), format_bytes);
        let title = format!(" Image #{} ", self.image.display_number);
        let title_width = u16::try_from(UnicodeWidthStr::width(title.as_str())).unwrap_or(u16::MAX);
        let title_x = overlay.x + overlay.width.saturating_sub(title_width) / 2;
        buffer.set_line(
            title_x,
            overlay.y,
            &Line::from(title.bold()),
            title_width.min(overlay.width),
        );

        let path_width = usize::from(inner.width.saturating_sub(6));
        let path = truncate_path(&self.image.path.display().to_string(), path_width);
        Paragraph::new(vec![
            Line::from(vec!["Format: ".dim(), format.into()]),
            Line::from(vec!["Dimensions: ".dim(), dimensions.into()]),
            Line::from(vec!["Size: ".dim(), size.into()]),
            Line::from(vec!["Path: ".dim(), path.into()]),
        ])
        .style(
            Style::default()
                .fg(theme.text_primary)
                .bg(theme.panel_background),
        )
        .render(inner, buffer);
    }
}

fn image_format(image: &LocalImage) -> String {
    image
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .map_or_else(|| "unknown".to_string(), str::to_uppercase)
}

fn format_bytes(byte_len: u64) -> String {
    if byte_len >= 1_000_000 {
        format!("{:.1} MB", byte_len as f64 / 1_000_000.0)
    } else if byte_len >= 1_000 {
        format!("{:.1} KB", byte_len as f64 / 1_000.0)
    } else {
        format!("{byte_len} bytes")
    }
}

fn truncate_path(path: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(path) <= max_width {
        return path.to_string();
    }
    if max_width <= 1 {
        return "…".to_string();
    }

    let mut suffix = String::new();
    for grapheme in path.graphemes(true).rev() {
        let next_width = UnicodeWidthStr::width(suffix.as_str())
            .saturating_add(UnicodeWidthStr::width(grapheme));
        if next_width >= max_width {
            break;
        }
        suffix.insert_str(0, grapheme);
    }
    format!("…{suffix}")
}

fn dim_area(buffer: &mut Buffer, area: Rect, theme: AstralTheme) {
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.set_fg(theme.gray_dim);
            }
        }
    }
}
