//! 工具权限策略（执行前校验）。
//!
//! 语义（见 `docs/spec/core.md` §2.17）：
//! - 策略在入参校验与工具执行**之前**调用；
//! - [`PolicyDecision::Deny`] 时工具不会被调用，拒绝原因以错误结果回灌给模型；
//! - 策略检查可被运行取消打断（由运行时与 `cancel` 竞争）。

use std::collections::HashSet;

use async_trait::async_trait;
use serde_json::Value;

use mycelium_core::ids::ToolName;

/// 策略决策。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// 允许执行。
    Allow,
    /// 拒绝执行；`reason` 会作为错误结果回灌给模型。
    Deny {
        /// 拒绝原因。
        reason: String,
    },
}

impl PolicyDecision {
    /// 允许执行。
    pub fn allow() -> Self {
        Self::Allow
    }

    /// 拒绝执行并附带原因。
    pub fn deny(reason: impl Into<String>) -> Self {
        Self::Deny {
            reason: reason.into(),
        }
    }

    /// 是否为允许。
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }
}

/// 工具权限策略（SPI 扩展点）。
///
/// 实现约定：
/// - 不得 panic；失败应归约为 `Deny`（携带原因）；
/// - 耗时操作（如人工审批）应响应取消：运行时会在 `cancel` 触发时竞争放弃等待；
/// - 默认策略为 [`AllowAll`]，未配置策略时行为与 v0.2 一致。
#[async_trait]
pub trait ToolPolicy: Send + Sync + 'static {
    /// 检查一次工具调用是否可以执行。
    ///
    /// # Arguments
    /// * `tool` - 工具名。
    /// * `arguments` - 模型给出的原始入参（只读）。
    async fn check(&self, tool: &ToolName, arguments: &Value) -> PolicyDecision;
}

/// 允许一切（默认策略）。
#[derive(Debug, Default, Clone, Copy)]
pub struct AllowAll;

#[async_trait]
impl ToolPolicy for AllowAll {
    async fn check(&self, _tool: &ToolName, _arguments: &Value) -> PolicyDecision {
        PolicyDecision::Allow
    }
}

/// 拒绝列表策略：列表内的工具一律拒绝，其余允许。
#[derive(Debug, Default, Clone)]
pub struct DenyList {
    names: HashSet<ToolName>,
}

impl DenyList {
    /// 由工具名集合创建。
    pub fn new(names: impl IntoIterator<Item = ToolName>) -> Self {
        Self {
            names: names.into_iter().collect(),
        }
    }

    /// 追加一个被拒绝的工具名。
    pub fn insert(&mut self, name: ToolName) {
        self.names.insert(name);
    }
}

#[async_trait]
impl ToolPolicy for DenyList {
    async fn check(&self, tool: &ToolName, _arguments: &Value) -> PolicyDecision {
        if self.names.contains(tool) {
            PolicyDecision::deny(format!("工具 {tool} 在拒绝列表中"))
        } else {
            PolicyDecision::Allow
        }
    }
}

/// 允许列表策略：仅列表内的工具可以执行，其余拒绝。
#[derive(Debug, Default, Clone)]
pub struct AllowList {
    names: HashSet<ToolName>,
}

impl AllowList {
    /// 由工具名集合创建。
    pub fn new(names: impl IntoIterator<Item = ToolName>) -> Self {
        Self {
            names: names.into_iter().collect(),
        }
    }

    /// 追加一个被允许的工具名。
    pub fn insert(&mut self, name: ToolName) {
        self.names.insert(name);
    }
}

#[async_trait]
impl ToolPolicy for AllowList {
    async fn check(&self, tool: &ToolName, _arguments: &Value) -> PolicyDecision {
        if self.names.contains(tool) {
            PolicyDecision::Allow
        } else {
            PolicyDecision::deny(format!("工具 {tool} 不在允许列表中"))
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn name(raw: &str) -> ToolName {
        ToolName::new(raw).unwrap()
    }

    #[tokio::test]
    async fn allow_all_permits_everything() {
        let policy = AllowAll;
        assert!(
            policy
                .check(&name("anything"), &json!({}))
                .await
                .is_allowed()
        );
    }

    #[tokio::test]
    async fn deny_list_rejects_listed_names() {
        let policy = DenyList::new([name("rm_rf"), name("format_disk")]);
        assert!(!policy.check(&name("rm_rf"), &json!({})).await.is_allowed());
        assert!(
            policy
                .check(&name("read_file"), &json!({}))
                .await
                .is_allowed()
        );

        let mut policy = DenyList::default();
        policy.insert(name("blocked"));
        match policy.check(&name("blocked"), &json!({})).await {
            PolicyDecision::Deny { reason } => assert!(reason.contains("blocked")),
            PolicyDecision::Allow => panic!("应拒绝"),
        }
    }

    #[tokio::test]
    async fn allow_list_allows_only_listed_names() {
        let mut policy = AllowList::default();
        policy.insert(name("read_file"));
        assert!(
            policy
                .check(&name("read_file"), &json!({}))
                .await
                .is_allowed()
        );
        match policy.check(&name("write_file"), &json!({})).await {
            PolicyDecision::Deny { reason } => assert!(reason.contains("不在允许列表")),
            PolicyDecision::Allow => panic!("应拒绝"),
        }
    }

    #[tokio::test]
    async fn check_called_before_execution() {
        // 契约 FC-POST-112：策略在入参校验前调用——即使入参为空对象也会触发策略检查。
        let policy = DenyList::new([name("echo")]);
        let decision = policy.check(&name("echo"), &json!(null)).await;
        assert!(!decision.is_allowed());
    }
}
