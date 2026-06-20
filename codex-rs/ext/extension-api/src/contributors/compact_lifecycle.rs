use crate::ExtensionData;
use codex_protocol::ThreadId;

/// Input passed to extensions immediately before compacting a thread.
pub struct CompactStartInput<'a> {
    pub thread_id: ThreadId,
    pub turn_id: &'a str,
    pub trigger: &'a str,
    pub session_store: &'a ExtensionData,
    pub thread_store: &'a ExtensionData,
}
