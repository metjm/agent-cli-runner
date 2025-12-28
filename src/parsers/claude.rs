//! Parser for Claude Code JSON streaming output.
//!
//! Claude Code emits JSONL events with a "type" field indicating the event kind.
//! Known event types include:
//! - "system": System information including session ID
//! - "assistant": Text output with content blocks
//! - "result": Final result with usage statistics

use crate::events::{AgentEvent, ToolCall, ToolResult, Usage};
use serde_json::Value;

/// Parses a Claude Code JSON event into agent events.
pub fn parse(json: &Value) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    let event_type = json.get("type").and_then(Value::as_str).unwrap_or("");
    match event_type {
        "system" => parse_system(json, &mut events),
        "assistant" => parse_assistant(json, &mut events),
        "result" => parse_result(json, &mut events),
        "tool_use" => parse_tool_use(json, &mut events),
        "tool_result" => parse_tool_result(json, &mut events),
        "thinking" => events.push(AgentEvent::Thinking),
        _ => {
            if let Some(text) = extract_text_content(json) {
                events.push(AgentEvent::Text {
                    content: text,
                    is_partial: false,
                });
            }
        }
    }
    events
}

fn parse_system(json: &Value, events: &mut Vec<AgentEvent>) {
    let session_id = json
        .get("session_id")
        .or_else(|| json.get("sessionId"))
        .and_then(Value::as_str)
        .map(String::from);
    events.push(AgentEvent::SessionStarted { session_id });
}

fn parse_assistant(json: &Value, events: &mut Vec<AgentEvent>) {
    if let Some(message) = json.get("message") {
        if let Some(content) = message.get("content") {
            parse_content_blocks(content, events);
        }
    } else if let Some(content) = json.get("content") {
        parse_content_blocks(content, events);
    }
}

fn parse_content_blocks(content: &Value, events: &mut Vec<AgentEvent>) {
    if let Some(blocks) = content.as_array() {
        for block in blocks {
            parse_content_block(block, events);
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
        "tool_use" => {
            if let Some(call) = parse_tool_call_from_block(block) {
                events.push(AgentEvent::ToolCall(call));
            }
        }
        "tool_result" => {
            if let Some(result) = parse_tool_result_from_block(block) {
                events.push(AgentEvent::ToolResult(result));
            }
        }
        _ => {}
    }
}

fn parse_tool_use(json: &Value, events: &mut Vec<AgentEvent>) {
    if let Some(call) = parse_tool_call_from_block(json) {
        events.push(AgentEvent::ToolCall(call));
    }
}

fn parse_tool_result(json: &Value, events: &mut Vec<AgentEvent>) {
    if let Some(result) = parse_tool_result_from_block(json) {
        events.push(AgentEvent::ToolResult(result));
    }
}

fn parse_tool_call_from_block(block: &Value) -> Option<ToolCall> {
    let id = block.get("id").and_then(Value::as_str)?.to_string();
    let name = block.get("name").and_then(Value::as_str)?.to_string();
    let input = block.get("input").cloned().unwrap_or(Value::Null);
    Some(ToolCall { id, name, input })
}

fn parse_tool_result_from_block(block: &Value) -> Option<ToolResult> {
    let tool_call_id = block
        .get("tool_use_id")
        .or_else(|| block.get("tool_call_id"))
        .and_then(Value::as_str)?
        .to_string();
    let output = block
        .get("content")
        .or_else(|| block.get("output"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let is_error = block.get("is_error").and_then(Value::as_bool).unwrap_or(false);
    Some(ToolResult {
        tool_call_id,
        output,
        success: !is_error,
    })
}

#[allow(clippy::cast_possible_truncation)]
fn parse_result(json: &Value, events: &mut Vec<AgentEvent>) {
    if let Some(usage) = parse_usage(json) {
        events.push(AgentEvent::Usage(usage));
    }
    let exit_code = json.get("exit_code").and_then(Value::as_i64).map(|c| c as i32);
    events.push(AgentEvent::SessionCompleted { exit_code });
}

fn parse_usage(json: &Value) -> Option<Usage> {
    let usage = json.get("usage")?;
    let input_tokens = usage.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
    let output_tokens = usage.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
    let cache_read = usage.get("cache_read_input_tokens").and_then(Value::as_u64);
    let cache_write = usage.get("cache_creation_input_tokens").and_then(Value::as_u64);
    Some(Usage {
        input_tokens,
        output_tokens,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
    })
}

fn extract_text_content(json: &Value) -> Option<String> {
    if let Some(text) = json.get("text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(content) = json.get("content").and_then(Value::as_str) {
        return Some(content.to_string());
    }
    None
}
