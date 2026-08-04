use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;

use super::AskUserPrompt;
use super::Focus;
use super::OTHER_LABEL;
use crate::ModalPresentation;
use crate::ModalShortcut;
use crate::ModalSizing;
use crate::ModalWindowConfig;

struct VisualRow {
    item: usize,
    line: Line<'static>,
}

impl AskUserPrompt {
    pub(in crate::prompt_interaction) fn render(
        &mut self,
        buffer: &mut Buffer,
        area: Rect,
        queue_len: usize,
        responding: bool,
    ) {
        self.item_hits.clear();
        self.editor_area = None;
        let title = self.title(queue_len);
        let tab_labels = if self.questions.len() > 1 {
            {
                self.questions
                    .iter()
                    .enumerate()
                    .map(|(index, question)| match question.header.trim() {
                        "" => format!("Question {}", index + 1),
                        header => header.to_string(),
                    })
                    .collect::<Vec<_>>()
            }
        } else {
            Default::default()
        };
        let tabs = tab_labels.iter().map(String::as_str).collect::<Vec<_>>();
        self.window.set_active_tab(self.active);
        let shortcuts = self.shortcuts();
        let mut sizing = ModalSizing::medium().compact();
        sizing.footer_rows = 1;
        let config = ModalWindowConfig::new(&title)
            .with_tabs(&tabs)
            .with_shortcuts(&shortcuts)
            .with_sizing(sizing)
            .with_presentation(ModalPresentation::Embedded);
        let Some(layout) = self.window.render(buffer, area, &config) else {
            return;
        };
        self.render_question(buffer, layout.content);
        if responding && !layout.footer.is_empty() {
            buffer.set_line(
                layout.footer.x,
                layout.footer.y,
                &Line::from("Sending answer…").dim(),
                layout.footer.width,
            );
        }
    }

    fn title(&self, queue_len: usize) -> String {
        let mut title = if self.questions.len() == 1 {
            let header = self.questions[0].header.trim();
            if header.is_empty() {
                "Answer question".to_string()
            } else {
                format!("Answer question · {header}")
            }
        } else {
            format!(
                "Answer questions · {}/{}",
                self.active.saturating_add(1),
                self.questions.len()
            )
        };
        if queue_len > 1 {
            title.push_str(&format!(" · {queue_len} requests waiting"));
        }
        title
    }

    fn shortcuts(&self) -> Vec<ModalShortcut<'static>> {
        if self.focus == Focus::Notes {
            vec![
                ModalShortcut::hint("Enter next/submit"),
                ModalShortcut::hint("Shift+Enter newline"),
                ModalShortcut::hint("Esc back"),
            ]
        } else {
            vec![
                ModalShortcut::hint("↑/↓ navigate"),
                ModalShortcut::hint("Space select"),
                ModalShortcut::hint("Enter next/submit"),
                ModalShortcut::hint("z notes"),
            ]
        }
    }

    fn render_question(&mut self, buffer: &mut Buffer, area: Rect) {
        let Some(question) = self.current_question().cloned() else {
            let message = Line::from("No questions were supplied. Press Enter to continue.").dim();
            buffer.set_line(area.x, area.y, &message, area.width);
            return;
        };
        let wrapped = textwrap::wrap(&question.question, usize::from(area.width.max(1)));
        let question_rows = wrapped.len().min(usize::from(area.height));
        for (offset, line) in wrapped.into_iter().take(question_rows).enumerate() {
            buffer.set_line(
                area.x,
                area.y + offset as u16,
                &Line::from(line.into_owned()).bold(),
                area.width,
            );
        }
        let top = area
            .y
            .saturating_add(question_rows as u16)
            .saturating_add(1);
        if top >= area.bottom() {
            return;
        }
        let editor_height = if self.focus == Focus::Notes {
            area.bottom().saturating_sub(top).min(3)
        } else {
            0
        };
        let list_height = area.bottom().saturating_sub(top + editor_height);
        let list = Rect::new(area.x, top, area.width, list_height);
        let rows = self.visual_rows(&question, usize::from(list.width.max(1)));
        self.render_rows(buffer, list, rows);
        if editor_height > 0 {
            self.render_editor(
                buffer,
                Rect::new(area.x, list.bottom(), area.width, editor_height),
                question.is_secret,
            );
        }
    }

    fn visual_rows(
        &self,
        question: &codex_app_server_protocol::ToolRequestUserInputQuestion,
        width: usize,
    ) -> Vec<VisualRow> {
        let mut rows = Vec::new();
        let option_count = self.option_count();
        for index in 0..option_count {
            let (label, description) = question
                .options
                .as_ref()
                .and_then(|options| options.get(index))
                .map(|option| (option.label.as_str(), option.description.as_str()))
                .unwrap_or((OTHER_LABEL, "Optionally explain your choice in notes."));
            let selected = self.answer().selected.as_deref() == Some(label);
            let focused = self.answer().cursor == index;
            let marker = if selected { "(●)" } else { "(○)" };
            let prefix = if focused { "›" } else { " " };
            let shortcut = if index < 9 {
                char::from(b'1' + index as u8).to_string()
            } else {
                "·".to_string()
            };
            let style = if focused {
                Style::default().cyan().bold()
            } else if self.hovered == Some(index) {
                Style::default().reversed()
            } else {
                Style::default()
            };
            rows.push(VisualRow {
                item: index,
                line: Line::from(format!("{prefix} {shortcut} {marker} {label}")).style(style),
            });
            if focused && !description.trim().is_empty() {
                rows.extend(
                    textwrap::wrap(description, width.saturating_sub(6).max(1))
                        .into_iter()
                        .take(3)
                        .map(|line| VisualRow {
                            item: index,
                            line: Line::from(format!("      {}", line.into_owned())).dim(),
                        }),
                );
            }
        }
        if self.focus != Focus::Notes {
            let index = option_count;
            let preview = self.answer().note.lines().next().unwrap_or("");
            let label = if preview.is_empty() {
                "Add notes".to_string()
            } else if question.is_secret {
                "Edit secret answer".to_string()
            } else {
                format!("Edit notes · {preview}")
            };
            let focused = self.answer().cursor == index;
            let style = if focused {
                Style::default().cyan().bold()
            } else if self.hovered == Some(index) {
                Style::default().reversed()
            } else {
                Style::default()
            };
            let prefix = if focused { "›" } else { " " };
            rows.push(VisualRow {
                item: index,
                line: Line::from(format!("{prefix} z › {label}")).style(style),
            });
        }
        rows
    }

    fn render_rows(&mut self, buffer: &mut Buffer, area: Rect, rows: Vec<VisualRow>) {
        if area.is_empty() {
            return;
        }
        let cursor = self.answer().cursor;
        let first = rows.iter().position(|row| row.item == cursor).unwrap_or(0);
        let last = rows
            .iter()
            .rposition(|row| row.item == cursor)
            .unwrap_or(first);
        let max_scroll = rows.len().saturating_sub(usize::from(area.height));
        let mut scroll = self.answer().scroll.min(max_scroll);
        if first < scroll {
            scroll = first;
        } else if last >= scroll + usize::from(area.height) {
            scroll = last + 1 - usize::from(area.height);
        }
        self.answer_mut().scroll = scroll;
        for (offset, row) in rows
            .into_iter()
            .skip(scroll)
            .take(usize::from(area.height))
            .enumerate()
        {
            let rect = Rect::new(area.x, area.y + offset as u16, area.width, 1);
            buffer.set_line(rect.x, rect.y, &row.line, rect.width);
            self.item_hits.push((rect, row.item));
        }
    }

    fn render_editor(&mut self, buffer: &mut Buffer, area: Rect, secret: bool) {
        self.editor_area = Some(area);
        let text = if secret {
            self.answer()
                .note
                .chars()
                .map(|character| if character == '\n' { '\n' } else { '•' })
                .collect::<String>()
        } else {
            self.answer().note.clone()
        };
        if text.is_empty() {
            let prompt = Line::from("z › Type your answer").dim();
            buffer.set_line(area.x, area.y, &prompt, area.width);
            return;
        }
        let options = textwrap::Options::new(usize::from(area.width.max(1)))
            .initial_indent("z › ")
            .subsequent_indent("    ");
        let lines = textwrap::wrap(&text, options);
        let visible = lines.len().min(usize::from(area.height));
        let skip = lines.len().saturating_sub(visible);
        for (offset, line) in lines.into_iter().skip(skip).enumerate() {
            buffer.set_line(
                area.x,
                area.y + offset as u16,
                &Line::from(line.into_owned()),
                area.width,
            );
        }
    }
}
