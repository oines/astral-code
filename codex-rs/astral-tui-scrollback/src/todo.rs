// Derived from Grok Build's todo pane status presentation at
// commit 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Modified for Astral's provider-neutral app-server plan notifications.

use codex_app_server_protocol::TurnPlanStepStatus;
use codex_app_server_protocol::TurnPlanUpdatedNotification;
use ratatui::style::Style;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use serde_json::Value;
use textwrap::Options;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoItemPresentation {
    pub text: String,
    pub status: TodoStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoPresentation {
    pub explanation: Option<String>,
    pub items: Vec<TodoItemPresentation>,
}

impl TodoPresentation {
    pub fn from_tool_arguments(arguments: &Value) -> Option<Self> {
        let steps = arguments
            .get("plan")
            .or_else(|| arguments.get("todos"))?
            .as_array()?;
        let items = steps
            .iter()
            .filter_map(|step| {
                let text = step
                    .get("step")
                    .or_else(|| step.get("content"))
                    .and_then(Value::as_str)?
                    .trim();
                if text.is_empty() {
                    return None;
                }
                let status = match step
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .replace(['_', '-'], "")
                    .as_str()
                {
                    "inprogress" => TodoStatus::InProgress,
                    "completed" => TodoStatus::Completed,
                    _ => TodoStatus::Pending,
                };
                Some(TodoItemPresentation {
                    text: text.to_string(),
                    status,
                })
            })
            .collect();
        Some(Self {
            explanation: arguments
                .get("explanation")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|explanation| !explanation.is_empty())
                .map(str::to_string),
            items,
        })
    }
}

impl From<&TurnPlanUpdatedNotification> for TodoPresentation {
    fn from(notification: &TurnPlanUpdatedNotification) -> Self {
        Self {
            explanation: notification.explanation.clone(),
            items: notification
                .plan
                .iter()
                .map(|step| TodoItemPresentation {
                    text: step.step.clone(),
                    status: match step.status {
                        TurnPlanStepStatus::Pending => TodoStatus::Pending,
                        TurnPlanStepStatus::InProgress => TodoStatus::InProgress,
                        TurnPlanStepStatus::Completed => TodoStatus::Completed,
                    },
                })
                .collect(),
        }
    }
}

pub(crate) fn render_todo(todo: &TodoPresentation, width: u16) -> Text<'static> {
    let mut lines = vec![vec!["◆ ".cyan(), "Todos".cyan().bold()].into()];
    if let Some(explanation) = todo
        .explanation
        .as_deref()
        .map(str::trim)
        .filter(|explanation| !explanation.is_empty())
    {
        lines.extend(wrap_styled(
            explanation,
            width,
            "  ",
            "  ",
            Style::default().dim().italic(),
        ));
    }
    if todo.items.is_empty() {
        lines.push("  (no items)".dim().italic().into());
    } else {
        for item in &todo.items {
            let (marker, style) = match item.status {
                TodoStatus::Pending => ("□ ", Style::default()),
                TodoStatus::InProgress => ("▶ ", Style::default().cyan().bold()),
                TodoStatus::Completed => ("✓ ", Style::default().dim().crossed_out()),
            };
            lines.extend(wrap_styled(&item.text, width, marker, "  ", style));
        }
    }
    Text::from(lines)
}

fn wrap_styled(
    text: &str,
    width: u16,
    initial_indent: &'static str,
    subsequent_indent: &'static str,
    style: Style,
) -> Vec<Line<'static>> {
    let options = Options::new(usize::from(width).max(1))
        .initial_indent(initial_indent)
        .subsequent_indent(subsequent_indent);
    textwrap::wrap(text, options)
        .into_iter()
        .map(|line| Line::from(Span::from(line.into_owned()).set_style(style)))
        .collect()
}
