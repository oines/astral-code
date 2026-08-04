use codex_app_server_protocol::McpElicitationStringFormat;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;

use super::DECLINE_ACTION;
use super::McpFormPrompt;
use crate::ModalPresentation;
use crate::ModalShortcut;
use crate::ModalSizing;
use crate::ModalWindowConfig;
use crate::prompt_interaction::mcp_form::field::McpFormControl;
use crate::prompt_interaction::mcp_form::field::McpFormField;
use crate::prompt_interaction::mcp_form::field::McpFormTextKind;

impl McpFormPrompt {
    pub(in crate::prompt_interaction) fn desired_height(&self, width: u16, available: u16) -> u16 {
        if available == 0 {
            return 0;
        }
        let content_width = usize::from(width.saturating_sub(2).max(1));
        let message_rows = textwrap::wrap(&self.message, content_width).len().min(4);
        let description_rows = self
            .model
            .active_field()
            .and_then(|field| field.description.as_deref())
            .map_or(0, |text| textwrap::wrap(text, content_width).len().min(2));
        let input_rows = self
            .model
            .active_field()
            .map_or(1, |field| match &field.control {
                McpFormControl::Text { .. } => 1,
                McpFormControl::Select { options, .. } => options.len().clamp(1, 6),
            });
        let tabs = usize::from(self.model.field_count() > 1) * 2;
        let desired = message_rows + description_rows + input_rows + tabs + 7;
        (desired as u16).min(available).max(8.min(available))
    }

    pub(in crate::prompt_interaction) fn render(
        &mut self,
        buffer: &mut Buffer,
        area: Rect,
        queue_len: usize,
        responding: bool,
    ) {
        let mut title = format!("Provide details · {}", self.server_name);
        if queue_len > 1 {
            title.push_str(&format!(" · {queue_len} requests waiting"));
        }
        let tab_labels = self
            .model
            .fields()
            .iter()
            .map(|field| field.title.clone())
            .collect::<Vec<_>>();
        let tabs = if tab_labels.len() > 1 {
            tab_labels.iter().map(String::as_str).collect::<Vec<_>>()
        } else {
            Default::default()
        };
        self.window.set_active_tab(self.model.active_index());
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
        self.render_content(buffer, layout.content);
        if responding && !layout.footer.is_empty() {
            buffer.set_line(
                layout.footer.x,
                layout.footer.y,
                &Line::from("Sending response…").dim(),
                layout.footer.width,
            );
        }
    }

    fn render_content(&mut self, buffer: &mut Buffer, area: Rect) {
        if area.is_empty() {
            return;
        }
        let Some(field) = self.model.active_field().cloned() else {
            return;
        };
        let mut y = area.y;
        let reserve = 5 + usize::from(field.description.is_some()) as u16;
        let message_rows = area.height.saturating_sub(reserve).min(4);
        let rendered_message = render_wrapped(
            buffer,
            Rect::new(area.x, y, area.width, message_rows),
            &self.message,
            Style::default().bold(),
        );
        y += rendered_message;
        if y < area.bottom() && rendered_message > 0 {
            y += 1;
        }
        if y < area.bottom() {
            let requirement = if field.required {
                "required"
            } else {
                "optional"
            };
            let progress = format!(
                "Field {}/{} · {requirement}",
                self.model.active_index() + 1,
                self.model.field_count(),
            );
            buffer.set_line(area.x, y, &Line::from(progress).dim(), area.width);
            y += 1;
        }
        if y < area.bottom() {
            let marker = if field.required { " *" } else { "" };
            buffer.set_line(
                area.x,
                y,
                &Line::from(format!("◆ {}{marker}", field.title)).bold(),
                area.width,
            );
            y += 1;
        }
        if let Some(description) = field.description.as_deref() {
            let height = area.bottom().saturating_sub(y).min(2);
            y += render_wrapped(
                buffer,
                Rect::new(area.x, y, area.width, height),
                description,
                Style::default().dim(),
            );
        }
        if y < area.bottom() {
            buffer.set_line(
                area.x,
                y,
                &Line::from(control_hint(&field.control)).dim(),
                area.width,
            );
            y += 1;
        }
        let error_rows = u16::from(self.model.error().is_some() && y < area.bottom());
        let control_height = area.bottom().saturating_sub(y).saturating_sub(error_rows);
        let control = Rect::new(area.x, y, area.width, control_height);
        match &field.control {
            McpFormControl::Text { draft, cursor, .. } => {
                self.render_text(buffer, control, draft, *cursor);
            }
            McpFormControl::Select { .. } => self.render_select(buffer, control, &field),
        }
        if let Some(error) = self.model.error()
            && error_rows > 0
        {
            let line = Line::from(format!("! {error}")).red().bold();
            buffer.set_line(area.x, area.bottom() - 1, &line, area.width);
        }
    }

    fn render_text(&mut self, buffer: &mut Buffer, area: Rect, draft: &str, cursor: usize) {
        if area.is_empty() {
            return;
        }
        let available = usize::from(area.width.saturating_sub(3));
        let (before, after) = editor_slices(draft, cursor, available);
        let mut spans = vec!["› ".cyan().bold(), before.into(), "▏".cyan().bold()];
        spans.push(if draft.is_empty() {
            "Type a value".dim()
        } else {
            after.into()
        });
        buffer.set_line(area.x, area.y, &Line::from(spans), area.width);
    }

    fn render_select(&mut self, buffer: &mut Buffer, area: Rect, field: &McpFormField) {
        let McpFormControl::Select {
            options,
            cursor,
            selected,
            multiple,
            ..
        } = &field.control
        else {
            return;
        };
        if area.is_empty() {
            return;
        }
        if options.is_empty() {
            buffer.set_line(area.x, area.y, &Line::from("No options").dim(), area.width);
            return;
        }
        let visible = usize::from(area.height).min(options.len());
        let start = cursor
            .saturating_add(1)
            .saturating_sub(visible)
            .min(options.len().saturating_sub(visible));
        for (offset, (index, option)) in options
            .iter()
            .enumerate()
            .skip(start)
            .take(visible)
            .enumerate()
        {
            let focused = index == *cursor;
            let marker = match (*multiple, selected.contains(&index)) {
                (true, true) => "[✓]",
                (true, false) => "[ ]",
                (false, true) => "(●)",
                (false, false) => "(○)",
            };
            let shortcut = if index < 9 {
                (index + 1).to_string()
            } else {
                "·".to_string()
            };
            let style = if focused {
                Style::default().cyan().bold()
            } else {
                Default::default()
            };
            let prefix = if focused { "›" } else { " " };
            let line =
                Line::from(format!("{prefix} {shortcut} {marker} {}", option.label)).style(style);
            buffer.set_line(area.x, area.y + offset as u16, &line, area.width);
        }
    }

    fn shortcuts(&self) -> Vec<ModalShortcut<'static>> {
        let mut shortcuts = match self.model.active_field().map(|field| &field.control) {
            Some(McpFormControl::Select { .. }) => vec![
                ModalShortcut::hint("↑/↓ navigate"),
                ModalShortcut::hint("Space select"),
                ModalShortcut::hint("←/→ field"),
                ModalShortcut::hint("Enter next/submit"),
                ModalShortcut::hint("Esc cancel"),
            ],
            Some(McpFormControl::Text { .. }) => vec![
                ModalShortcut::hint("Ctrl+P/N field"),
                ModalShortcut::hint("Enter next/submit"),
                ModalShortcut::hint("Shift+Enter newline"),
                ModalShortcut::hint("Esc cancel"),
            ],
            None => Vec::new(),
        };
        shortcuts.push(ModalShortcut::action(DECLINE_ACTION, "Ctrl+D decline"));
        shortcuts
    }
}

fn render_wrapped(buffer: &mut Buffer, area: Rect, text: &str, style: Style) -> u16 {
    if area.is_empty() || text.trim().is_empty() {
        return 0;
    }
    let lines = textwrap::wrap(text, usize::from(area.width.max(1)));
    let visible = lines.len().min(usize::from(area.height));
    for (offset, line) in lines.into_iter().take(visible).enumerate() {
        let line = Line::from(line.into_owned()).style(style);
        buffer.set_line(area.x, area.y + offset as u16, &line, area.width);
    }
    visible as u16
}

fn control_hint(control: &McpFormControl) -> String {
    match control {
        McpFormControl::Text {
            kind:
                McpFormTextKind::String {
                    min_length,
                    max_length,
                    format,
                },
            ..
        } => join_hints(
            format.map_or("Text", string_format_label),
            min_length.map(|value| format!("min {value} chars")),
            max_length.map(|value| format!("max {value} chars")),
        ),
        McpFormControl::Text {
            kind:
                McpFormTextKind::Number {
                    integer,
                    minimum,
                    maximum,
                },
            ..
        } => join_hints(
            if *integer { "Integer" } else { "Number" },
            minimum.map(|value| format!("min {value}")),
            maximum.map(|value| format!("max {value}")),
        ),
        McpFormControl::Select {
            multiple,
            min_selected,
            max_selected,
            ..
        } => join_hints(
            if *multiple {
                "Choose any"
            } else {
                "Choose one"
            },
            min_selected.map(|value| format!("min {value}")),
            max_selected.map(|value| format!("max {value}")),
        ),
    }
}

fn join_hints(base: &str, minimum: Option<String>, maximum: Option<String>) -> String {
    [Some(base.to_string()), minimum, maximum]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
}

fn string_format_label(format: McpElicitationStringFormat) -> &'static str {
    match format {
        McpElicitationStringFormat::Email => "Email",
        McpElicitationStringFormat::Uri => "URL",
        McpElicitationStringFormat::Date => "Date",
        McpElicitationStringFormat::DateTime => "Date and time",
    }
}

fn editor_slices(draft: &str, cursor: usize, width: usize) -> (String, String) {
    let before = sanitize_text(&draft[..cursor]);
    let after = sanitize_text(&draft[cursor..]);
    let after_budget = width.saturating_sub(1) / 2;
    let after_width = Line::from(after.as_str()).width().min(after_budget);
    let before_budget = width.saturating_sub(1).saturating_sub(after_width);
    let visible_before = tail_within_width(&before, before_budget);
    let remaining = width
        .saturating_sub(1)
        .saturating_sub(Line::from(visible_before.as_str()).width());
    (visible_before, head_within_width(&after, remaining))
}

fn sanitize_text(text: &str) -> String {
    text.replace('\n', "↵").replace('\t', "⇥")
}

fn head_within_width(text: &str, width: usize) -> String {
    text.chars()
        .scan(0, |used, character| {
            *used += Line::from(character.to_string()).width();
            (*used <= width).then_some(character)
        })
        .collect()
}

fn tail_within_width(text: &str, width: usize) -> String {
    text.chars()
        .rev()
        .scan(0, |used, character| {
            *used += Line::from(character.to_string()).width();
            (*used <= width).then_some(character)
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}
