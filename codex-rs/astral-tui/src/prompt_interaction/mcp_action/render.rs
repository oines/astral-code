use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;

use super::McpActionPrompt;
use super::Stage;
use crate::ModalPresentation;
use crate::ModalShortcut;
use crate::ModalSizing;
use crate::ModalWindowConfig;

impl McpActionPrompt {
    pub(in crate::prompt_interaction) fn desired_height(&self, width: u16, available: u16) -> u16 {
        let content_width = usize::from(width.saturating_sub(2).max(1));
        let body_rows = self.body_lines(content_width).len();
        (body_rows + self.options().len() + 2)
            .min(usize::from(available))
            .max(7.min(usize::from(available))) as u16
    }

    pub(in crate::prompt_interaction) fn render(
        &mut self,
        buffer: &mut Buffer,
        area: Rect,
        queue_len: usize,
        responding: bool,
    ) {
        self.choices.begin_frame();
        let mut title = if self.stage == Stage::WaitingForBrowser {
            "Finish in browser".to_string()
        } else {
            format!("{} · {}", self.title, self.server_name)
        };
        if queue_len > 1 {
            title.push_str(&format!(" · {queue_len} requests waiting"));
        }
        let shortcuts = [
            ModalShortcut::hint("↑/↓ navigate"),
            ModalShortcut::hint("Enter confirm"),
            ModalShortcut::hint("Esc cancel"),
        ];
        let mut sizing = ModalSizing::medium().compact();
        sizing.footer_rows = 1;
        let config = ModalWindowConfig::new(&title)
            .with_shortcuts(&shortcuts)
            .with_sizing(sizing)
            .with_presentation(ModalPresentation::Embedded);
        let Some(layout) = self.window.render(buffer, area, &config) else {
            return;
        };
        let option_rows = (self.options().len() as u16).min(layout.content.height);
        let options_y = layout.content.bottom().saturating_sub(option_rows);
        let body_area = Rect::new(
            layout.content.x,
            layout.content.y,
            layout.content.width,
            options_y.saturating_sub(layout.content.y),
        );
        for (offset, line) in self
            .body_lines(usize::from(body_area.width.max(1)))
            .into_iter()
            .take(usize::from(body_area.height))
            .enumerate()
        {
            buffer.set_line(
                body_area.x,
                body_area.y + offset as u16,
                &line,
                body_area.width,
            );
        }
        for index in 0..usize::from(option_rows) {
            let row = Rect::new(
                layout.content.x,
                options_y + index as u16,
                layout.content.width,
                1,
            );
            let option = &self.options()[index];
            let line = Line::from(format!(
                "{}{}. {}",
                self.choices.prefix(index),
                index + 1,
                option.label
            ))
            .style(self.choices.style(index));
            buffer.set_line(row.x, row.y, &line, row.width);
            self.choices.record_hit(row);
        }
        if responding && !layout.footer.is_empty() {
            buffer.set_line(
                layout.footer.x,
                layout.footer.y,
                &Line::from("Sending response…").dim(),
                layout.footer.width,
            );
        }
    }

    fn body_lines(&self, width: usize) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        for section in self.body.iter().filter(|section| !section.is_empty()) {
            lines.extend(
                textwrap::wrap(section, width.max(1))
                    .into_iter()
                    .map(|line| Line::from(line.into_owned())),
            );
        }
        if let Some(url) = self.displayed_url.as_deref() {
            lines.push(Line::default());
            lines.push("Setup URL".dim().into());
            let safe = self.safe_url.is_some();
            for line in textwrap::wrap(url, width.max(1)) {
                let line = Line::from(line.into_owned());
                lines.push(if safe {
                    line.cyan().underlined()
                } else {
                    line.red()
                });
            }
            if !safe {
                lines.push(
                    "Blocked: only credential-free HTTPS URLs can be opened."
                        .red()
                        .into(),
                );
            }
        }
        if self.stage == Stage::WaitingForBrowser {
            lines.push(Line::default());
            lines.extend(
                textwrap::wrap(
                    "Complete setup in the browser, then return here and confirm.",
                    width.max(1),
                )
                .into_iter()
                .map(|line| Line::from(line.into_owned())),
            );
        }
        lines
    }
}
