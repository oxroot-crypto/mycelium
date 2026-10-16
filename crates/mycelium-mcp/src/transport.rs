//! MCP 传输抽象。

use async_trait::async_trait;
use serde_json::Value;

use crate::McpError;

/// JSON-RPC 传输（stdio / streamable HTTP）。
///
/// 语义（FC-POST-234 / FC-ERR-090）：
/// - [`Transport::request`] 发出请求并等待**匹配 id** 的响应；其余消息（通知等）忽略；
/// - [`Transport::notify`] 发出通知（无响应）；
/// - [`Transport::close`] 主动关闭；未完成请求必须显式失败，不得悬挂。
#[async_trait]
pub(crate) trait Transport: Send + Sync + 'static {
    /// 发送请求并等待匹配响应。
    async fn request(&self, method: &str, params: Option<Value>) -> Result<Value, McpError>;

    /// 发送通知（无响应）。
    async fn notify(&self, method: &str, params: Option<Value>) -> Result<(), McpError>;

    /// 主动关闭传输（默认无操作）。
    async fn close(&self) {}
}
