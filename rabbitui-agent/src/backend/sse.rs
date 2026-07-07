//! The Anthropic Messages streaming decoder.
//!
//! Turns the server-sent-events wire (`event:` / `data:` lines) into the app's
//! wire-neutral [`StreamEvent`]s. This is the single most error-prone part of the
//! wire integration — content-block indices must be correlated to tool-use ids,
//! and `stop_reason`/`usage` arrive on `message_delta` but must be emitted with
//! `message_stop` — so it is built and tested in isolation here, ahead of the thin
//! `reqwest` client that will feed it (slice 2 remainder).
//!
//! The decoder is incremental: [`SseDecoder::push`] accepts a UTF-8 chunk (the
//! client buffers partial multi-byte sequences at the byte level before calling)
//! and appends any decoded events to a caller-owned vector, buffering the trailing
//! partial line across calls. It dispatches on each `data:` payload's own `type`
//! field and ignores the `event:` line, so it is robust to unknown event types
//! (`ping`, future additions) — they are simply skipped.

use std::collections::HashMap;

use serde_json::Value;

use super::{BackendError, StopReason, StreamEvent, Usage};

/// An incremental decoder from Anthropic SSE bytes to [`StreamEvent`]s.
#[derive(Debug, Default)]
pub struct SseDecoder {
    /// Bytes received but not yet terminated by a newline.
    buffer: String,
    /// Content-block index → tool-use id, for the blocks that are tool calls.
    tool_blocks: HashMap<u64, String>,
    /// The stop reason carried by `message_delta`, emitted with `message_stop`.
    pending_stop: Option<StopReason>,
    /// The usage accumulated across `message_start` and `message_delta`.
    usage: Usage,
}

impl SseDecoder {
    /// A fresh decoder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one UTF-8 chunk, appending any completed events to `out`.
    ///
    /// A chunk may split a line; the trailing partial is buffered until the next
    /// call completes it.
    pub fn push(&mut self, chunk: &str, out: &mut Vec<Result<StreamEvent, BackendError>>) {
        self.buffer.push_str(chunk);
        while let Some(newline) = self.buffer.find('\n') {
            let line: String = self.buffer.drain(..=newline).collect();
            self.line(line.trim_end_matches(['\r', '\n']), out);
        }
    }

    /// Processes one complete line.
    fn line(&mut self, line: &str, out: &mut Vec<Result<StreamEvent, BackendError>>) {
        // Only `data:` lines carry payloads; `event:` and blank lines are framing.
        let Some(data) = line.strip_prefix("data:") else {
            return;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            return;
        }
        match serde_json::from_str::<Value>(data) {
            Ok(value) => self.dispatch(&value, out),
            Err(error) => out.push(Err(BackendError::Decode(format!(
                "malformed SSE data: {error}"
            )))),
        }
    }

    /// Dispatches one decoded event JSON on its `type`.
    fn dispatch(&mut self, value: &Value, out: &mut Vec<Result<StreamEvent, BackendError>>) {
        match value.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                if let Some(tokens) = value
                    .pointer("/message/usage/input_tokens")
                    .and_then(Value::as_u64)
                {
                    self.usage.input_tokens = tokens;
                }
            }
            Some("content_block_start") => self.content_block_start(value, out),
            Some("content_block_delta") => self.content_block_delta(value, out),
            Some("content_block_stop") => {
                if let Some(id) = index_of(value).and_then(|index| self.tool_blocks.remove(&index)) {
                    out.push(Ok(StreamEvent::ToolUseStop { id }));
                }
            }
            Some("message_delta") => {
                if let Some(reason) = value.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.pending_stop = parse_stop_reason(reason);
                }
                if let Some(tokens) = value
                    .pointer("/usage/output_tokens")
                    .and_then(Value::as_u64)
                {
                    self.usage.output_tokens = tokens;
                }
            }
            Some("message_stop") => out.push(Ok(StreamEvent::MessageDone {
                stop_reason: self.pending_stop.take().unwrap_or(StopReason::EndTurn),
                usage: self.usage,
            })),
            Some("error") => {
                let message = value
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("stream error")
                    .to_string();
                // An in-stream error carries no HTTP status; the client fills the
                // status on transport-level failures instead.
                out.push(Err(BackendError::Api { status: 0, message }));
            }
            _ => {}
        }
    }

    /// Handles `content_block_start`: only tool-use blocks produce an event (text
    /// and thinking blocks carry their content in the following deltas).
    fn content_block_start(
        &mut self,
        value: &Value,
        out: &mut Vec<Result<StreamEvent, BackendError>>,
    ) {
        let block = &value["content_block"];
        if block.get("type").and_then(Value::as_str) != Some("tool_use") {
            return;
        }
        let (Some(index), Some(id), name) = (
            index_of(value),
            block.get("id").and_then(Value::as_str),
            block
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ) else {
            return;
        };
        self.tool_blocks.insert(index, id.to_string());
        out.push(Ok(StreamEvent::ToolUseStart {
            id: id.to_string(),
            name: name.to_string(),
        }));
    }

    /// Handles `content_block_delta`: text, thinking, or streamed tool input.
    fn content_block_delta(
        &mut self,
        value: &Value,
        out: &mut Vec<Result<StreamEvent, BackendError>>,
    ) {
        let delta = &value["delta"];
        match delta.get("type").and_then(Value::as_str) {
            Some("text_delta") => {
                out.push(Ok(StreamEvent::TextDelta {
                    text: string_field(delta, "text"),
                }));
            }
            Some("thinking_delta") => {
                out.push(Ok(StreamEvent::ThinkingDelta {
                    text: string_field(delta, "thinking"),
                }));
            }
            Some("input_json_delta") => {
                if let Some(id) = index_of(value).and_then(|index| self.tool_blocks.get(&index)) {
                    out.push(Ok(StreamEvent::ToolUseInputDelta {
                        id: id.clone(),
                        json: string_field(delta, "partial_json"),
                    }));
                }
            }
            _ => {}
        }
    }
}

/// The `index` field of an event, if present.
fn index_of(value: &Value) -> Option<u64> {
    value.get("index").and_then(Value::as_u64)
}

/// A string field's value, or empty.
fn string_field(value: &Value, field: &str) -> String {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Maps a wire `stop_reason` to [`StopReason`]; unknown reasons decode as `None`
/// (the caller defaults to `EndTurn`).
fn parse_stop_reason(reason: &str) -> Option<StopReason> {
    match reason {
        "end_turn" => Some(StopReason::EndTurn),
        "max_tokens" => Some(StopReason::MaxTokens),
        "tool_use" => Some(StopReason::ToolUse),
        "refusal" => Some(StopReason::Refusal),
        "pause_turn" => Some(StopReason::PauseTurn),
        "stop_sequence" => Some(StopReason::StopSequence),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decodes a whole SSE transcript in one push.
    fn decode(transcript: &str) -> Vec<Result<StreamEvent, BackendError>> {
        let mut decoder = SseDecoder::new();
        let mut out = Vec::new();
        decoder.push(transcript, &mut out);
        out
    }

    /// Unwraps the `Ok` events, panicking on any error.
    fn events(transcript: &str) -> Vec<StreamEvent> {
        decode(transcript)
            .into_iter()
            .map(|result| result.expect("no decode error"))
            .collect()
    }

    const TEXT_TURN: &str = "\
event: message_start
data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":12}}}

event: content_block_start
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}

event: content_block_delta
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}

event: content_block_delta
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\", world\"}}

event: content_block_stop
data: {\"type\":\"content_block_stop\",\"index\":0}

event: message_delta
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}

event: message_stop
data: {\"type\":\"message_stop\"}

";

    #[test]
    fn decodes_a_text_turn_with_usage_and_stop_reason() {
        let events = events(TEXT_TURN);
        assert_eq!(events, vec![
            StreamEvent::TextDelta {
                text: "Hello".to_string()
            },
            StreamEvent::TextDelta {
                text: ", world".to_string()
            },
            StreamEvent::MessageDone {
                stop_reason: StopReason::EndTurn,
                usage: Usage {
                    input_tokens: 12,
                    output_tokens: 5,
                },
            },
        ]);
    }

    #[test]
    fn decodes_thinking_deltas() {
        let transcript = "\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"hmm\"}}
data: {\"type\":\"message_stop\"}
";
        assert_eq!(events(transcript), vec![
            StreamEvent::ThinkingDelta {
                text: "hmm".to_string()
            },
            StreamEvent::MessageDone {
                stop_reason: StopReason::EndTurn,
                usage: Usage::default(),
            },
        ]);
    }

    #[test]
    fn correlates_tool_use_blocks_by_index() {
        let transcript = "\
data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_9\",\"name\":\"read_file\"}}
data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\"}}
data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"a.rs\\\"}\"}}
data: {\"type\":\"content_block_stop\",\"index\":1}
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":3}}
data: {\"type\":\"message_stop\"}
";
        assert_eq!(events(transcript), vec![
            StreamEvent::ToolUseStart {
                id: "toolu_9".to_string(),
                name: "read_file".to_string(),
            },
            StreamEvent::ToolUseInputDelta {
                id: "toolu_9".to_string(),
                json: "{\"path\":".to_string(),
            },
            StreamEvent::ToolUseInputDelta {
                id: "toolu_9".to_string(),
                json: "\"a.rs\"}".to_string(),
            },
            StreamEvent::ToolUseStop {
                id: "toolu_9".to_string(),
            },
            StreamEvent::MessageDone {
                stop_reason: StopReason::ToolUse,
                usage: Usage {
                    input_tokens: 0,
                    output_tokens: 3,
                },
            },
        ]);
    }

    #[test]
    fn buffers_lines_split_across_chunks() {
        let mut decoder = SseDecoder::new();
        let mut out = Vec::new();
        // The JSON payload is split mid-token across three pushes.
        decoder.push("data: {\"type\":\"content_bl", &mut out);
        assert!(out.is_empty(), "no complete line yet");
        decoder.push("ock_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",", &mut out);
        assert!(out.is_empty(), "still no newline");
        decoder.push("\"text\":\"ok\"}}\n", &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].as_ref().unwrap(), &StreamEvent::TextDelta {
            text: "ok".to_string()
        });
    }

    #[test]
    fn ignores_ping_and_unknown_events() {
        let transcript = "\
event: ping
data: {\"type\":\"ping\"}
data: {\"type\":\"some_future_event\",\"index\":0}
data: {\"type\":\"message_stop\"}
";
        // Only the message_stop produces an event.
        assert_eq!(events(transcript).len(), 1);
    }

    #[test]
    fn an_in_stream_error_becomes_an_api_error() {
        let transcript = "\
data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"overloaded\"}}
";
        let decoded = decode(transcript);
        assert_eq!(decoded.len(), 1);
        assert!(matches!(
            &decoded[0],
            Err(BackendError::Api { message, .. }) if message == "overloaded"
        ));
    }

    #[test]
    fn an_unknown_stop_reason_defaults_to_end_turn() {
        let transcript = "\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"future_reason\"},\"usage\":{}}
data: {\"type\":\"message_stop\"}
";
        assert_eq!(events(transcript), vec![StreamEvent::MessageDone {
            stop_reason: StopReason::EndTurn,
            usage: Usage::default(),
        }]);
    }
}
