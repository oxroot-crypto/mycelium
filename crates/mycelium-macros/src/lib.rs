//! mycelium 过程宏。
//!
//! 提供 [`macro@tool`] 属性宏：把普通函数（async 或 sync）转换为 `Tool` 实现。
//! 生成的代码只依赖 `mycelium_tools`（含其隐藏的 `__private` 重导出），
//! 调用方无需额外依赖 `serde` / `async-trait` 等 crate。

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::{
    Error, FnArg, GenericArgument, ItemFn, LitStr, Pat, PathArguments, ReturnType, Token, Type,
    parse_macro_input,
};

/// 由函数生成 `Tool` 实现（见 `docs/spec/core.md` §2.18）。
///
/// # 用法
/// ```ignore
/// /// 加法工具（描述取自文档注释首段）。
/// #[mycelium_tools::tool]
/// async fn add(a: i64, b: i64) -> Result<i64, String> {
///     Ok(a + b)
/// }
///
/// // 生成 `AddTool` 结构体与 `add_tool()` 帮助函数：
/// let tool: std::sync::Arc<dyn mycelium_tools::Tool> = add_tool();
/// ```
///
/// # 生成规则
/// - 工具名默认 = 函数名（可用 `#[tool(name = "...")]` 覆盖，需满足
///   `1..=64` 个 ASCII 字母/数字/`_`/`-`，编译期校验）；
/// - 描述默认 = 文档注释首段（可用 `description` 覆盖）；
/// - schema 由参数类型映射：`String` / 整数 / `f32|f64` / `bool` / `Vec<T>` / `Option<T>`；
///   `Option<T>` 不计入 `required`；其余类型生成空 schema（宽松接受）；
/// - 返回类型必须是路径名以 `Result` 结尾的类型：`String` / `ToolOutput` 直传，
///   其余类型 JSON 序列化；错误类型需实现 `Display`；
/// - 编译期拒绝：非法工具名、`self` 参数、`mut`/模式参数、引用参数、泛型函数。
#[proc_macro_attribute]
pub fn tool(attr: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);
    let args = parse_macro_input!(attr as ToolArgs);
    match expand_tool(&function, &args) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// `#[tool(...)]` 支持的参数（均可选）。
#[derive(Default)]
struct ToolArgs {
    name: Option<LitStr>,
    description: Option<LitStr>,
}

impl syn::parse::Parse for ToolArgs {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let mut args = Self::default();
        if input.is_empty() {
            return Ok(args);
        }
        let pairs = Punctuated::<syn::MetaNameValue, Token![,]>::parse_terminated(input)?;
        for pair in pairs {
            let key = pair
                .path
                .get_ident()
                .map(ToString::to_string)
                .unwrap_or_default();
            let value = match &pair.value {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(text),
                    ..
                }) => text.clone(),
                other => return Err(Error::new_spanned(other, "期望字符串字面量")),
            };
            match key.as_str() {
                "name" => args.name = Some(value),
                "description" => args.description = Some(value),
                other => {
                    return Err(Error::new_spanned(
                        &pair.path,
                        format!("未知参数 {other}（支持 name / description）"),
                    ));
                }
            }
        }
        Ok(args)
    }
}

/// 成功结果的转换方式。
enum OkKind {
    /// `ToolOutput` 直接返回。
    ToolOutput,
    /// `String` 直接作为内容。
    Text,
    /// 其余类型 JSON 序列化。
    Serialize,
}

/// 单个参数的生成信息。
struct ParamInfo {
    ident: syn::Ident,
    key: String,
    ty: Type,
    schema: TokenStream2,
    required: bool,
}

/// 生成工具实现。
fn expand_tool(function: &ItemFn, args: &ToolArgs) -> syn::Result<TokenStream2> {
    if function.sig.generics.params.iter().next().is_some() {
        return Err(Error::new_spanned(
            &function.sig.generics,
            "#[tool] 不支持泛型函数",
        ));
    }
    let fn_name = &function.sig.ident;

    let tool_name_lit = match &args.name {
        Some(lit) => {
            validate_tool_name(lit)?;
            lit.clone()
        }
        None => {
            let derived = LitStr::new(&fn_name.to_string(), fn_name.span());
            validate_tool_name(&derived)?;
            derived
        }
    };
    let description = match &args.description {
        Some(lit) => lit.value(),
        None => extract_doc_comment(&function.attrs),
    };

    let mut params: Vec<ParamInfo> = Vec::new();
    for input in &function.sig.inputs {
        let FnArg::Typed(pat_type) = input else {
            return Err(Error::new_spanned(input, "#[tool] 不支持 self 参数"));
        };
        let Pat::Ident(pat_ident) = &*pat_type.pat else {
            return Err(Error::new_spanned(
                &pat_type.pat,
                "#[tool] 仅支持简单标识符参数",
            ));
        };
        if pat_ident.mutability.is_some() {
            return Err(Error::new_spanned(&pat_type.pat, "#[tool] 参数不支持 mut"));
        }
        let ident = pat_ident.ident.clone();
        let (schema, required) = schema_tokens(&pat_type.ty)?;
        params.push(ParamInfo {
            key: ident.to_string(),
            ident,
            ty: (*pat_type.ty).clone(),
            schema,
            required,
        });
    }

    let ok_kind = detect_ok_kind(&function.sig.output)?;
    let is_async = function.sig.asyncness.is_some();

    let struct_name = format_ident!("{}Tool", to_pascal_case(&fn_name.to_string()));
    let helper_name = format_ident!("{}_tool", fn_name);
    let visibility = &function.vis;

    let bindings = params.iter().map(|param| {
        let ident = &param.ident;
        let ty = &param.ty;
        let key = &param.key;
        quote! {
            let #ident: #ty =
                ::mycelium_tools::__private::parse_argument(&arguments, #key, &__tool_name)?;
        }
    });
    let param_idents = params.iter().map(|param| &param.ident);
    let param_keys = params.iter().map(|param| &param.key);
    let param_schemas = params.iter().map(|param| &param.schema);
    let param_requireds = params.iter().map(|param| param.required);

    let call = quote! { #fn_name(#(#param_idents),*) };
    let invoke = if is_async {
        quote! { #call.await }
    } else {
        call
    };
    let success = match ok_kind {
        OkKind::ToolOutput => quote! { Ok(value) },
        OkKind::Text => quote! { Ok(::mycelium_tools::ToolOutput::success(value)) },
        OkKind::Serialize => quote! {
            match ::mycelium_tools::__private::serde_json::to_string(&value) {
                Ok(text) => Ok(::mycelium_tools::ToolOutput::success(text)),
                Err(error) => Err(::mycelium_tools::__private::ToolError::Execution {
                    tool: __tool_name,
                    message: format!("结果序列化失败：{error}"),
                }),
            }
        },
    };

    let struct_doc = LitStr::new(
        &format!("由 `#[tool]` 为函数 `{fn_name}` 生成的工具实现。"),
        Span::call_site(),
    );
    let helper_doc = LitStr::new(
        &format!("返回函数 `{fn_name}` 对应工具的 `Arc<dyn Tool>`（便于注册进运行时）。"),
        Span::call_site(),
    );

    Ok(quote! {
        #function

        #[doc = #struct_doc]
        #[derive(Debug, Default, Clone, Copy)]
        #visibility struct #struct_name;

        #[::mycelium_tools::__private::async_trait::async_trait]
        impl ::mycelium_tools::Tool for #struct_name {
            fn spec(&self) -> ::mycelium_tools::ToolSpec {
                ::mycelium_tools::__private::spec(#tool_name_lit, #description, {
                    use ::mycelium_tools::__private::serde_json::{Value, json, Map};
                    #[allow(unused_mut)]
                    let mut properties = Map::new();
                    #[allow(unused_mut)]
                    let mut required: Vec<Value> = Vec::new();
                    #(
                        properties.insert(#param_keys.to_owned(), #param_schemas);
                        if #param_requireds {
                            required.push(Value::String(#param_keys.to_owned()));
                        }
                    )*
                    json!({
                        "type": "object",
                        "properties": properties,
                        "required": required,
                        "additionalProperties": false
                    })
                })
            }

            async fn call(
                &self,
                _context: &::mycelium_tools::ToolContext,
                arguments: ::mycelium_tools::__private::serde_json::Value,
            ) -> Result<::mycelium_tools::ToolOutput, ::mycelium_tools::__private::ToolError> {
                let __tool_name = ::mycelium_tools::__private::tool_name(#tool_name_lit);
                #(#bindings)*
                let __result = #invoke;
                match __result {
                    Ok(value) => #success,
                    Err(error) => Err(::mycelium_tools::__private::ToolError::Execution {
                        tool: __tool_name,
                        message: error.to_string(),
                    }),
                }
            }
        }

        #[doc = #helper_doc]
        #visibility fn #helper_name() -> ::std::sync::Arc<dyn ::mycelium_tools::Tool> {
            ::std::sync::Arc::new(#struct_name)
        }
    })
}

/// 校验工具名（与 `ToolName` 的域一致）。
fn validate_tool_name(lit: &LitStr) -> syn::Result<()> {
    let value = lit.value();
    let len = value.chars().count();
    let valid = (1..=64).contains(&len)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if valid {
        Ok(())
    } else {
        Err(Error::new_spanned(
            lit,
            "工具名必须为 1..=64 个 ASCII 字母 / 数字 / 下划线 / 连字符",
        ))
    }
}

/// 参数类型 → (schema 表达式, 是否必填)。
fn schema_tokens(ty: &Type) -> syn::Result<(TokenStream2, bool)> {
    match ty {
        Type::Reference(reference) => Err(Error::new_spanned(
            reference,
            "#[tool] 参数不支持引用类型，请使用持有类型（如 String）",
        )),
        Type::Path(type_path) => {
            let segment = type_path
                .path
                .segments
                .last()
                .ok_or_else(|| Error::new_spanned(ty, "无法识别的参数类型"))?;
            let name = segment.ident.to_string();
            let inner_types: Vec<&Type> = match &segment.arguments {
                PathArguments::AngleBracketed(args) => args
                    .args
                    .iter()
                    .filter_map(|arg| match arg {
                        GenericArgument::Type(inner) => Some(inner),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            Ok(match name.as_str() {
                "String" | "str" => (quote! { json!({"type": "string"}) }, true),
                "bool" => (quote! { json!({"type": "boolean"}) }, true),
                "f32" | "f64" => (quote! { json!({"type": "number"}) }, true),
                "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64"
                | "u128" | "usize" => (quote! { json!({"type": "integer"}) }, true),
                "Vec" => {
                    let item = inner_types.first().ok_or_else(|| {
                        Error::new_spanned(ty, "Vec 缺少元素类型（当前不支持裸 Vec）")
                    })?;
                    let (item_schema, _) = schema_tokens(item)?;
                    (
                        quote! { json!({"type": "array", "items": #item_schema}) },
                        true,
                    )
                }
                "Option" => {
                    let inner = inner_types.first().ok_or_else(|| {
                        Error::new_spanned(ty, "Option 缺少元素类型（当前不支持裸 Option）")
                    })?;
                    let (inner_schema, _) = schema_tokens(inner)?;
                    (inner_schema, false)
                }
                // 未映射类型：空 schema（宽松接受），文档见 §2.18
                _ => (quote! { json!({}) }, true),
            })
        }
        other => Err(Error::new_spanned(
            other,
            "#[tool] 不支持该参数类型（支持 String / 整数 / f32|f64 / bool / Vec<T> / Option<T>）",
        )),
    }
}

/// 校验返回类型并判定成功结果转换方式。
fn detect_ok_kind(output: &ReturnType) -> syn::Result<OkKind> {
    let ReturnType::Type(_, ty) = output else {
        return Err(Error::new_spanned(
            output,
            "#[tool] 函数必须返回 Result<T, E>",
        ));
    };
    let Type::Path(type_path) = &**ty else {
        return Err(Error::new_spanned(ty, "#[tool] 函数必须返回 Result<T, E>"));
    };
    let Some(segment) = type_path.path.segments.last() else {
        return Err(Error::new_spanned(ty, "#[tool] 函数必须返回 Result<T, E>"));
    };
    if segment.ident != "Result" {
        return Err(Error::new_spanned(ty, "#[tool] 函数必须返回 Result<T, E>"));
    }
    let ok_type = match &segment.arguments {
        PathArguments::AngleBracketed(args) => args.args.iter().find_map(|arg| match arg {
            GenericArgument::Type(inner) => Some(inner),
            _ => None,
        }),
        _ => None,
    };
    let Some(ok_type) = ok_type else {
        return Err(Error::new_spanned(ty, "#[tool] 无法解析 Result 的成功类型"));
    };
    let kind = match ok_type {
        Type::Path(path) => match path.path.segments.last().map(|seg| seg.ident.to_string()) {
            Some(name) if name == "ToolOutput" => OkKind::ToolOutput,
            Some(name) if name == "String" => OkKind::Text,
            _ => OkKind::Serialize,
        },
        _ => OkKind::Serialize,
    };
    Ok(kind)
}

/// 提取文档注释首段（遇到空行停止）。
fn extract_doc_comment(attrs: &[syn::Attribute]) -> String {
    let mut out = String::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        let syn::Meta::NameValue(name_value) = &attr.meta else {
            continue;
        };
        let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(text),
            ..
        }) = &name_value.value
        else {
            continue;
        };
        let line = text.value().trim().to_owned();
        if line.is_empty() {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&line);
    }
    out
}

/// `snake_case` → `PascalCase`。
fn to_pascal_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for part in name.split('_') {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.extend(chars);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 解析属性参数（辅助）。
    fn parse_args(tokens: TokenStream2) -> syn::Result<ToolArgs> {
        syn::parse2(tokens)
    }

    #[test]
    fn parses_attributes() {
        let parsed = parse_args(quote! { name = "custom", description = "描述" }).unwrap();
        assert_eq!(parsed.name.as_ref().unwrap().value(), "custom");
        assert_eq!(parsed.description.as_ref().unwrap().value(), "描述");

        // 未知参数 / 非字符串字面量必须拒绝
        assert!(parse_args(quote! { bogus = "x" }).is_err());
        assert!(parse_args(quote! { name = 1 }).is_err());
    }

    #[test]
    fn expands_valid_function() {
        let function: ItemFn = syn::parse_quote! {
            /// 加法工具。
            async fn add(a: i64, b: i64) -> Result<i64, String> {
                Ok(a + b)
            }
        };
        let tokens = expand_tool(&function, &ToolArgs::default())
            .unwrap()
            .to_string();
        // 生成结构体与帮助函数
        assert!(tokens.contains("struct AddTool"));
        assert!(tokens.contains("fn add_tool"));
        // 文档注释作为描述
        assert!(tokens.contains("加法工具。"));
    }

    #[test]
    fn rejects_invalid_definitions() {
        // 非法工具名（编译期拒绝）
        let function: ItemFn = syn::parse_quote! {
            async fn bad() -> Result<i64, String> { Ok(1) }
        };
        let args = ToolArgs {
            name: Some(LitStr::new("a b", Span::call_site())),
            ..ToolArgs::default()
        };
        assert!(expand_tool(&function, &args).is_err());

        // 引用参数
        let function: ItemFn = syn::parse_quote! {
            async fn bad(path: &str) -> Result<String, String> { Ok(path.to_owned()) }
        };
        assert!(expand_tool(&function, &ToolArgs::default()).is_err());

        // 非 Result 返回类型
        let function: ItemFn = syn::parse_quote! {
            async fn bad() -> String { String::new() }
        };
        assert!(expand_tool(&function, &ToolArgs::default()).is_err());

        // 泛型函数
        let function: ItemFn = syn::parse_quote! {
            async fn bad<T: Send>(value: T) -> Result<String, String> { Ok(String::new()) }
        };
        assert!(expand_tool(&function, &ToolArgs::default()).is_err());

        // 模式参数
        let function: ItemFn = syn::parse_quote! {
            async fn bad((a, b): (i64, i64)) -> Result<String, String> { Ok(String::new()) }
        };
        assert!(expand_tool(&function, &ToolArgs::default()).is_err());

        // mut 参数
        let function: ItemFn = syn::parse_quote! {
            async fn bad(mut value: i64) -> Result<i64, String> { value += 1; Ok(value) }
        };
        assert!(expand_tool(&function, &ToolArgs::default()).is_err());
    }

    #[test]
    fn schema_mapping_covers_supported_types() {
        let function: ItemFn = syn::parse_quote! {
            async fn typed(
                text: String,
                count: u64,
                ratio: f64,
                flag: bool,
                items: Vec<String>,
                note: Option<String>,
            ) -> Result<String, String> {
                Ok(text)
            }
        };
        let tokens = expand_tool(&function, &ToolArgs::default())
            .unwrap()
            .to_string();
        assert!(
            tokens.contains(r#"{"type" : "string"}"#)
                || tokens.contains(r#""type": "string""#)
                || tokens.contains("string")
        );
        assert!(tokens.contains("integer"));
        assert!(tokens.contains("number"));
        assert!(tokens.contains("boolean"));
        assert!(tokens.contains("array"));
    }
}
