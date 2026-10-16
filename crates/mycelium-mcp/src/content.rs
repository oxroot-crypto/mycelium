//! MCP `content` 值 → 内容块映射（tools / prompts 共用）。
//!
//! 语义（FC-POST-233）：`content` 可以是单个内容对象或内容块数组；
//! `text` / `image` 一一映射为 `ToolResultBlock`；未知类型与缺字段块忽略（防御式）。

use mycelium_core::message::ToolResultBlock;
use serde_json::Value;

/// 把 MCP `content` 映射为内容块列表。
pub(crate) fn blocks_from_content(content: &Value) -> Vec<ToolResultBlock> {
    let mut blocks = Vec::new();
    if let Some(items) = content.as_array() {
        for item in items {
            push_block(&mut blocks, item);
        }
    } else if content.is_object() {
        push_block(&mut blocks, content);
    }
    blocks
}

/// 映射单个内容块。
fn push_block(blocks: &mut Vec<ToolResultBlock>, item: &Value) {
    match item.get("type").and_then(Value::as_str) {
        Some("text") => {
            if let Some(text) = item.get("text").and_then(Value::as_str) {
                blocks.push(ToolResultBlock::Text {
                    text: text.to_owned(),
                });
            }
        }
        Some("image") => {
            if let (Some(media_type), Some(data)) = (
                item.get("mimeType").and_then(Value::as_str),
                item.get("data").and_then(Value::as_str),
            ) {
                blocks.push(ToolResultBlock::Image {
                    media_type: media_type.to_owned(),
                    data: data.to_owned(),
                });
            }
        }
        // 其它类型（audio / resource）当前忽略，计划随 MCP 资源能力扩展。
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn maps_single_object_and_array() {
        let single = json!({ "type": "text", "text": "hi" });
        assert_eq!(
            blocks_from_content(&single),
            vec![ToolResultBlock::Text {
                text: "hi".to_owned()
            }]
        );

        let array = json!([
            { "type": "text", "text": "a" },
            { "type": "image", "mimeType": "image/png", "data": "aGVsbG8=" },
            { "type": "audio", "data": "ignored" }
        ]);
        let blocks = blocks_from_content(&array);
        assert_eq!(blocks.len(), 2);
        assert!(
            matches!(&blocks[1], ToolResultBlock::Image { media_type, .. } if media_type == "image/png")
        );
    }
}
