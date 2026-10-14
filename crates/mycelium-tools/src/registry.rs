//! 工具注册表。

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use mycelium_core::ids::ToolName;
use mycelium_core::tool::ToolSpec;

use crate::tool::Tool;

/// 注册表错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    /// 同名工具已注册（注册表内容保持不变）。
    #[error("工具名已注册：{name}")]
    DuplicateName {
        /// 冲突的工具名。
        name: ToolName,
    },
}

/// 工具注册表。
///
/// 不变量：内部按名称唯一索引；[`ToolRegistry::specs`] 按名称字典序返回（确定性输出）。
#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<ToolName, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// 创建空注册表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册工具。
    ///
    /// # Errors
    /// 已存在同名工具时返回 [`RegistryError::DuplicateName`]，且**不覆盖**原有工具。
    pub fn register(&mut self, tool: Arc<dyn Tool>) -> Result<(), RegistryError> {
        let name = tool.spec().name().clone();
        if self.tools.contains_key(&name) {
            return Err(RegistryError::DuplicateName { name });
        }
        self.tools.insert(name, tool);
        Ok(())
    }

    /// 按名称查找工具。
    pub fn get(&self, name: &ToolName) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// 返回全部工具规格（按名称字典序）。
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|tool| tool.spec()).collect()
    }

    /// 已注册工具数量。
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// 注册表是否为空。
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolRegistry")
            .field("tools", &self.tools.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use mycelium_core::{ToolError, ToolName};

    use super::*;
    use crate::tool::{ToolContext, ToolOutput};

    struct StubTool {
        spec: ToolSpec,
    }

    #[async_trait]
    impl Tool for StubTool {
        fn spec(&self) -> ToolSpec {
            self.spec.clone()
        }

        async fn call(
            &self,
            _context: &ToolContext,
            _arguments: serde_json::Value,
        ) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::success("stub"))
        }
    }

    fn stub(name: &str) -> Arc<dyn Tool> {
        Arc::new(StubTool {
            spec: ToolSpec::new(
                ToolName::new(name).unwrap(),
                "stub",
                serde_json::json!({ "type": "object" }),
            )
            .unwrap(),
        })
    }

    #[test]
    fn register_rejects_duplicate_and_keeps_original() {
        let mut registry = ToolRegistry::new();
        let original = stub("echo");
        registry.register(original.clone()).unwrap();

        let err = registry
            .register(stub("echo"))
            .expect_err("重名注册必须被拒绝");
        assert_eq!(
            err,
            RegistryError::DuplicateName {
                name: ToolName::new("echo").unwrap()
            }
        );

        // 原工具保留且数量不变
        assert_eq!(registry.len(), 1);
        let kept = registry.get(&ToolName::new("echo").unwrap()).unwrap();
        assert_eq!(kept.spec().name(), original.spec().name());
    }

    #[test]
    fn specs_are_sorted_by_name() {
        let mut registry = ToolRegistry::new();
        for name in ["zeta", "alpha", "mid_1"] {
            registry.register(stub(name)).unwrap();
        }
        let names: Vec<String> = registry
            .specs()
            .iter()
            .map(|spec| spec.name().as_str().to_owned())
            .collect();
        assert_eq!(names, vec!["alpha", "mid_1", "zeta"]);
    }

    #[test]
    fn get_unknown_returns_none() {
        let mut registry = ToolRegistry::new();
        registry.register(stub("known")).unwrap();
        assert!(registry.get(&ToolName::new("unknown").unwrap()).is_none());
    }

    #[test]
    fn len_and_is_empty_track_registrations() {
        let mut registry = ToolRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        registry.register(stub("a")).unwrap();
        registry.register(stub("b")).unwrap();
        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 2);
    }
}
