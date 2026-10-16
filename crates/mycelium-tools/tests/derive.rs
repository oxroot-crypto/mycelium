//! `#[tool]` 派生宏的集成测试（契约见 `docs/spec/core.md` §2.18）。
// 测试代码可放宽 unwrap / expect / panic 限制（规范允许）。
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mycelium_core::{RunId, ToolCallId, ToolError};
use mycelium_tools::{ToolContext, ToolOutput, tool};

/// 加法工具：计算两个整数之和。
#[tool]
async fn add(a: i64, b: i64) -> Result<i64, String> {
    Ok(a + b)
}

/// 读取文件（演示属性覆盖与可选参数）。
#[tool(name = "read_file", description = "读取文件内容（截断）")]
async fn read_file(path: String, max_bytes: Option<u64>) -> Result<String, String> {
    let limit = max_bytes.unwrap_or(16);
    Ok(format!("{path}:{limit}"))
}

/// 同步函数：翻倍。
#[tool]
fn double(x: i64) -> Result<i64, String> {
    Ok(x * 2)
}

/// 直接返回 [`ToolOutput`]。
#[tool]
async fn raw(text: String) -> Result<ToolOutput, String> {
    Ok(ToolOutput::success(text))
}

/// 总是失败（验证错误映射）。
#[tool]
async fn always_fail() -> Result<String, String> {
    Err("boom".to_owned())
}

fn context() -> ToolContext {
    ToolContext {
        run_id: RunId::generate(),
        session_id: None,
        call_id: ToolCallId::new("call_1").unwrap(),
        cancel: mycelium_core::CancellationToken::new(),
    }
}

#[test]
fn spec_matches_signature() {
    let tool = add_tool();
    let spec = tool.spec();
    assert_eq!(spec.name().as_str(), "add");
    assert_eq!(spec.description(), "加法工具：计算两个整数之和。");
    assert_eq!(
        spec.input_schema(),
        &serde_json::json!({
            "type": "object",
            "properties": { "a": { "type": "integer" }, "b": { "type": "integer" } },
            "required": ["a", "b"],
            "additionalProperties": false
        })
    );
}

#[test]
fn attributes_override_name_and_description() {
    let spec = read_file_tool().spec();
    assert_eq!(spec.name().as_str(), "read_file");
    assert_eq!(spec.description(), "读取文件内容（截断）");
    assert_eq!(
        spec.input_schema(),
        &serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "max_bytes": { "type": "integer" }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    );
}

#[tokio::test]
async fn executes_async_function() {
    let output = add_tool()
        .call(&context(), serde_json::json!({"a": 21, "b": 21}))
        .await
        .unwrap();
    assert_eq!(output.text(), "42");
    assert!(!output.is_error());

    // Option 参数缺省
    let output = read_file_tool()
        .call(&context(), serde_json::json!({"path": "p"}))
        .await
        .unwrap();
    assert_eq!(output.text(), "p:16");
}

#[tokio::test]
async fn executes_sync_function() {
    let output = double_tool()
        .call(&context(), serde_json::json!({"x": 3}))
        .await
        .unwrap();
    assert_eq!(output.text(), "6");
}

#[tokio::test]
async fn string_and_tool_output_passthrough() {
    // String 直传（而不是 JSON 引号包裹）
    let output = read_file_tool()
        .call(&context(), serde_json::json!({"path": "a", "max_bytes": 8}))
        .await
        .unwrap();
    assert_eq!(output.text(), "a:8");

    // ToolOutput 直传
    let output = raw_tool()
        .call(&context(), serde_json::json!({"text": "hello"}))
        .await
        .unwrap();
    assert_eq!(output.text(), "hello");
}

#[tokio::test]
async fn rejects_invalid_arguments() {
    // 类型不符
    let error = add_tool()
        .call(&context(), serde_json::json!({"a": "x", "b": 1}))
        .await
        .unwrap_err();
    match error {
        ToolError::InvalidArguments { detail, .. } => assert!(detail.contains("参数 a")),
        other => panic!("期望 InvalidArguments，实际：{other:?}"),
    }

    // 缺必填参数
    let error = add_tool()
        .call(&context(), serde_json::json!({"a": 1}))
        .await
        .unwrap_err();
    assert!(matches!(error, ToolError::InvalidArguments { .. }));
}

#[tokio::test]
async fn function_error_maps_to_execution_error() {
    let error = always_fail_tool()
        .call(&context(), serde_json::json!({}))
        .await
        .unwrap_err();
    match error {
        ToolError::Execution { message, .. } => assert_eq!(message, "boom"),
        other => panic!("期望 Execution，实际：{other:?}"),
    }
}
