//! Parser for Gemini CLI JSON streaming output.
//!
//! Gemini CLI emits JSONL events with a `type` field indicating the event kind.
//! Known event types include:
//! - `session_start`: Session initialization
//! - `text`: Text output
//! - `tool_call`: Tool invocation
//! - `tool_result`: Tool execution result
//! - `session_end`: Session completion with usage

use crate::events::{AgentEvent, ToolCall, ToolResult, Usage};
use serde_json::Value;

/// Parses a Gemini CLI JSON event into agent events.
pub fn parse(json: &Value) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    let event_type = json.get("type").and_then(Value::as_str).unwrap_or("");
    match event_type {
        "session_start" | "sessionStart" => parse_session_start(json, &mut events),
        "text" | "content" => parse_text(json, &mut events),
        "tool_call" | "toolCall" | "function_call" => parse_tool_call_event(json, &mut events),
        "tool_result" | "toolResult" | "function_result" => parse_tool_result(json, &mut events),
        "session_end" | "sessionEnd" => parse_session_end(json, &mut events),
        "thinking" => events.push(AgentEvent::Thinking),
        _ => {
            if let Some(text) = extract_text(json) {
                events.push(AgentEvent::Text {
                    content: text,
                    is_partial: false,
                });
            }
        }
    }
    events
}

fn parse_session_start(json: &Value, events: &mut Vec<AgentEvent>) {
    let session_id = json
        .get("session_id")
        .or_else(|| json.get("sessionId"))
        .and_then(Value::as_str)
        .map(String::from);
    events.push(AgentEvent::SessionStarted { session_id });
}

fn parse_text(json: &Value, events: &mut Vec<AgentEvent>) {
    let text = json
        .get("text")
        .or_else(|| json.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let is_partial = json
        .get("partial")
        .or_else(|| json.get("isPartial"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !text.is_empty() {
        events.push(AgentEvent::Text {
            content: text.to_string(),
            is_partial,
        });
    }
}

fn parse_tool_call_event(json: &Value, events: &mut Vec<AgentEvent>) {
    if let Some(call) = parse_tool_call(json) {
        events.push(AgentEvent::ToolCall(call));
    }
}

fn parse_tool_call(json: &Value) -> Option<ToolCall> {
    let id = json
        .get("id")
        .or_else(|| json.get("call_id"))
        .or_else(|| json.get("callId"))
        .and_then(Value::as_str)?
        .to_string();
    let name = json
        .get("name")
        .or_else(|| json.get("function"))
        .or_else(|| json.get("tool"))
        .and_then(Value::as_str)?
        .to_string();
    let input = json
        .get("input")
        .or_else(|| json.get("args"))
        .or_else(|| json.get("arguments"))
        .cloned()
        .unwrap_or(Value::Null);
    Some(ToolCall { id, name, input })
}

fn parse_tool_result(json: &Value, events: &mut Vec<AgentEvent>) {
    let tool_call_id = json
        .get("call_id")
        .or_else(|| json.get("callId"))
        .or_else(|| json.get("tool_call_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let output = json
        .get("output")
        .or_else(|| json.get("result"))
        .or_else(|| json.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let success = json
        .get("success")
        .or_else(|| json.get("ok"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !tool_call_id.is_empty() {
        events.push(AgentEvent::ToolResult(ToolResult {
            tool_call_id,
            output,
            success,
        }));
    }
}

#[allow(clippy::cast_possible_truncation)]
fn parse_session_end(json: &Value, events: &mut Vec<AgentEvent>) {
    if let Some(usage) = parse_usage(json) {
        events.push(AgentEvent::Usage(usage));
    }
    let exit_code = json.get("exit_code").and_then(Value::as_i64).map(|c| c as i32);
    events.push(AgentEvent::SessionCompleted { exit_code });
}

fn parse_usage(json: &Value) -> Option<Usage> {
    let usage = json.get("usage").or_else(|| json.get("tokenUsage"))?;
    let input = usage
        .get("input_tokens")
        .or_else(|| usage.get("inputTokens"))
        .or_else(|| usage.get("promptTokenCount"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output = usage
        .get("output_tokens")
        .or_else(|| usage.get("outputTokens"))
        .or_else(|| usage.get("candidatesTokenCount"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(Usage::new(input, output))
}

fn extract_text(json: &Value) -> Option<String> {
    json.get("text")
        .or_else(|| json.get("content"))
        .or_else(|| json.get("message"))
        .and_then(Value::as_str)
        .map(String::from)
}
