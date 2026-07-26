use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use codex_app_server_protocol::DynamicToolCallParams;
use codex_app_server_protocol::DynamicToolCallResponse;

type HandlerFuture =
    Pin<Box<dyn Future<Output = Result<DynamicToolCallResponse, ClientToolError>> + Send>>;
type Handler = Arc<dyn Fn(DynamicToolCallParams) -> HandlerFuture + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientToolError {
    pub message: String,
}

impl ClientToolError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ClientToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ClientToolError {}

/// Thread-safe dynamic client tool handlers keyed by protocol namespace/name.
///
/// Claude/Codex model tools remain runtime-owned and arrive as normal
/// `ThreadItem` notifications. This registry is only for app-server tools that
/// explicitly call back into the client surface.
#[derive(Clone, Default)]
pub struct ClientToolRegistry {
    handlers: HashMap<(Option<String>, String), Handler>,
}

impl ClientToolRegistry {
    pub fn register<F, Fut>(
        &mut self,
        namespace: Option<String>,
        tool: impl Into<String>,
        handler: F,
    ) where
        F: Fn(DynamicToolCallParams) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<DynamicToolCallResponse, ClientToolError>> + Send + 'static,
    {
        self.handlers.insert(
            (namespace, tool.into()),
            Arc::new(move |params| Box::pin(handler(params))),
        );
    }

    pub fn contains(&self, namespace: Option<&str>, tool: &str) -> bool {
        self.handlers
            .contains_key(&(namespace.map(str::to_string), tool.to_string()))
    }

    pub async fn call(
        &self,
        params: DynamicToolCallParams,
    ) -> Result<DynamicToolCallResponse, ClientToolError> {
        let key = (params.namespace.clone(), params.tool.clone());
        let handler = self.handlers.get(&key).ok_or_else(|| {
            ClientToolError::new(format!(
                "no Astral client handler registered for {}",
                qualified_name(params.namespace.as_deref(), &params.tool)
            ))
        })?;
        handler(params).await
    }
}

fn qualified_name(namespace: Option<&str>, tool: &str) -> String {
    namespace.map_or_else(
        || tool.to_string(),
        |namespace| format!("{namespace}/{tool}"),
    )
}

#[cfg(test)]
#[path = "client_tools_tests.rs"]
mod tests;
