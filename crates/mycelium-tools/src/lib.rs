//! mycelium 工具抽象（Tool SPI）与注册表。
//!
//! 工具是模型可调用的能力单元：实现 [`Tool`] 并提供 [`ToolSpec`]
//! （名称 / 描述 / 入参 JSON Schema），注册进 [`ToolRegistry`] 后即可被模型调用。
//!
//! 执行契约（Planned，v0.2 起由运行时强制）见 `docs/architecture/lifecycle.md` §7。

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod policy;
pub mod registry;
pub mod schema;
pub mod tool;

pub use policy::{AllowAll, AllowList, DenyList, PolicyDecision, ToolPolicy};
pub use registry::{RegistryError, ToolRegistry};
pub use schema::{SchemaViolation, validate_arguments};
pub use tool::{Tool, ToolContext, ToolOutput};

pub use async_trait::async_trait;
pub use mycelium_core::error::ToolError;
pub use mycelium_core::tool::ToolSpec;

/// 过程宏 re-export：`#[mycelium_tools::tool]` 由普通函数生成 [`Tool`] 实现。
pub use mycelium_macros::tool;

/// 供过程宏生成代码使用的隐藏接口（不属于稳定 API）。
#[doc(hidden)]
pub mod __private {
    pub use async_trait;
    pub use serde_json;

    use mycelium_core::ids::ToolName;
    use serde_json::Value;

    pub use crate::ToolError;
    use crate::ToolSpec;

    /// 由宏校验过名称与 schema 后构造 `ToolSpec`（仅在编译期保证的前提下调用）。
    ///
    /// 工具名与 schema 合法性由 `#[tool]` 宏在编译期校验；此函数仅供生成代码调用。
    // 允许 expect：失败路径仅在宏实现存在缺陷时可达（名称与 schema 已在编译期校验）。
    #[allow(clippy::expect_used)]
    #[doc(hidden)]
    #[must_use]
    pub fn spec(name: &str, description: &str, schema: Value) -> ToolSpec {
        // 名称与 schema 已由宏在编译期验证，这里失败属于宏缺陷。
        ToolSpec::new(
            ToolName::new(name).expect("宏已校验工具名"),
            description,
            schema,
        )
        .expect("宏已保证 schema 为 JSON 对象")
    }

    /// 构造工具名（名称已由宏在编译期校验）。
    // 允许 expect：失败路径仅在宏实现存在缺陷时可达。
    #[allow(clippy::expect_used)]
    #[doc(hidden)]
    #[must_use]
    pub fn tool_name(name: &str) -> ToolName {
        ToolName::new(name).expect("宏已校验工具名")
    }

    /// 从 JSON 入参中反序列化单个参数（供生成代码使用）。
    ///
    /// # Errors
    /// 缺参或类型不符时返回 [`mycelium_core::ToolError::InvalidArguments`]。
    #[doc(hidden)]
    pub fn parse_argument<T>(
        arguments: &Value,
        key: &str,
        tool: &ToolName,
    ) -> Result<T, mycelium_core::ToolError>
    where
        T: serde::de::DeserializeOwned,
    {
        let raw = arguments.get(key).cloned().unwrap_or(Value::Null);
        serde_json::from_value(raw).map_err(|error| mycelium_core::ToolError::InvalidArguments {
            tool: tool.clone(),
            detail: format!("参数 {key}: {error}"),
        })
    }
}
