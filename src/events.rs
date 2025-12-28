//! Unified event model for agent CLI output streams.

use crate::error::ErrorKind;
use serde::{Deserialize, Serialize};

/// An event emitted by an agent CLI during execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentEvent {
    /// Text output from the agent.
    Text {
        /// The text content.
        content: String,
        /// Whether this is a partial (streaming) chunk.
        is_partial: bool,
    },
    /// The agent is invoking a tool.
    ToolCall(ToolCall),
    /// The result of a tool invocation.
    ToolResult(ToolResult),
    /// Token usage statistics (not guaranteed for all CLIs).
    Usage(Usage),
    /// The agent session has started.
    SessionStarted {
        /// The session ID, if available.
        session_id: Option<String>,
    },
    /// The agent session has completed.
    SessionCompleted {
        /// The exit code of the process.
        exit_code: Option<i32>,
    },
    /// An error occurred during streaming.
    Error {
        /// The kind of error.
        kind: ErrorKind,
        /// The error message.
        message: String,
    },
    /// The agent is thinking/processing (no output yet).
    Thinking,
}

/// A tool call initiated by the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique identifier for this tool call.
    pub id: String,
    /// The name of the tool being called.
    pub name: String,
    /// The input arguments as a JSON value.
    pub input: serde_json::Value,
}

/// The result of a tool execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResult {
    /// The ID of the tool call this result corresponds to.
    pub tool_call_id: String,
    /// The output of the tool execution.
    pub output: String,
    /// Whether the tool execution was successful.
    pub success: bool,
}

/// Token usage statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Number of input tokens consumed.
    pub input_tokens: u64,
    /// Number of output tokens generated.
    pub output_tokens: u64,
    /// Number of cache read tokens (if applicable).
    pub cache_read_tokens: Option<u64>,
    /// Number of cache write tokens (if applicable).
    pub cache_write_tokens: Option<u64>,
}

impl Usage {
    /// Creates a new Usage with the given token counts.
    #[must_use]
    pub const fn new(input_tokens: u64, output_tokens: u64) -> Self {
        Self {
            input_tokens,
            output_tokens,
            cache_read_tokens: None,
            cache_write_tokens: None,
        }
    }

    /// Returns the total number of tokens (input + output).
    #[must_use]
    pub const fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}
