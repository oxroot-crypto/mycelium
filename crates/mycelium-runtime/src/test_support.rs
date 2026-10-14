//! 测试辅助（仅在测试构建中编译）。

use std::sync::Arc;

use mycelium_core::{StopReason, ToolError, ToolName, ToolSpec};
use mycelium_model::{
    ModelCapabilities, ModelDelta, ModelError, ModelProvider, ModelRequest, ModelStream,
    async_trait, stream_from_deltas,
};
use mycelium_tools::{Tool, ToolContext, ToolOutput};

/// 固定产出 "pong" 文本流的桩提供方。
pub(crate) struct StubProvider;

#[async_trait]
impl ModelProvider for StubProvider {
    fn id(&self) -> &str {
        "stub"
    }

    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            supports_streaming: true,
            ..Default::default()
        }
    }

    async fn stream(&self, _request: ModelRequest) -> Result<ModelStream, ModelError> {
        Ok(stream_from_deltas(vec![
            ModelDelta::TextDelta {
                text: "pong".to_owned(),
            },
            ModelDelta::Finished {
                stop_reason: StopReason::EndTurn,
            },
        ]))
    }
}

/// 返回共享的桩提供方。
pub(crate) fn stub_provider() -> Arc<dyn ModelProvider> {
    Arc::new(StubProvider)
}

/// 固定返回文本的桩工具。
pub(crate) struct StubTool {
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

/// 构造指定名称的桩工具。
pub(crate) fn stub_tool(name: &str) -> Arc<dyn Tool> {
    Arc::new(StubTool {
        spec: ToolSpec::new(
            ToolName::new(name).unwrap(),
            "stub tool",
            serde_json::json!({ "type": "object" }),
        )
        .unwrap(),
    })
}
