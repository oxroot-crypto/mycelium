//! SSE 解析与增量映射。
//!
//! 契约（见 `docs/spec/core.md` §2.15）：
//! - `Usage`（若有）先于 `Finished`；
//! - `Finished` 恰一条且为最后一项；
//! - 未收到 `finish_reason` 与 `[DONE]` 即中断 → [`ModelError::Transport`]。

use std::collections::VecDeque;
use std::marker::PhantomData;
use std::pin::Pin;

use futures_util::{Stream, StreamExt, stream};
use mycelium_core::error::ModelError;
use mycelium_core::ids::{ToolCallId, ToolName};
use mycelium_core::response::StopReason;
use mycelium_model::{ModelDelta, ModelStream};

/// 把 HTTP 字节流解析为 [`ModelStream`]。
pub(crate) fn parse_sse_stream<S, B>(bytes: S) -> ModelStream
where
    S: Stream<Item = Result<B, reqwest::Error>> + Send + 'static,
    B: AsRef<[u8]> + Send + 'static,
{
    Box::pin(stream::unfold(
        SseState::<S, B>::new(bytes),
        |mut state| async move { state.next_delta().await.map(|item| (item, state)) },
    ))
}

/// SSE 解析状态机。
struct SseState<S, B> {
    upstream: Pin<Box<S>>,
    buffer: String,
    pending: VecDeque<Result<ModelDelta, ModelError>>,
    stop_reason: Option<StopReason>,
    saw_done: bool,
    upstream_done: bool,
    finished: bool,
    marker: PhantomData<fn() -> B>,
}

impl<S, B> SseState<S, B>
where
    S: Stream<Item = Result<B, reqwest::Error>>,
    B: AsRef<[u8]>,
{
    /// 创建状态机。
    fn new(upstream: S) -> Self {
        Self {
            upstream: Box::pin(upstream),
            buffer: String::new(),
            pending: VecDeque::new(),
            stop_reason: None,
            saw_done: false,
            upstream_done: false,
            finished: false,
            marker: PhantomData,
        }
    }

    /// 产出下一个增量（内部驱动上游与解析）。
    async fn next_delta(&mut self) -> Option<Result<ModelDelta, ModelError>> {
        loop {
            if let Some(item) = self.pending.pop_front() {
                return Some(item);
            }
            if self.finished {
                return None;
            }
            if self.upstream_done {
                return Some(self.finalize());
            }

            match self.upstream.next().await {
                None => {
                    if !self.buffer.trim().is_empty() {
                        let rest = std::mem::take(&mut self.buffer);
                        self.handle_event(&rest);
                    }
                    self.upstream_done = true;
                }
                Some(Ok(chunk)) => {
                    let text = String::from_utf8_lossy(chunk.as_ref());
                    self.buffer.push_str(&text);
                    // 统一换行符：兼容 CRLF 与 LF 两种 SSE 分隔。
                    if self.buffer.contains('\r') {
                        self.buffer = self.buffer.replace("\r\n", "\n");
                    }
                    self.drain_buffer();
                }
                Some(Err(error)) => {
                    self.finished = true;
                    return Some(Err(ModelError::Transport {
                        message: error.to_string(),
                    }));
                }
            }
        }
    }

    /// 处理缓冲中的完整 SSE 事件（以空行分隔）。
    fn drain_buffer(&mut self) {
        while let Some(index) = self.buffer.find("\n\n") {
            let event: String = self.buffer.drain(..index + 2).collect();
            self.handle_event(&event);
        }
    }

    /// 处理单个 SSE 事件块。
    fn handle_event(&mut self, event: &str) {
        for line in event.lines() {
            let line = line.trim_end_matches('\r');
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                self.saw_done = true;
                continue;
            }
            if data.is_empty() {
                continue;
            }
            match map_chunk(data, &mut self.stop_reason) {
                Ok(deltas) => self.pending.extend(deltas.into_iter().map(Ok)),
                Err(error) => self.pending.push_back(Err(error)),
            }
        }
    }

    /// 上游结束后产出最终结果（`Finished` 或中断错误）。
    fn finalize(&mut self) -> Result<ModelDelta, ModelError> {
        self.finished = true;
        match self.stop_reason {
            Some(stop_reason) => Ok(ModelDelta::Finished { stop_reason }),
            None if self.saw_done => Ok(ModelDelta::Finished {
                stop_reason: StopReason::Other,
            }),
            None => Err(ModelError::Transport {
                message: "响应流在完成前中断（未收到 finish_reason 与 [DONE]）".to_owned(),
            }),
        }
    }
}

/// 单个 `data:` 载荷 → 增量序列。
fn map_chunk(
    payload: &str,
    stop_reason: &mut Option<StopReason>,
) -> Result<Vec<ModelDelta>, ModelError> {
    let value: serde_json::Value = serde_json::from_str(payload)
        .map_err(|error| invalid(format!("SSE data 不是合法 JSON：{error}")))?;
    let mut deltas = Vec::new();

    if let Some(choice) = value.get("choices").and_then(|choices| choices.get(0)) {
        let delta = choice.get("delta");
        // 思考（推理）增量：DeepSeek `delta.reasoning_content`
        if let Some(reasoning) = delta
            .and_then(|delta| delta.get("reasoning_content"))
            .and_then(|content| content.as_str())
            && !reasoning.is_empty()
        {
            deltas.push(ModelDelta::ThinkingDelta {
                text: reasoning.to_owned(),
            });
        }
        if let Some(content) = delta
            .and_then(|delta| delta.get("content"))
            .and_then(|content| content.as_str())
            && !content.is_empty()
        {
            deltas.push(ModelDelta::TextDelta {
                text: content.to_owned(),
            });
        }
        if let Some(tool_calls) = delta
            .and_then(|delta| delta.get("tool_calls"))
            .and_then(|calls| calls.as_array())
        {
            for call in tool_calls {
                let index = call
                    .get("index")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| invalid("tool_calls 缺少合法 index"))?;
                let id = call
                    .get("id")
                    .and_then(|value| value.as_str())
                    .map(|raw| {
                        ToolCallId::new(raw)
                            .map_err(|error| invalid(format!("非法工具调用 ID：{error}")))
                    })
                    .transpose()?;
                let name = call
                    .get("function")
                    .and_then(|function| function.get("name"))
                    .and_then(|value| value.as_str())
                    .map(|raw| {
                        ToolName::new(raw).map_err(|error| invalid(format!("非法工具名：{error}")))
                    })
                    .transpose()?;
                let arguments = call
                    .get("function")
                    .and_then(|function| function.get("arguments"))
                    .and_then(|value| value.as_str())
                    .map(str::to_owned);
                deltas.push(ModelDelta::ToolCallDelta {
                    index,
                    id,
                    name,
                    arguments_json_delta: arguments,
                });
            }
        }
        if let Some(reason) = choice.get("finish_reason").and_then(|value| value.as_str()) {
            *stop_reason = Some(map_finish_reason(reason));
        }
    }

    if let Some(usage) = value.get("usage").filter(|usage| !usage.is_null()) {
        let input_tokens = usage
            .get("prompt_tokens")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let output_tokens = usage
            .get("completion_tokens")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let reasoning_tokens = usage
            .get("completion_tokens_details")
            .and_then(|details| details.get("reasoning_tokens"))
            .and_then(serde_json::Value::as_u64);
        deltas.push(ModelDelta::Usage {
            input_tokens,
            output_tokens,
            reasoning_tokens,
        });
    }

    Ok(deltas)
}

/// OpenAI `finish_reason` → [`StopReason`]。
fn map_finish_reason(reason: &str) -> StopReason {
    match reason {
        "stop" => StopReason::EndTurn,
        "tool_calls" | "function_call" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        _ => StopReason::Other,
    }
}

/// 构造 `InvalidResponse` 错误。
fn invalid(detail: impl Into<String>) -> ModelError {
    ModelError::InvalidResponse {
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;

    use super::*;

    /// 构造内存中的字节流。
    fn chunk_stream(
        chunks: Vec<&'static str>,
    ) -> impl Stream<Item = Result<Vec<u8>, reqwest::Error>> + Send + 'static {
        stream::iter(
            chunks
                .into_iter()
                .map(|chunk| Ok(chunk.as_bytes().to_vec())),
        )
    }

    /// 收集全部增量（断言无错误）。
    async fn collect_deltas(chunks: Vec<&'static str>) -> Vec<ModelDelta> {
        parse_sse_stream(chunk_stream(chunks))
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .expect("不应出现错误")
    }

    #[tokio::test]
    async fn sse_parser_handles_split_chunks() {
        let chunks = vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel",
            "lo\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            ": 注释行应被忽略\n\ndata: [DONE]\n\n",
        ];
        let deltas = collect_deltas(chunks).await;
        assert_eq!(
            deltas,
            vec![
                ModelDelta::TextDelta {
                    text: "Hello".to_owned()
                },
                ModelDelta::Finished {
                    stop_reason: StopReason::EndTurn
                },
            ]
        );
    }

    #[tokio::test]
    async fn sse_parser_handles_crlf() {
        let chunks = vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"a\"},\"finish_reason\":null}]}\r\n\r\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\r\n\r\ndata: [DONE]\r\n\r\n",
        ];
        let deltas = collect_deltas(chunks).await;
        assert_eq!(
            deltas,
            vec![
                ModelDelta::TextDelta {
                    text: "a".to_owned()
                },
                ModelDelta::Finished {
                    stop_reason: StopReason::EndTurn
                },
            ]
        );
    }

    #[tokio::test]
    async fn maps_chunks_to_deltas() {
        let chunks = vec![
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"echo\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"a\\\":1}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":7}}\n\n",
            "data: [DONE]\n\n",
        ];
        let deltas = collect_deltas(chunks).await;
        assert_eq!(
            deltas,
            vec![
                ModelDelta::ToolCallDelta {
                    index: 0,
                    id: Some(ToolCallId::new("call_1").unwrap()),
                    name: Some(ToolName::new("echo").unwrap()),
                    arguments_json_delta: Some(String::new()),
                },
                ModelDelta::ToolCallDelta {
                    index: 0,
                    id: None,
                    name: None,
                    arguments_json_delta: Some("{\"a\":1}".to_owned()),
                },
                ModelDelta::Usage {
                    input_tokens: 11,
                    output_tokens: 7,
                    reasoning_tokens: None,
                },
                ModelDelta::Finished {
                    stop_reason: StopReason::ToolUse,
                },
            ]
        );
    }

    #[tokio::test]
    async fn maps_reasoning_content_and_tokens() {
        let chunks = vec![
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"先想\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"一下\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"答案\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":20,\"completion_tokens_details\":{\"reasoning_tokens\":15}}}\n\n",
            "data: [DONE]\n\n",
        ];
        let deltas = collect_deltas(chunks).await;
        assert_eq!(
            deltas,
            vec![
                ModelDelta::ThinkingDelta {
                    text: "先想".to_owned()
                },
                ModelDelta::ThinkingDelta {
                    text: "一下".to_owned()
                },
                ModelDelta::TextDelta {
                    text: "答案".to_owned()
                },
                ModelDelta::Usage {
                    input_tokens: 10,
                    output_tokens: 20,
                    reasoning_tokens: Some(15),
                },
                ModelDelta::Finished {
                    stop_reason: StopReason::EndTurn,
                },
            ]
        );
    }

    #[tokio::test]
    async fn truncated_stream_is_transport_error() {
        let chunks = vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n",
        ];
        let collected: Vec<Result<ModelDelta, ModelError>> = parse_sse_stream(chunk_stream(chunks))
            .collect::<Vec<_>>()
            .await;
        assert_eq!(collected.len(), 2);
        assert!(matches!(collected[0], Ok(ModelDelta::TextDelta { .. })));
        assert!(matches!(collected[1], Err(ModelError::Transport { .. })));
    }

    #[tokio::test]
    async fn done_without_finish_reason_maps_to_other() {
        let chunks = vec!["data: [DONE]\n\n"];
        let deltas = collect_deltas(chunks).await;
        assert_eq!(
            deltas,
            vec![ModelDelta::Finished {
                stop_reason: StopReason::Other
            }]
        );
    }
}
