//! Screen-to-buffer mapping for prompt pointer interaction.

use std::ops::Range;

use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::chrome::prompt_layout;

pub(crate) fn prompt_cursor_at(
    text: &str,
    cursor_byte: usize,
    area: Rect,
    position: Position,
) -> Option<usize> {
    if area.width < 4
        || area.height < 3
        || !area.contains(position)
        || position.x == area.x
        || position.x >= area.right().saturating_sub(1)
        || position.y == area.y
        || position.y >= area.bottom().saturating_sub(1)
    {
        return None;
    }
    let layout = prompt_layout(text, cursor_byte, area.width.saturating_sub(4));
    let visible_rows = usize::from(area.height.saturating_sub(2));
    let first_visible = layout
        .cursor_row
        .saturating_sub(visible_rows.saturating_sub(1));
    let row = first_visible.saturating_add(usize::from(
        position.y.saturating_sub(area.y.saturating_add(1)),
    ));
    let range = layout.ranges.get(row)?;
    let content_x = area.x.saturating_add(2);
    let text_x = content_x.saturating_add(u16::from(row == 0) * 2);
    let column = usize::from(position.x.saturating_sub(text_x));
    Some(byte_at_display_column(text, range.clone(), column))
}

fn byte_at_display_column(text: &str, range: Range<usize>, column: usize) -> usize {
    let mut width = 0;
    for (offset, character) in text[range.clone()].char_indices() {
        let byte = range.start.saturating_add(offset);
        if column <= width {
            return byte;
        }
        let character_width = Line::from(character.to_string()).width();
        if column < width.saturating_add(character_width) {
            return byte.saturating_add(character.len_utf8());
        }
        width = width.saturating_add(character_width);
    }
    range.end
}
