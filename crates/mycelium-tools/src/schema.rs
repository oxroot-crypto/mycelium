//! 工具入参校验（JSON Schema 子集）。
//!
//! 支持的关键字：`type`（字符串或联合类型数组）、`properties`、`required`、
//! `additionalProperties: false`、`items`、`enum`；**未识别的关键字被忽略**
//! （子集语义，见 `docs/spec/core.md` §2.13）。
//!
//! 失败路径使用 JSONPath 风格：`$`、`$.a`、`$[0]`、`$.a[0].b`。

use serde_json::Value;

/// 校验失败（含 JSON 路径与原因）。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("路径 {path} 校验失败：{message}")]
pub struct SchemaViolation {
    /// 违规位置（JSONPath 风格）。
    pub path: String,
    /// 违规原因。
    pub message: String,
}

impl SchemaViolation {
    /// 构造违规记录。
    fn new(path: &str, message: impl Into<String>) -> Self {
        Self {
            path: path.to_owned(),
            message: message.into(),
        }
    }
}

/// 按 JSON Schema 子集校验入参。
///
/// # Arguments
/// * `schema` - 工具声明 `input_schema`（顶层通常是 `{"type": "object", ...}`）。
/// * `arguments` - 模型给出的原始入参。
///
/// # Errors
/// 返回首个违规位置（路径 + 原因）的 [`SchemaViolation`]。
///
/// # Examples
/// ```
/// use mycelium_tools::schema::validate_arguments;
///
/// let schema = serde_json::json!({
///     "type": "object",
///     "properties": { "path": { "type": "string" } },
///     "required": ["path"]
/// });
/// assert!(validate_arguments(&schema, &serde_json::json!({ "path": "a.txt" })).is_ok());
/// let violation = validate_arguments(&schema, &serde_json::json!({})).unwrap_err();
/// assert_eq!(violation.path, "$");
/// ```
pub fn validate_arguments(schema: &Value, arguments: &Value) -> Result<(), SchemaViolation> {
    validate_at(schema, arguments, "$")
}

/// 递归校验：`path` 为当前值在 JSON 文档中的位置。
fn validate_at(schema: &Value, value: &Value, path: &str) -> Result<(), SchemaViolation> {
    let Value::Object(schema) = schema else {
        // 非对象 schema 已在 ToolSpec 构造时被拒绝；此处按子集语义防御式跳过。
        return Ok(());
    };

    if let Some(type_spec) = schema.get("type")
        && !matches_type(type_spec, value)
    {
        return Err(SchemaViolation::new(
            path,
            format!(
                "期望类型 {}，实际 {}",
                describe_type_spec(type_spec),
                json_type_name(value)
            ),
        ));
    }

    if let Some(Value::Array(allowed)) = schema.get("enum")
        && !allowed.iter().any(|candidate| candidate == value)
    {
        return Err(SchemaViolation::new(
            path,
            format!("取值不在允许集合中：{}", Value::Array(allowed.clone())),
        ));
    }

    match value {
        Value::Object(object) => {
            if let Some(Value::Array(required)) = schema.get("required") {
                for key in required.iter().filter_map(Value::as_str) {
                    if !object.contains_key(key) {
                        return Err(SchemaViolation::new(path, format!("缺少必填字段 {key:?}")));
                    }
                }
            }
            let properties = schema.get("properties").and_then(Value::as_object);
            let allow_additional = schema.get("additionalProperties") != Some(&Value::Bool(false));
            for (key, child) in object {
                match properties.and_then(|properties| properties.get(key)) {
                    Some(child_schema) => {
                        validate_at(child_schema, child, &format!("{path}.{key}"))?;
                    }
                    None if !allow_additional => {
                        return Err(SchemaViolation::new(
                            path,
                            format!("不允许的字段 {key:?}（additionalProperties: false）"),
                        ));
                    }
                    None => {}
                }
            }
        }
        Value::Array(items) => {
            if let Some(item_schema) = schema.get("items") {
                for (index, item) in items.iter().enumerate() {
                    validate_at(item_schema, item, &format!("{path}[{index}]"))?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// 判断 `type` 声明（字符串或数组）是否接受该值。
fn matches_type(type_spec: &Value, value: &Value) -> bool {
    match type_spec {
        Value::String(name) => type_matches(name, value),
        Value::Array(names) => names
            .iter()
            .filter_map(Value::as_str)
            .any(|name| type_matches(name, value)),
        _ => true,
    }
}

/// 单个类型名是否匹配。
fn type_matches(type_name: &str, value: &Value) -> bool {
    match type_name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        // JSON Schema 语义：数学意义上的整数（1.0 也视为 integer）。
        "integer" => value.as_f64().is_some_and(|number| number.fract() == 0.0),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        // 未识别的类型名：子集语义忽略。
        _ => true,
    }
}

/// 生成 `type` 声明的人类可读描述。
fn describe_type_spec(type_spec: &Value) -> String {
    match type_spec {
        Value::String(name) => name.clone(),
        Value::Array(names) => names
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" | "),
        other => other.to_string(),
    }
}

/// JSON 值的人类可读类型名。
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "lines": { "type": "integer", "minimum": 1 }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    #[test]
    fn accepts_valid_object() {
        assert!(validate_arguments(&schema(), &json!({ "path": "a.txt" })).is_ok());
        assert!(validate_arguments(&schema(), &json!({ "path": "a.txt", "lines": 3 })).is_ok());
        // integer 语义：1.0 合法，1.5 非法
        assert!(validate_arguments(&schema(), &json!({ "path": "a", "lines": 1.0 })).is_ok());
        assert!(validate_arguments(&schema(), &json!({ "path": "a", "lines": 1.5 })).is_err());
        // 联合类型
        let union = json!({ "type": ["string", "null"] });
        assert!(validate_arguments(&union, &json!("x")).is_ok());
        assert!(validate_arguments(&union, &json!(null)).is_ok());
        assert!(validate_arguments(&union, &json!(1)).is_err());
    }

    #[test]
    fn rejects_type_mismatch() {
        let violation = validate_arguments(&schema(), &json!({ "path": 1 })).unwrap_err();
        assert_eq!(violation.path, "$.path");
        assert!(violation.message.contains("string"));
        assert!(violation.message.contains("number"));

        let violation = validate_arguments(&schema(), &json!(1)).unwrap_err();
        assert_eq!(violation.path, "$");
        assert!(violation.message.contains("object"));
    }

    #[test]
    fn rejects_missing_required() {
        let violation = validate_arguments(&schema(), &json!({})).unwrap_err();
        assert_eq!(violation.path, "$");
        assert!(violation.message.contains("path"));
    }

    #[test]
    fn rejects_additional_property_when_disallowed() {
        let violation =
            validate_arguments(&schema(), &json!({ "path": "a", "extra": 1 })).unwrap_err();
        assert_eq!(violation.path, "$");
        assert!(violation.message.contains("extra"));
    }

    #[test]
    fn allows_additional_properties_by_default() {
        let open = json!({
            "type": "object",
            "properties": { "path": { "type": "string" } }
        });
        assert!(validate_arguments(&open, &json!({ "path": "a", "extra": 1 })).is_ok());
    }

    #[test]
    fn rejects_enum_mismatch() {
        let schema = json!({
            "type": "object",
            "properties": { "mode": { "enum": ["fast", "slow"] } }
        });
        assert!(validate_arguments(&schema, &json!({ "mode": "fast" })).is_ok());
        let violation = validate_arguments(&schema, &json!({ "mode": "medium" })).unwrap_err();
        assert_eq!(violation.path, "$.mode");
        assert!(violation.message.contains("fast"));
    }

    #[test]
    fn validates_array_items() {
        let schema = json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": { "id": { "type": "integer" } },
                        "required": ["id"]
                    }
                }
            }
        });
        assert!(
            validate_arguments(&schema, &json!({ "items": [{ "id": 1 }, { "id": 2 }] })).is_ok()
        );
        assert!(
            validate_arguments(&schema, &json!({ "items": [{ "id": 1 }, { "id": "x" }] })).is_err()
        );
        assert!(validate_arguments(&schema, &json!({ "items": [{}] })).is_err());
    }

    #[test]
    fn reports_nested_paths() {
        let schema = json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": { "id": { "type": "integer" } }
                    }
                }
            }
        });
        let violation =
            validate_arguments(&schema, &json!({ "items": [{ "id": 1 }, { "id": "x" }] }))
                .unwrap_err();
        assert_eq!(violation.path, "$.items[1].id");
    }

    #[test]
    fn ignores_unknown_keywords() {
        // minimum / pattern 等未识别关键字按子集语义忽略（不误报）
        let schema = json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "pattern": "^[a-z]+$" },
                "lines": { "type": "integer", "minimum": 5 }
            }
        });
        assert!(validate_arguments(&schema, &json!({ "path": "UPPER", "lines": 1 })).is_ok());
    }
}
