use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;

use super::AstralTheme;
use crate::plan_review::PlanReviewFocus;
use crate::plan_review::PlanReviewState;

pub(crate) struct PlanReviewPane<'a> {
    pub(crate) state: &'a PlanReviewState,
}

impl PlanReviewPane<'_> {
    pub(crate) const HEIGHT: u16 = 2;

    pub(crate) fn render(self, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        buffer.set_style(area, Style::default().bg(theme.bg_base));
        let title = if self.state.has_plan() {
            "Plan ready for review"
        } else {
            "No plan written yet"
        };
        buffer.set_line(
            area.x,
            area.y,
            &Line::from(title.bold().fg(theme.text_secondary)),
            area.width,
        );
        if area.height > 1 {
            let controls: Line<'static> = match self.state.focus() {
                PlanReviewFocus::Decision => vec![
                    "enter".bold().fg(theme.text_secondary),
                    " implement".dim(),
                    " · ".dim(),
                    "c".bold().fg(theme.text_secondary),
                    " fresh context".dim(),
                    " · ".dim(),
                    "s".bold().fg(theme.text_secondary),
                    " revise".dim(),
                    " · ".dim(),
                    "q".bold().fg(theme.text_secondary),
                    " keep planning".dim(),
                ]
                .into(),
                PlanReviewFocus::Revision => vec![
                    "enter".bold().fg(theme.text_secondary),
                    " request changes".dim(),
                    " · ".dim(),
                    "tab".bold().fg(theme.text_secondary),
                    " plan".dim(),
                    " · ".dim(),
                    "esc".bold().fg(theme.text_secondary),
                    " back".dim(),
                ]
                .into(),
            };
            buffer.set_line(area.x, area.y + 1, &controls, area.width);
        }
    }
}
