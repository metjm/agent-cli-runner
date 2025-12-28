//! Parser for Codex CLI JSON streaming output.
//!
//! Codex CLI emits JSONL events with an `event` field indicating the event kind.
//! Known event types include:
//! - `session_start`: Session initialization
//! - `message`: Agent messages (text, tool calls, etc.)
//! - `exec_result`: Tool execution results
//! - `session_end`: Session completion

use crate::events::{AgentEvent, ToolCall, ToolResult, Usage};
use serde_json::Value;

/// Parses a Codex CLI JSON event into agent events.
pub fn parse(json: &Value) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    let event_type = json.get("event").and_then(Value::as_str).unwrap_or("");
    match event_type {
        "session_start" => parse_session_start(json, &mut events),
        "message" => parse_message(json, &mut events),
        "exec_result" | "tool_result" => parse_exec_result(json, &mut events),
        "session_end" => parse_session_end(json, &mut events),
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

fn parse_message(json: &Value, events: &mut Vec<AgentEvent>) {
    if let Some(message) = json.get("message").or(Some(json)) {
        let role = message.get("role").and_then(Value::as_str).unwrap_or("");
        if role == "assistant" {
            parse_assistant_message(message, events);
        }
    }
}

fn parse_assistant_message(message: &Value, events: &mut Vec<AgentEvent>) {
    if let Some(content) = message.get("content") {
        if let Some(text) = content.as_str() {
            events.push(AgentEvent::Text {
                content: text.to_string(),
                is_partial: false,
            });
        } else if let Some(blocks) = content.as_array() {
            for block in blocks {
                parse_content_block(block, events);
            }
        }
    }
}

fn parse_content_block(block: &Value, events: &mut Vec<AgentEvent>) {
    let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
    match block_type {
        "text" => {
            if let Some(text) = block.get("text").and_then(Value::as_str) {
                events.push(AgentEvent::Text {
                    content: text.to_string(),
                    is_partial: false,
                });
            }
        }
        "function_call" | "tool_use" => {
            if let Some(call) = parse_tool_call(block) {
                events.push(AgentEvent::ToolCall(call));
            }
        }
        _ => {}
    }
}

fn parse_tool_call(block: &Value) -> Option<ToolCall> {
    let id = block
        .get("id")
        .or_else(|| block.get("call_id"))
        .and_then(Value::as_str)?
        .to_string();
    let name = block
        .get("name")
        .or_else(|| block.get("function"))
        .and_then(Value::as_str)?
        .to_string();
    let input = block
        .get("input")
        .or_else(|| block.get("arguments"))
        .cloned()
        .unwrap_or_else(|| {
            block
                .get("arguments")
                .and_then(Value::as_str)
                .map_or(Value::Null, |args_str| {
                    serde_json::from_str(args_str).unwrap_or(Value::Null)
                })
        });
    Some(ToolCall { id, name, input })
}

fn parse_exec_result(json: &Value, events: &mut Vec<AgentEvent>) {
    let tool_call_id = json
        .get("call_id")
        .or_else(|| json.get("tool_call_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let output = json
        .get("output")
        .or_else(|| json.get("result"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let exit_code = json.get("exit_code").and_then(Value::as_i64);
    let success = exit_code.is_none_or(|c| c == 0);
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
    let usage = json.get("usage")?;
    let input = usage
        .get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output = usage
        .get("output_tokens")
        .or_else(|| usage.get("completion_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(Usage::new(input, output))
}

fn extract_text(json: &Value) -> Option<String> {
    json.get("text")
        .or_else(|| json.get("content"))
        .and_then(Value::as_str)
        .map(String::from)
}
