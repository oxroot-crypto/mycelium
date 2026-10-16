//! mycelium 的 MCP（Model Context Protocol）客户端适配。
//!
//! 覆盖 MCP stdio 传输的工具能力（契约见 `docs/spec/core.md` §2.19）：
//!
//! - 传输：换行分隔的 JSON-RPC 2.0（stdio 子进程，或任意双向流）；
//! - 握手：`initialize` → `notifications/initialized`；
//! - 发现：`tools/list`（支持 `nextCursor` 分页）→ [`mycelium_core::ToolSpec`]；
//! - 调用：`tools/call` → [`mycelium_tools::ToolOutput`]；
//! - [`McpClient::tools`] 产出的 `Arc<dyn Tool>` 可直接注册进 `AgentRuntime`。
//!
//! # Examples
//! ```no_run
//! use std::sync::Arc;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Arc::new(mycelium_mcp::McpClient::spawn("npx", &["-y", "@modelcontextprotocol/server-everything"]).await?);
//! for tool in client.tools().await? {
//!     println!("MCP 工具：{}", tool.spec().name());
//! }
//! client.shutdown().await?;
//! # Ok(())
//! # }
//! ```

mod client;
mod tool;

pub use client::{MAX_TOOL_PAGES, McpClient, McpServerInfo, PROTOCOL_VERSION};
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
