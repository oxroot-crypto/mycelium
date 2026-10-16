//! MCP 工具的 [`Tool`] 代理。

use std::sync::Arc;

use async_trait::async_trait;
use mycelium_core::error::ToolError;
use mycelium_core::tool::ToolSpec;
use mycelium_tools::{Tool, ToolContext, ToolOutput};
use serde_json::Value;

use crate::McpClient;

/// 把 MCP 服务端工具代理为 mycelium [`Tool`]：`call` 转发 `tools/call`。
///
/// 通常通过 [`McpClient::tools`] 获取，而不是直接构造。
pub struct McpTool {
    client: Arc<McpClient>,
    spec: ToolSpec,
}

impl McpTool {
    /// 创建代理。
    pub fn new(client: Arc<McpClient>, spec: ToolSpec) -> Self {
        Self { client, spec }
    }
}

impl std::fmt::Debug for McpTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpTool")
            .field("name", self.spec.name())
            .finish()
    }
}

#[async_trait]
impl Tool for McpTool {
    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    async fn call(
        &self,
        _context: &ToolContext,
        arguments: Value,
    ) -> Result<ToolOutput, ToolError> {
        self.client
            .call_tool(self.spec.name(), arguments)
            .await
            .map_err(|error| ToolError::Execution {
                tool: self.spec.name().clone(),
                message: error.to_string(),
            })
    }
}
