use ratatui::layout::Rect;

use crate::request_user_input::RequestUserInputHit;

use super::PaneRow;
use super::RequestPane;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PaneHit {
    UserInput(RequestUserInputHit),
}

impl RequestPane<'_> {
    pub(crate) fn choice_hit_rows(self, area: Rect) -> Vec<(usize, Rect)> {
        self.hit_rows(area, |row| match row {
            PaneRow::Choice { index, .. } => Some(*index),
            _ => None,
        })
    }

    pub(crate) fn user_input_hit_rows(self, area: Rect) -> Vec<(RequestUserInputHit, Rect)> {
        self.hit_rows(area, |row| match row {
            PaneRow::Option {
                hit: Some(PaneHit::UserInput(hit)),
                ..
            }
            | PaneRow::Input {
                hit: Some(PaneHit::UserInput(hit)),
                ..
            } => Some(*hit),
            _ => None,
        })
    }

    fn hit_rows<T: Copy>(
        self,
        area: Rect,
        target: impl Fn(&PaneRow) -> Option<T>,
    ) -> Vec<(T, Rect)> {
        if area.is_empty() {
            return Vec::new();
        }
        self.content(area.height)
            .rows
            .iter()
            .enumerate()
            .filter_map(|(row, item)| {
                let target = target(item)?;
                let y = area
                    .y
                    .saturating_add(u16::try_from(row).unwrap_or(u16::MAX));
                (y < area.bottom()).then(|| {
                    (
                        target,
                        Rect::new(area.x.saturating_add(1), y, area.width.saturating_sub(1), 1),
                    )
                })
            })
            .collect()
    }
}
