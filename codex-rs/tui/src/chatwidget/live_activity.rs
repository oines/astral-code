//! Low-noise live activity surface for in-flight background work.

use super::*;

const MAX_PREVIEW_LINES: usize = 3;

#[derive(Clone, Debug)]
pub(super) struct BackgroundTaskActivity {
    call_id: String,
    task_id: String,
    command_display: String,
    status: BackgroundTaskStatus,
    output_lines: Vec<String>,
    omitted_output_lines: usize,
    duration_ms: Option<i64>,
    started_at: Instant,
    summary_committed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BackgroundTaskStatus {
    Running,
    Completed,
    Failed,
    Stopped,
}

impl BackgroundTaskStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }

    fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Stopped)
    }
}

#[derive(Default)]
pub(super) struct LiveActivityStore {
    background_tasks: Vec<BackgroundTaskActivity>,
}

impl LiveActivityStore {
    pub(super) fn is_empty(&self) -> bool {
        self.background_tasks.is_empty()
    }

    pub(super) fn has_background_task(&self, task_id: &str) -> bool {
        self.background_tasks
            .iter()
            .any(|task| task.task_id == task_id)
    }

    pub(super) fn has_any_background_task(&self) -> bool {
        !self.background_tasks.is_empty()
    }

    pub(super) fn start_background_task(
        &mut self,
        call_id: &str,
        process_id: &str,
        command_display: String,
    ) {
        if let Some(task) = self
            .background_tasks
            .iter_mut()
            .find(|task| task.task_id == process_id)
        {
            task.call_id = call_id.to_string();
            task.command_display = command_display;
            task.status = BackgroundTaskStatus::Running;
            task.duration_ms = None;
            task.summary_committed = false;
            return;
        }

        self.background_tasks.push(BackgroundTaskActivity {
            call_id: call_id.to_string(),
            task_id: process_id.to_string(),
            command_display,
            status: BackgroundTaskStatus::Running,
            output_lines: Vec::new(),
            omitted_output_lines: 0,
            duration_ms: None,
            started_at: Instant::now(),
            summary_committed: false,
        });
    }

    pub(super) fn append_output_for_call(&mut self, call_id: &str, output: &str) -> bool {
        let Some(task) = self
            .background_tasks
            .iter_mut()
            .find(|task| task.call_id == call_id)
        else {
            return false;
        };
        task.push_output(output);
        true
    }

    pub(super) fn update_for_terminal_interaction(
        &mut self,
        process_id: &str,
        stdin: &str,
    ) -> bool {
        let Some(task) = self
            .background_tasks
            .iter_mut()
            .find(|task| task.task_id == process_id)
        else {
            return false;
        };
        if !stdin.trim().is_empty() {
            task.push_output(stdin);
        }
        true
    }

    pub(super) fn complete_background_task(
        &mut self,
        call_id: &str,
        process_id: Option<&str>,
        output: &str,
        exit_code: Option<i32>,
        duration_ms: Option<i64>,
    ) -> bool {
        let Some(task) = self.background_task_mut(call_id, process_id) else {
            return false;
        };
        task.push_output(output);
        task.status = match exit_code {
            Some(0) => BackgroundTaskStatus::Completed,
            Some(_) => BackgroundTaskStatus::Failed,
            None => BackgroundTaskStatus::Completed,
        };
        task.duration_ms = duration_ms;
        task.summary_committed = false;
        true
    }

    pub(super) fn attach_core_tool_call(&mut self, item: &ThreadItem) -> bool {
        let ThreadItem::CoreToolCall {
            tool,
            arguments,
            result,
            error,
            status,
            ..
        } = item
        else {
            return false;
        };

        match tool.as_str() {
            "ListBackgroundTasks" => {
                let mut matched = false;
                if let Some(result) = result.as_deref() {
                    matched |= self.update_from_list_result(result);
                }
                if matched {
                    return true;
                }
                error.is_some() && self.has_any_background_task()
            }
            "ReadTaskOutput" => {
                let Some(task_id) = task_id_arg(arguments) else {
                    return false;
                };
                let Some(task) = self
                    .background_tasks
                    .iter_mut()
                    .find(|task| task.task_id == task_id)
                else {
                    return false;
                };
                if let Some(result) = result.as_deref() {
                    task.push_output(result);
                }
                if error.is_some()
                    || matches!(
                        status,
                        codex_app_server_protocol::CoreToolCallStatus::Failed
                    )
                {
                    task.status = BackgroundTaskStatus::Failed;
                    task.summary_committed = false;
                }
                true
            }
            "SendTaskInput" => {
                let Some(task_id) = task_id_arg(arguments) else {
                    return false;
                };
                let Some(task) = self
                    .background_tasks
                    .iter_mut()
                    .find(|task| task.task_id == task_id)
                else {
                    return false;
                };
                if let Some(input) = arguments.get("input").and_then(serde_json::Value::as_str) {
                    task.push_output(input);
                }
                true
            }
            "StopBackgroundTask" => {
                let Some(task_id) = task_id_arg(arguments) else {
                    return false;
                };
                let Some(task) = self
                    .background_tasks
                    .iter_mut()
                    .find(|task| task.task_id == task_id)
                else {
                    return false;
                };
                task.status = BackgroundTaskStatus::Stopped;
                task.summary_committed = false;
                true
            }
            _ => false,
        }
    }

    pub(super) fn to_cell(&self) -> LiveActivitiesCell {
        LiveActivitiesCell {
            background_tasks: self.background_tasks.clone(),
        }
    }

    pub(super) fn drain_summary_cells(&mut self) -> Vec<Box<dyn HistoryCell>> {
        let mut summaries = Vec::new();
        self.background_tasks.retain_mut(|task| {
            if task.status.is_terminal() || !task.summary_committed {
                summaries.push(Box::new(LiveActivitiesCell {
                    background_tasks: vec![task.clone()],
                }) as Box<dyn HistoryCell>);
                task.summary_committed = true;
            }
            !task.status.is_terminal()
        });
        summaries
    }

    fn background_task_mut(
        &mut self,
        call_id: &str,
        process_id: Option<&str>,
    ) -> Option<&mut BackgroundTaskActivity> {
        self.background_tasks.iter_mut().find(|task| {
            task.call_id == call_id
                || process_id.is_some_and(|process_id| task.task_id == process_id)
        })
    }

    fn update_from_list_result(&mut self, result: &str) -> bool {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(result) else {
            return false;
        };
        let Some(tasks) = value.get("tasks").and_then(serde_json::Value::as_array) else {
            return false;
        };

        let mut matched = false;
        for task_value in tasks {
            let Some(task_id) = task_value
                .get("task_id")
                .or_else(|| task_value.get("taskId"))
                .or_else(|| task_value.get("process_id"))
                .or_else(|| task_value.get("processId"))
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let Some(task) = self
                .background_tasks
                .iter_mut()
                .find(|task| task.task_id == task_id)
            else {
                continue;
            };
            matched = true;
            if let Some(command) = task_value
                .get("command")
                .or_else(|| task_value.get("cmd"))
                .and_then(serde_json::Value::as_str)
                && task.command_display.is_empty()
            {
                task.command_display = command.to_string();
            }
            if let Some(status) = task_value
                .get("status")
                .and_then(serde_json::Value::as_str)
                .and_then(parse_background_task_status)
            {
                if status.is_terminal() && task.status != status {
                    task.summary_committed = false;
                }
                task.status = status;
            }
        }
        matched
    }
}

impl BackgroundTaskActivity {
    fn push_output(&mut self, output: &str) {
        for line in output
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.is_empty())
        {
            self.output_lines.push(line.to_string());
        }
        if self.output_lines.len() > MAX_PREVIEW_LINES {
            let drop_count = self.output_lines.len() - MAX_PREVIEW_LINES;
            self.output_lines.drain(0..drop_count);
            self.omitted_output_lines += drop_count;
        }
    }

    fn elapsed_seconds(&self) -> u64 {
        if let Some(duration_ms) = self
            .duration_ms
            .and_then(|duration_ms| u64::try_from(duration_ms).ok())
        {
            duration_ms / 1_000
        } else {
            self.started_at.elapsed().as_secs()
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct LiveActivitiesCell {
    background_tasks: Vec<BackgroundTaskActivity>,
}

impl HistoryCell for LiveActivitiesCell {
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        for (index, task) in self.background_tasks.iter().enumerate() {
            if index > 0 {
                lines.push(Line::from(""));
            }
            lines.push(task_header_line(task));
            if !task.command_display.is_empty() {
                lines.push(Line::from(vec![
                    "  └ ".dim(),
                    task.command_display.clone().dim(),
                ]));
            }
            for output in &task.output_lines {
                lines.push(Line::from(vec!["    ".into(), output.clone().dim()]));
            }
            if task.omitted_output_lines > 0 {
                lines.push(Line::from(vec![
                    "    ".into(),
                    format!(
                        "... +{} lines (ctrl+t to view transcript)",
                        task.omitted_output_lines
                    )
                    .dim(),
                ]));
            }
        }
        lines
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        history_cell::plain_lines(self.display_lines(u16::MAX))
    }

    fn transcript_animation_tick(&self) -> Option<u64> {
        self.background_tasks
            .iter()
            .filter(|task| !task.status.is_terminal())
            .map(BackgroundTaskActivity::elapsed_seconds)
            .max()
    }
}

fn task_header_line(task: &BackgroundTaskActivity) -> Line<'static> {
    let marker = match task.status {
        BackgroundTaskStatus::Running => "•".dim(),
        BackgroundTaskStatus::Completed => "✓".green().bold(),
        BackgroundTaskStatus::Failed => "•".red().bold(),
        BackgroundTaskStatus::Stopped => "•".dim(),
    };
    let elapsed = task.elapsed_seconds();
    let elapsed_text = if task.status.is_terminal() {
        format!(" in {elapsed}s")
    } else {
        format!(" · {elapsed}s")
    };
    Line::from(vec![
        marker,
        " ".into(),
        format!("Bash task {} ", task.task_id).bold(),
        task.status.label().bold(),
        elapsed_text.dim(),
    ])
}

fn task_id_arg(arguments: &serde_json::Value) -> Option<&str> {
    arguments
        .get("task_id")
        .or_else(|| arguments.get("taskId"))
        .and_then(serde_json::Value::as_str)
}

fn parse_background_task_status(status: &str) -> Option<BackgroundTaskStatus> {
    match status.to_ascii_lowercase().as_str() {
        "running" => Some(BackgroundTaskStatus::Running),
        "completed" | "complete" | "done" => Some(BackgroundTaskStatus::Completed),
        "failed" | "error" => Some(BackgroundTaskStatus::Failed),
        "stopped" | "cancelled" | "canceled" => Some(BackgroundTaskStatus::Stopped),
        _ => None,
    }
}

impl ChatWidget {
    pub(super) fn sync_live_activity_cell(&mut self) {
        if self.live_activities.is_empty() {
            if self
                .transcript
                .active_cell
                .as_ref()
                .is_some_and(|cell| cell.as_any().is::<LiveActivitiesCell>())
            {
                self.transcript.active_cell = None;
                self.bump_active_cell_revision();
                self.request_redraw();
            }
            return;
        }

        if self
            .transcript
            .active_cell
            .as_ref()
            .is_some_and(|cell| !cell.as_any().is::<LiveActivitiesCell>())
        {
            self.flush_active_cell();
        }
        self.transcript.active_cell = Some(Box::new(self.live_activities.to_cell()));
        self.bump_active_cell_revision();
        self.request_redraw();
    }

    pub(super) fn commit_live_activity_summaries(&mut self) {
        if self
            .transcript
            .active_cell
            .as_ref()
            .is_some_and(|cell| cell.as_any().is::<LiveActivitiesCell>())
        {
            self.transcript.active_cell = None;
            self.bump_active_cell_revision();
        }

        let summaries = self.live_activities.drain_summary_cells();
        if summaries.is_empty() {
            return;
        }
        self.transcript.needs_final_message_separator = true;
        self.transcript.had_work_activity = true;
        for summary in summaries {
            self.app_event_tx.send(AppEvent::InsertHistoryCell(summary));
        }
        self.request_redraw();
    }
}
