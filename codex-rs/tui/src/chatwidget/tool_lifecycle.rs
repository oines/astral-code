//! Non-command tool lifecycle rendering for `ChatWidget`.
//!
//! This module handles patch, MCP, web search, image, and collaborator tool
//! events as transcript cells.

use super::*;

impl ChatWidget {
    pub(super) fn on_patch_apply_begin_with_id(
        &mut self,
        id: Option<String>,
        changes: HashMap<PathBuf, FileChange>,
    ) {
        if let Some(id) = id
            && self.pending_file_tool_call_ids.contains(&id)
        {
            self.file_tool_change_ids.insert(id);
        }
        self.record_visible_turn_activity();
        self.add_to_history(history_cell::new_patch_event(changes, &self.config.cwd));
    }

    pub(super) fn on_view_image_tool_call(&mut self, path: AbsolutePathBuf) {
        self.record_visible_turn_activity();
        self.flush_answer_stream_with_separator();
        self.add_to_history(history_cell::new_view_image_tool_call(
            path,
            &self.config.cwd,
        ));
        self.request_redraw();
    }

    pub(super) fn on_image_generation_begin(&mut self) {
        self.record_visible_turn_activity();
        self.flush_answer_stream_with_separator();
    }

    pub(super) fn on_image_generation_end(
        &mut self,
        call_id: String,
        revised_prompt: Option<String>,
        saved_path: Option<AbsolutePathBuf>,
    ) {
        self.flush_answer_stream_with_separator();
        self.add_to_history(history_cell::new_image_generation_call(
            call_id,
            revised_prompt,
            saved_path,
        ));
        self.request_redraw();
    }

    pub(super) fn on_file_change_completed(&mut self, item: ThreadItem) {
        let item2 = item.clone();
        self.defer_or_handle(
            |q| q.push_item_completed(item),
            |s| s.handle_file_change_completed_now(item2),
        );
    }

    pub(super) fn on_mcp_tool_call_started(&mut self, item: ThreadItem) {
        let item2 = item.clone();
        self.defer_or_handle(
            |q| q.push_item_started(item),
            |s| s.handle_mcp_tool_call_started_now(item2),
        );
    }

    pub(super) fn on_mcp_tool_call_completed(&mut self, item: ThreadItem) {
        let item2 = item.clone();
        self.defer_or_handle(
            |q| q.push_item_completed(item),
            |s| s.handle_mcp_tool_call_completed_now(item2),
        );
    }

    pub(super) fn on_core_tool_call_started(&mut self, item: ThreadItem) {
        let item2 = item.clone();
        self.defer_or_handle(
            |q| q.push_item_started(item),
            |s| s.handle_core_tool_call_started_now(item2),
        );
    }

    pub(super) fn on_core_tool_call_completed(&mut self, item: ThreadItem) {
        let item2 = item.clone();
        self.defer_or_handle(
            |q| q.push_item_completed(item),
            |s| s.handle_core_tool_call_completed_now(item2),
        );
    }

    pub(super) fn on_web_search_begin(&mut self, call_id: String) {
        self.record_visible_turn_activity();
        self.flush_answer_stream_with_separator();
        self.flush_active_cell();
        self.transcript.active_cell = Some(Box::new(history_cell::new_active_web_search_call(
            call_id,
            String::new(),
            self.config.animations,
        )));
        self.bump_active_cell_revision();
        self.request_redraw();
    }

    pub(super) fn on_web_search_end(
        &mut self,
        call_id: String,
        query: String,
        action: codex_app_server_protocol::WebSearchAction,
    ) {
        self.flush_answer_stream_with_separator();
        let mut handled = false;
        if let Some(cell) = self
            .transcript
            .active_cell
            .as_mut()
            .and_then(|cell| cell.as_any_mut().downcast_mut::<WebSearchCell>())
            && cell.call_id() == call_id
        {
            cell.update(action.clone(), query.clone());
            cell.complete();
            self.bump_active_cell_revision();
            self.flush_active_cell();
            handled = true;
        }

        if !handled {
            self.add_to_history(history_cell::new_web_search_call(call_id, query, action));
        }
        self.transcript.had_work_activity = true;
    }

    pub(super) fn on_collab_event(&mut self, cell: PlainHistoryCell) {
        self.flush_answer_stream_with_separator();
        self.add_to_history(cell);
        self.request_redraw();
    }

    pub(super) fn on_collab_agent_tool_call(&mut self, item: ThreadItem) {
        self.record_visible_turn_activity();
        let ThreadItem::CollabAgentToolCall {
            id, tool, status, ..
        } = &item
        else {
            return;
        };
        if matches!(tool, CollabAgentTool::SpawnAgent)
            && let Some(spawn_request) = multi_agents::spawn_request_summary(&item)
        {
            self.pending_collab_spawn_requests
                .insert(id.clone(), spawn_request);
        }

        let cached_spawn_request = if matches!(tool, CollabAgentTool::SpawnAgent)
            && !matches!(status, CollabAgentToolCallStatus::InProgress)
        {
            self.pending_collab_spawn_requests.remove(id)
        } else {
            None
        };

        if let Some(cell) = multi_agents::tool_call_history_cell(
            &item,
            cached_spawn_request.as_ref(),
            |thread_id| self.collab_agent_metadata(thread_id),
        ) {
            self.on_collab_event(cell);
        }
    }

    pub(crate) fn handle_file_change_completed_now(&mut self, item: ThreadItem) {
        let ThreadItem::FileChange { status, .. } = item else {
            return;
        };
        // If the patch was successful, just let the "Edited" block stand.
        // Otherwise, add a failure block.
        if matches!(status, codex_app_server_protocol::PatchApplyStatus::Failed) {
            self.add_to_history(history_cell::new_patch_apply_failure(String::new()));
        }
        // Mark that actual work was done (patch applied)
        self.transcript.had_work_activity = true;
    }

    pub(crate) fn handle_mcp_tool_call_started_now(&mut self, item: ThreadItem) {
        self.record_visible_turn_activity();
        let ThreadItem::McpToolCall {
            id,
            server,
            tool,
            arguments,
            ..
        } = item
        else {
            return;
        };
        self.flush_answer_stream_with_separator();
        self.flush_active_cell();
        self.transcript.active_cell = Some(Box::new(history_cell::new_active_mcp_tool_call(
            id,
            McpInvocation {
                server,
                tool,
                arguments: Some(arguments),
            },
            self.config.animations,
        )));
        self.bump_active_cell_revision();
        self.request_redraw();
    }

    pub(crate) fn handle_mcp_tool_call_completed_now(&mut self, item: ThreadItem) {
        self.flush_answer_stream_with_separator();

        let ThreadItem::McpToolCall {
            id,
            server,
            tool,
            arguments,
            result,
            error,
            duration_ms,
            ..
        } = item
        else {
            return;
        };
        let invocation = McpInvocation {
            server,
            tool,
            arguments: Some(arguments),
        };
        let duration = Duration::from_millis(duration_ms.unwrap_or_default().max(0) as u64);
        let result = match (result, error) {
            (_, Some(error)) => Err(error.message),
            (Some(result), None) => {
                let result = *result;
                Ok(codex_protocol::mcp::CallToolResult {
                    content: result.content,
                    structured_content: result.structured_content,
                    is_error: Some(false),
                    meta: None,
                })
            }
            (None, None) => Err("MCP tool call completed without a result".to_string()),
        };

        let extra_cell = match self
            .transcript
            .active_cell
            .as_mut()
            .and_then(|cell| cell.as_any_mut().downcast_mut::<McpToolCallCell>())
        {
            Some(cell) if cell.call_id() == id => cell.complete(duration, result),
            _ => {
                self.flush_active_cell();
                let mut cell =
                    history_cell::new_active_mcp_tool_call(id, invocation, self.config.animations);
                let extra_cell = cell.complete(duration, result);
                self.transcript.active_cell = Some(Box::new(cell));
                extra_cell
            }
        };

        self.flush_active_cell();
        if let Some(extra) = extra_cell {
            self.add_boxed_history(extra);
        }
        // Mark that actual work was done (MCP tool call)
        self.transcript.had_work_activity = true;
    }

    pub(crate) fn handle_core_tool_call_started_now(&mut self, item: ThreadItem) {
        self.record_visible_turn_activity();
        if let Some(id) = file_tool_call_id(&item) {
            self.pending_file_tool_call_ids.insert(id.to_string());
        }
        if should_suppress_core_tool_start(&item, &self.live_activities) {
            return;
        }
        let Some(cell) = history_cell::new_core_tool_call_cell(item) else {
            return;
        };
        self.flush_answer_stream_with_separator();
        self.flush_active_cell();
        self.transcript.active_cell = Some(Box::new(cell));
        self.bump_active_cell_revision();
        self.request_redraw();
    }

    pub(crate) fn handle_core_tool_call_completed_now(&mut self, item: ThreadItem) {
        self.flush_answer_stream_with_separator();
        if self.should_suppress_successful_file_tool(&item) {
            return;
        }
        if self.live_activities.attach_core_tool_call(&item) {
            self.transcript.had_work_activity = true;
            self.sync_live_activity_cell();
            return;
        }
        let id = item.id().to_string();
        let mut handled = false;
        if let Some(cell) = self
            .transcript
            .active_cell
            .as_mut()
            .and_then(|cell| cell.as_any_mut().downcast_mut::<CoreToolCallCell>())
            && cell.call_id() == id
        {
            cell.update_from_item(item.clone());
            self.bump_active_cell_revision();
            self.flush_active_cell();
            handled = true;
        }

        if !handled && let Some(cell) = history_cell::new_core_tool_call_cell(item) {
            self.flush_active_cell();
            self.add_to_history(cell);
        }
        self.transcript.had_work_activity = true;
    }

    pub(crate) fn handle_queued_item_started_now(&mut self, item: ThreadItem) {
        match item {
            item @ ThreadItem::CommandExecution { .. } => {
                self.handle_command_execution_started_now(item);
            }
            item @ ThreadItem::McpToolCall { .. } => {
                self.handle_mcp_tool_call_started_now(item);
            }
            item @ ThreadItem::CoreToolCall { .. } => {
                self.handle_core_tool_call_started_now(item);
            }
            _ => {}
        }
    }

    pub(crate) fn handle_queued_item_completed_now(&mut self, item: ThreadItem) {
        match item {
            item @ ThreadItem::CommandExecution { .. } => {
                self.handle_command_execution_completed_now(item);
            }
            item @ ThreadItem::FileChange { .. } => self.handle_file_change_completed_now(item),
            item @ ThreadItem::McpToolCall { .. } => self.handle_mcp_tool_call_completed_now(item),
            item @ ThreadItem::CoreToolCall { .. } => {
                self.handle_core_tool_call_completed_now(item)
            }
            _ => {}
        }
    }

    fn should_suppress_successful_file_tool(&mut self, item: &ThreadItem) -> bool {
        let Some(id) = file_tool_call_id(item) else {
            return false;
        };
        self.pending_file_tool_call_ids.remove(id);
        let ThreadItem::CoreToolCall { status, error, .. } = item else {
            return false;
        };
        if matches!(
            status,
            codex_app_server_protocol::CoreToolCallStatus::Completed
        ) && error.is_none()
        {
            return self.file_tool_change_ids.remove(id);
        }
        self.file_tool_change_ids.remove(id);
        false
    }
}

fn should_suppress_core_tool_start(item: &ThreadItem, live_activities: &LiveActivityStore) -> bool {
    let ThreadItem::CoreToolCall {
        tool, arguments, ..
    } = item
    else {
        return false;
    };
    match tool.as_str() {
        "Edit" | "Write" => true,
        "ListBackgroundTasks" => live_activities.has_any_background_task(),
        "ReadTaskOutput" | "SendTaskInput" | "StopBackgroundTask" => arguments
            .get("task_id")
            .or_else(|| arguments.get("taskId"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|task_id| live_activities.has_background_task(task_id)),
        _ => false,
    }
}

fn file_tool_call_id(item: &ThreadItem) -> Option<&str> {
    let ThreadItem::CoreToolCall { id, tool, .. } = item else {
        return None;
    };
    matches!(tool.as_str(), "Edit" | "Write").then_some(id.as_str())
}
