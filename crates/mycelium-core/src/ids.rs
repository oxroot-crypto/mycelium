//! 标识符类型。
//!
//! 所有标识符均为 newtype 包装，**构造即校验**：非法值无法进入系统内部。
//! 对应契约见 `docs/spec/core.md` 的 FC-PRE-001 ~ FC-PRE-004。

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use crate::error::{InvalidModelId, InvalidToolCallId, InvalidToolName};

/// 会话标识（UUID v7，时间有序，便于持久化排序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(Uuid);

impl SessionId {
    /// 生成一个新的会话标识（UUID v7）。
    pub fn generate() -> Self {
        Self(Uuid::now_v7())
    }

    /// 从已有 UUID 构造（持久化恢复场景）。
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// 返回内部 UUID。
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 运行标识（UUID v7，时间有序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunId(Uuid);

impl RunId {
    /// 生成一个新的运行标识（UUID v7）。
    pub fn generate() -> Self {
        Self(Uuid::now_v7())
    }

    /// 从已有 UUID 构造（持久化恢复场景）。
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// 返回内部 UUID。
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 模型标识（例如 `"claude-sonnet-4-5"`、`"gpt-5"`）。
///
/// 不变量：`trim` 后非空。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ModelId(String);

impl ModelId {
    /// 构造模型标识。
    ///
    /// # Errors
    /// 当 `value` 为空或仅含空白时返回 [`InvalidModelId`]。
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidModelId> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(InvalidModelId);
        }
        Ok(Self(value))
    }

    /// 以 `&str` 形式返回内部值。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ModelId {
    type Err = InvalidModelId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl<'de> Deserialize<'de> for ModelId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::new(raw).map_err(serde::de::Error::custom)
    }
}

/// 工具名。
///
/// 不变量：`1..=64` 个字符，仅允许 ASCII 字母、数字、`_`、`-`；该域与主流模型提供方
/// （Anthropic / OpenAI）对工具名的约束一致。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ToolName(String);

impl ToolName {
    /// 允许的最大字符数。
    pub const MAX_LEN: usize = 64;

    /// 构造工具名。
    ///
    /// # Errors
    /// 按以下优先级返回 [`InvalidToolName`]：
    /// 1. 空字符串 → `Empty`；
    /// 2. 字符数超过 [`ToolName::MAX_LEN`] → `TooLong`；
    /// 3. 包含 `[A-Za-z0-9_-]` 之外的字符 → `InvalidCharacter`。
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidToolName> {
        let value = value.into();
        if value.is_empty() {
            return Err(InvalidToolName::Empty);
        }
        let len = value.chars().count();
        if len > Self::MAX_LEN {
            return Err(InvalidToolName::TooLong {
                len,
                max: Self::MAX_LEN,
            });
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return Err(InvalidToolName::InvalidCharacter { value });
        }
        Ok(Self(value))
    }

    /// 以 `&str` 形式返回内部值。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ToolName {
    type Err = InvalidToolName;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl<'de> Deserialize<'de> for ToolName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::new(raw).map_err(serde::de::Error::custom)
    }
}

/// 工具调用标识（由模型提供方生成的不透明字符串）。
///
/// 不变量：`trim` 后非空（保留原始值，不做裁剪）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ToolCallId(String);

impl ToolCallId {
    /// 构造工具调用标识。
    ///
    /// # Errors
    /// 当 `value` 为空或仅含空白时返回 [`InvalidToolCallId`]。
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidToolCallId> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(InvalidToolCallId);
        }
        Ok(Self(value))
    }

    /// 以 `&str` 形式返回内部值。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolCallId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ToolCallId {
    type Err = InvalidToolCallId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl<'de> Deserialize<'de> for ToolCallId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::new(raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_name_accepts_boundary_lengths() {
        // 边界值独立于实现常量硬编码（64/65），保证实现被篡改时测试能够变红（证伪原则）。
        let single = "a";
        let max = "a".repeat(64);
        assert_eq!(ToolName::new(single).unwrap().as_str(), single);
        assert_eq!(ToolName::new(max.clone()).unwrap().as_str(), max);
        assert_eq!(ToolName::new("a-b_c9").unwrap().as_str(), "a-b_c9");
    }

    #[test]
    fn tool_name_rejects_empty() {
        assert_eq!(ToolName::new("").unwrap_err(), InvalidToolName::Empty);
    }

    #[test]
    fn tool_name_rejects_too_long() {
        let too_long = "a".repeat(65);
        assert_eq!(
            ToolName::new(too_long).unwrap_err(),
            InvalidToolName::TooLong { len: 65, max: 64 }
        );
    }

    #[test]
    fn tool_name_rejects_invalid_characters() {
        for raw in ["a b", "工具", "a.b", "a/b", "a\n"] {
            assert!(
                matches!(
                    ToolName::new(raw),
                    Err(InvalidToolName::InvalidCharacter { .. })
                ),
                "应当拒绝：{raw:?}"
            );
        }
    }

    #[test]
    fn tool_name_deserialize_matches_new_validation() {
        let ok: ToolName = serde_json::from_str("\"read_file\"").unwrap();
        assert_eq!(ok.as_str(), "read_file");
        assert!(serde_json::from_str::<ToolName>("\"\"").is_err());
        assert!(serde_json::from_str::<ToolName>("\"a b\"").is_err());
        let too_long = format!("\"{}\"", "a".repeat(65));
        assert!(serde_json::from_str::<ToolName>(&too_long).is_err());
    }

    #[test]
    fn tool_call_id_rejects_blank() {
        assert_eq!(ToolCallId::new("").unwrap_err(), InvalidToolCallId);
        assert_eq!(ToolCallId::new("   \t").unwrap_err(), InvalidToolCallId);
    }

    #[test]
    fn tool_call_id_accepts_non_blank() {
        assert_eq!(ToolCallId::new("call_1").unwrap().as_str(), "call_1");
        // 保留原始值，不做裁剪
        assert_eq!(ToolCallId::new(" p ").unwrap().as_str(), " p ");
    }

    #[test]
    fn model_id_rejects_blank() {
        assert_eq!(ModelId::new("").unwrap_err(), InvalidModelId);
        assert_eq!(ModelId::new("   ").unwrap_err(), InvalidModelId);
        assert_eq!(
            ModelId::new("claude-sonnet-4-5").unwrap().as_str(),
            "claude-sonnet-4-5"
        );
    }
}
