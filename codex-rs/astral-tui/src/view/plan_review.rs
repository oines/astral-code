use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;

use super::AstralTheme;
use crate::plan_review::PlanReviewChoice;
use crate::plan_review::PlanReviewFocus;
use crate::plan_review::PlanReviewState;

pub(crate) struct PlanReviewPane<'a> {
    pub(crate) state: &'a PlanReviewState,
}

impl PlanReviewPane<'_> {
    const DECISION_HEIGHT: u16 = 4;
    const REVISION_HEIGHT: u16 = 2;

    pub(crate) fn height(&self) -> u16 {
        match self.state.focus() {
            PlanReviewFocus::Decision => Self::DECISION_HEIGHT,
            PlanReviewFocus::Revision => Self::REVISION_HEIGHT,
        }
    }

    pub(crate) fn render(self, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        buffer.set_style(area, Style::default().bg(theme.bg_base));
        let title = if self.state.has_plan() {
            "Implement this plan?"
        } else {
            "No plan written yet"
        };
        buffer.set_line(
            area.x,
            area.y,
            &Line::from(title.bold().fg(theme.text_secondary)),
            area.width,
        );
        match self.state.focus() {
            PlanReviewFocus::Decision => {
                render_choices(self.state, area, buffer, theme);
            }
            PlanReviewFocus::Revision if area.height > 1 => {
                let controls: Line<'static> = vec![
                    "enter".bold().fg(theme.text_secondary),
                    " request changes".dim(),
                    " · ".dim(),
                    "tab".bold().fg(theme.text_secondary),
                    " plan".dim(),
                    " · ".dim(),
                    "esc".bold().fg(theme.text_secondary),
                    " back".dim(),
                ]
                .into();
                buffer.set_line(area.x, area.y + 1, &controls, area.width);
            }
            PlanReviewFocus::Revision => {}
        }
    }
}

fn render_choices(state: &PlanReviewState, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
    for (index, choice) in PlanReviewChoice::ALL.iter().enumerate() {
        let y = area
            .y
            .saturating_add(1)
            .saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
        if y >= area.bottom() {
            break;
        }
        let selected = *choice == state.selection();
        let row = Rect::new(area.x, y, area.width, 1);
        let row_style = if selected {
            Style::default()
                .fg(theme.text_primary)
                .bg(theme.panel_selected)
        } else {
            Style::default().fg(theme.text_primary).bg(theme.bg_base)
        };
        buffer.set_style(row, row_style);
        let marker = if selected { "›" } else { " " };
        let description_width = u16::try_from(choice.description().chars().count())
            .unwrap_or(u16::MAX)
            .saturating_add(2);
        let show_description = area.width > description_width.saturating_add(34);
        let label = if show_description {
            format!("{:<34}", choice.label())
        } else {
            choice.label().to_string()
        };
        let mut line = vec![
            format!("{marker} {}. ", index + 1).set_style(row_style.fg(if selected {
                theme.accent_running
            } else {
                theme.gray
            })),
            label.set_style(row_style),
        ];
        if show_description {
            line.push(
                choice
                    .description()
                    .set_style(row_style.fg(theme.text_secondary)),
            );
        }
        buffer.set_line(area.x, y, &Line::from(line), area.width);
    }
}
