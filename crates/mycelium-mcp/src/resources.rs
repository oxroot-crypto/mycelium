//! MCP 资源类型（`resources/list` / `resources/read`）。

use serde::Serialize;
use serde_json::Value;

use crate::McpError;

/// 资源描述（`resources/list` 条目）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct McpResource {
    /// 资源 URI（如 `file:///data/report.csv`）。
    pub uri: String,
    /// 人类可读名称。
    pub name: String,
    /// 描述（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// MIME 类型（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

impl McpResource {
    /// 从 `resources/list` 条目解析（`uri` / `name` 必填且非空，否则 `Protocol`）。
    pub(crate) fn from_value(value: &Value) -> Result<Self, McpError> {
        let uri = required_string(value, "uri", "resource")?;
        let name = required_string(value, "name", "resource")?;
        Ok(Self {
            uri,
            name,
            description: optional_string(value, "description"),
            mime_type: optional_string(value, "mimeType"),
        })
    }
}

/// 资源内容块（`resources/read` 条目）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct McpResourceContent {
    /// 资源 URI。
    pub uri: String,
    /// MIME 类型（可选）。
    pub mime_type: Option<String>,
    /// 文本内容（与 `blob` 至少一项）。
    pub text: Option<String>,
    /// base64 二进制内容（与 `text` 至少一项）。
    pub blob: Option<String>,
}

impl McpResourceContent {
    /// 从 `resources/read` 条目解析（`uri` 必填；`text` / `blob` 至少一项，否则 `Protocol`）。
    pub(crate) fn from_value(value: &Value) -> Result<Self, McpError> {
        let uri = required_string(value, "uri", "resource content")?;
        let text = optional_string(value, "text");
        let blob = optional_string(value, "blob");
        if text.is_none() && blob.is_none() {
            return Err(McpError::Protocol {
                message: format!("资源 {uri} 的 contents 条目缺少 text 与 blob"),
            });
        }
        Ok(Self {
            uri,
            mime_type: optional_string(value, "mimeType"),
            text,
            blob,
        })
    }
}

/// 读取必填字符串字段（缺失 / 非字符串 / 空白 → `Protocol`）。
pub(crate) fn required_string(value: &Value, key: &str, context: &str) -> Result<String, McpError> {
    let raw = value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| McpError::Protocol {
            message: format!("{context} 缺少 {key}"),
        })?;
    if raw.trim().is_empty() {
        return Err(McpError::Protocol {
            message: format!("{context} 的 {key} 为空"),
        });
    }
    Ok(raw.to_owned())
}

/// 读取可选字符串字段。
pub(crate) fn optional_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}
