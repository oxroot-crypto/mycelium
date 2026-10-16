//! mycelium 的 MCP（Model Context Protocol）客户端适配。
//!
//! 覆盖 MCP stdio 与 streamable HTTP 传输的能力（契约见 `docs/spec/core.md` §2.19 / §2.29）：
//!
//! - 传输：换行分隔的 JSON-RPC 2.0（stdio 子进程 / 任意双向流）与 streamable HTTP；
//! - 握手：`initialize` → `notifications/initialized`（HTTP 模式捕获 `Mcp-Session-Id`）；
//! - 工具：`tools/list`（支持 `nextCursor` 分页）→ [`mycelium_core::ToolSpec`]；`tools/call`；
//! - 资源：`resources/list` → [`McpResource`]；`resources/read` → [`McpResourceContent`]；
//! - 提示模板：`prompts/list` → [`McpPrompt`]；`prompts/get` → [`McpPromptMessage`]；
//! - [`McpClient::tools`] 产出的 `Arc<dyn Tool>` 可直接注册进 `AgentRuntime`。
//!
//! # Examples
//! ```no_run
//! use std::sync::Arc;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // stdio 子进程
//! let client = Arc::new(mycelium_mcp::McpClient::spawn("npx", &["-y", "@modelcontextprotocol/server-everything"]).await?);
//! for tool in client.tools().await? {
//!     println!("MCP 工具：{}", tool.spec().name());
//! }
//! client.shutdown().await?;
//!
//! // streamable HTTP 端点
//! let http = mycelium_mcp::McpClient::connect_http("https://example.com/mcp").await?;
//! let _ = http.list_resources().await?;
//! http.shutdown().await?;
//! # Ok(())
//! # }
//! ```

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod client;
mod content;
mod http;
mod prompts;
mod resources;
mod tool;
mod transport;

pub use client::{MAX_LIST_PAGES, McpClient, McpServerInfo, PROTOCOL_VERSION};
pub use prompts::{McpPrompt, McpPromptArgument, McpPromptMessage};
pub use resources::{McpResource, McpResourceContent};
pub use tool::McpTool;

/// MCP 客户端错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum McpError {
    /// 传输层失败（IO / 子进程）。
    #[error("MCP 传输失败：{message}")]
    Transport {
        /// 失败描述。
        message: String,
    },
    /// 传输已关闭（未完成的请求已显式失败，不得悬挂）。
    #[error("MCP 传输已关闭（未完成的请求已显式失败）")]
    TransportClosed,
    /// 协议错误（缺失字段 / 非法数据）。
    #[error("MCP 协议错误：{message}")]
    Protocol {
        /// 违约细节。
        message: String,
    },
    /// 服务端 JSON-RPC 错误。
    #[error("MCP 服务返回错误：code={code}，{message}")]
    Rpc {
        /// JSON-RPC 错误码。
        code: i64,
        /// 错误描述。
        message: String,
    },
}
