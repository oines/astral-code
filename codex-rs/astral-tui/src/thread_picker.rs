use std::collections::HashSet;
use std::io;
use std::io::Stdout;

use codex_app_server_client::AppServerClient;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadListResponse;
use crossterm::event::Event;
use crossterm::event::EventStream;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use tokio_stream::StreamExt;

use crate::terminal_guard::TerminalGuard;
use crate::view::AstralTheme;
use crate::view::ModalHeight;
use crate::view::render_modal_frame;

type PickerTerminal = Terminal<CrosstermBackend<Stdout>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadPickerAction {
    Resume,
    Fork,
}

impl ThreadPickerAction {
    pub(crate) fn verb(self) -> &'static str {
        match self {
            Self::Resume => "Resume",
            Self::Fork => "Fork",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThreadPickerOptions {
    pub action: ThreadPickerAction,
    pub list_params: ThreadListParams,
}

impl ThreadPickerOptions {
    pub fn new(action: ThreadPickerAction, list_params: ThreadListParams) -> Self {
        Self {
            action,
            list_params,
        }
    }
}

#[derive(Debug)]
pub(crate) struct PickerState {
    action: ThreadPickerAction,
    threads: Vec<Thread>,
    known_ids: HashSet<String>,
    next_cursor: Option<String>,
    query: String,
    selected: usize,
    notice: Option<String>,
}

impl PickerState {
    pub(crate) fn new(action: ThreadPickerAction, page: ThreadListResponse) -> Self {
        let known_ids = page.data.iter().map(|thread| thread.id.clone()).collect();
        Self {
            action,
            threads: page.data,
            known_ids,
            next_cursor: page.next_cursor,
            query: String::new(),
            selected: 0,
            notice: None,
        }
    }

    pub(crate) fn append(&mut self, page: ThreadListResponse) {
        self.threads.extend(
            page.data
                .into_iter()
                .filter(|thread| self.known_ids.insert(thread.id.clone())),
        );
        self.next_cursor = page.next_cursor;
        self.clamp_selection();
    }

    pub(crate) fn action(&self) -> ThreadPickerAction {
        self.action
    }

    pub(crate) fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }

    fn filtered_indices(&self) -> Vec<usize> {
        let query = self.query.to_lowercase();
        self.threads
            .iter()
            .enumerate()
            .filter_map(|(index, thread)| {
                let matches = query.is_empty()
                    || thread_title(thread).to_lowercase().contains(&query)
                    || thread.preview.to_lowercase().contains(&query)
                    || thread.cwd.to_string_lossy().to_lowercase().contains(&query);
                matches.then_some(index)
            })
            .collect()
    }

    fn selected_thread(&self) -> Option<&Thread> {
        let indices = self.filtered_indices();
        indices
            .get(self.selected)
            .and_then(|index| self.threads.get(*index))
    }

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub(crate) fn move_down(&mut self) {
        let last = self.filtered_indices().len().saturating_sub(1);
        self.selected = self.selected.saturating_add(1).min(last);
    }

    fn page_up(&mut self, rows: usize) {
        self.selected = self.selected.saturating_sub(rows.max(1));
    }

    fn page_down(&mut self, rows: usize) {
        let last = self.filtered_indices().len().saturating_sub(1);
        self.selected = self.selected.saturating_add(rows.max(1)).min(last);
    }

    fn clamp_selection(&mut self) {
        self.selected = self
            .selected
            .min(self.filtered_indices().len().saturating_sub(1));
    }

    fn at_end(&self) -> bool {
        self.selected + 1 >= self.filtered_indices().len()
    }

    pub(crate) fn paste(&mut self, text: &str) {
        self.query.push_str(text);
        self.clamp_selection();
    }

    pub(crate) fn set_notice(&mut self, notice: impl Into<String>) {
        self.notice = Some(notice.into());
    }
}

pub(crate) enum PickerInput {
    None,
    Redraw,
    LoadNext,
    Select(Box<Thread>),
    Cancel,
}

/// Runs Astral's app-server-backed session picker before activating a thread.
///
/// The picker owns presentation and keyboard state while the caller owns
/// configuration-derived list filters and the eventual resume/fork request.
pub async fn run_thread_picker(
    client: &AppServerClient,
    mut options: ThreadPickerOptions,
) -> io::Result<Option<Thread>> {
    options.list_params.limit = Some(100);
    let page = load_page(client, &options.list_params, /*request_id*/ 0).await?;
    let mut state = PickerState::new(options.action, page);
    let mut guard = TerminalGuard::enter_alternate()?;
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = PickerTerminal::new(backend)?;
    terminal.hide_cursor()?;
    let mut input = EventStream::new();
    let mut request_id = 1;

    let result = loop {
        terminal.draw(|frame| draw_picker(frame, &state))?;
        let Some(event) = input.next().await else {
            break Ok(None);
        };
        let action = match event? {
            Event::Key(key) => handle_key(&mut state, key, terminal.size()?.height),
            Event::Paste(text) => {
                state.paste(&text);
                PickerInput::Redraw
            }
            Event::Resize(_, _) | Event::FocusGained | Event::FocusLost | Event::Mouse(_) => {
                PickerInput::Redraw
            }
        };
        match action {
            PickerInput::None | PickerInput::Redraw => {}
            PickerInput::Select(thread) => break Ok(Some(*thread)),
            PickerInput::Cancel => break Ok(None),
            PickerInput::LoadNext => {
                let Some(cursor) = state.next_cursor.clone() else {
                    continue;
                };
                options.list_params.cursor = Some(cursor);
                match load_page(client, &options.list_params, request_id).await {
                    Ok(page) => {
                        state.append(page);
                        state.move_down();
                        state.notice = None;
                    }
                    Err(error) => state.notice = Some(error.to_string()),
                }
                request_id += 1;
            }
        }
    };

    let _ = terminal.show_cursor();
    drop(terminal);
    guard.restore();
    result
}

async fn load_page(
    client: &AppServerClient,
    params: &ThreadListParams,
    request_id: i64,
) -> io::Result<ThreadListResponse> {
    client
        .request_typed(ClientRequest::ThreadList {
            request_id: RequestId::Integer(request_id),
            params: params.clone(),
        })
        .await
        .map_err(io::Error::other)
}

pub(crate) fn handle_key(
    state: &mut PickerState,
    key: KeyEvent,
    terminal_height: u16,
) -> PickerInput {
    if key.kind == KeyEventKind::Release {
        return PickerInput::None;
    }
    let page_rows = usize::from(terminal_height.saturating_sub(6) / 2).max(1);
    match (key.code, key.modifiers) {
        (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => PickerInput::Cancel,
        (KeyCode::Enter, _) => state
            .selected_thread()
            .cloned()
            .map(Box::new)
            .map(PickerInput::Select)
            .unwrap_or(PickerInput::None),
        (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
            state.move_up();
            PickerInput::Redraw
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
            if state.at_end() && state.next_cursor.is_some() {
                PickerInput::LoadNext
            } else {
                state.move_down();
                PickerInput::Redraw
            }
        }
        (KeyCode::PageUp, _) => {
            state.page_up(page_rows);
            PickerInput::Redraw
        }
        (KeyCode::PageDown, _) => {
            state.page_down(page_rows);
            PickerInput::Redraw
        }
        (KeyCode::Backspace, _) => {
            state.query.pop();
            state.clamp_selection();
            PickerInput::Redraw
        }
        (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
            state.query.clear();
            state.clamp_selection();
            PickerInput::Redraw
        }
        (KeyCode::Char(character), modifiers)
            if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) =>
        {
            state.query.push(character);
            state.clamp_selection();
            PickerInput::Redraw
        }
        _ => PickerInput::None,
    }
}

fn draw_picker(frame: &mut Frame<'_>, state: &PickerState) {
    render_picker(
        state,
        frame.area(),
        frame.buffer_mut(),
        AstralTheme::default(),
    );
}

pub(crate) fn render_picker(
    state: &PickerState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    let title = format!("{} Astral session", state.action.verb());
    let Some(content) = render_modal_frame(
        area,
        buffer,
        theme,
        &title,
        "↑/↓ navigate · Enter select · Esc cancel",
        ModalHeight::MinimumContent(7),
    ) else {
        return;
    };

    buffer.set_line(
        content.x,
        content.y,
        &vec!["Search: ".dim(), state.query.clone().cyan()].into(),
        content.width,
    );
    let list = Rect::new(
        content.x,
        content.y.saturating_add(2),
        content.width,
        content.height.saturating_sub(3),
    );
    let rows = usize::from(list.height / 2).max(1);
    let indices = state.filtered_indices();
    let start = state.selected.saturating_sub(rows.saturating_sub(1));
    let mut lines = Vec::new();
    for (visible_index, thread_index) in indices.iter().enumerate().skip(start).take(rows) {
        let thread = &state.threads[*thread_index];
        let selected = visible_index == state.selected;
        let marker = if selected { "❯ ".cyan() } else { "  ".into() };
        let title = if selected {
            thread_title(thread).bold()
        } else {
            thread_title(thread).into()
        };
        lines.push(vec![marker, title].into());
        lines.push(
            vec![
                "    ".into(),
                thread.cwd.to_string_lossy().to_string().dim(),
                " · ".dim(),
                format!("updated {}", thread.updated_at).dim(),
            ]
            .into(),
        );
    }
    if lines.is_empty() {
        lines.push("  No matching Astral sessions".dim().into());
    }
    Paragraph::new(lines).render(list, buffer);

    let mut footer_line = vec![
        format!(
            "{}/{}",
            state.selected.saturating_add(1).min(indices.len()),
            indices.len()
        )
        .dim(),
    ];
    if state.next_cursor.is_some() {
        footer_line.push(" · more available".cyan());
    }
    if let Some(notice) = &state.notice {
        footer_line.push(" · ".dim());
        footer_line.push(notice.clone().red());
    }
    buffer.set_line(
        content.x,
        content.bottom().saturating_sub(1),
        &footer_line.into(),
        content.width,
    );
}

fn thread_title(thread: &Thread) -> String {
    thread
        .name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .or_else(|| {
            let preview = thread.preview.trim();
            (!preview.is_empty()).then(|| preview.to_string())
        })
        .unwrap_or_else(|| thread.id.clone())
}

#[cfg(test)]
#[path = "thread_picker_tests.rs"]
mod tests;
