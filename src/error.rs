//! Error types for the agent-cli-runner library.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;

/// The result type for agent-cli-runner operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur when spawning or interacting with agent CLIs.
#[derive(Debug)]
pub enum Error {
    /// The CLI binary was not found in PATH.
    BinaryNotFound {
        /// The name of the CLI that was not found.
        cli_name: String,
    },
    /// The required API key environment variable is not set.
    ApiKeyMissing {
        /// The name of the environment variable that should be set.
        env_var: String,
    },
    /// Failed to spawn the CLI process.
    SpawnFailed {
        /// The underlying IO error.
        source: io::Error,
    },
    /// Failed to write to the process stdin.
    StdinWriteFailed {
        /// The underlying IO error.
        source: io::Error,
    },
    /// The CLI process exited with a non-zero status.
    ProcessFailed {
        /// The exit code, if available.
        exit_code: Option<i32>,
        /// Error message from stderr, if available.
        stderr: Option<String>,
    },
    /// Multi-turn sessions are not supported for this CLI.
    MultiTurnNotSupported {
        /// The CLI kind that doesn't support multi-turn.
        cli_kind: String,
    },
    /// Session resume failed because no session ID is available.
    NoSessionId,
    /// The event receiver was dropped or disconnected.
    ReceiverDisconnected,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BinaryNotFound { cli_name } => {
                write!(f, "CLI binary not found in PATH: {cli_name}")
            }
            Self::ApiKeyMissing { env_var } => {
                write!(f, "API key environment variable not set: {env_var}")
            }
            Self::SpawnFailed { source } => {
                write!(f, "Failed to spawn CLI process: {source}")
            }
            Self::StdinWriteFailed { source } => {
                write!(f, "Failed to write to process stdin: {source}")
            }
            Self::ProcessFailed { exit_code, stderr } => {
                let code_str = exit_code
                    .map_or_else(|| "unknown".to_string(), |c| c.to_string());
                let stderr_str = stderr.as_deref().unwrap_or("no stderr captured");
                write!(f, "CLI process failed with exit code {code_str}: {stderr_str}")
            }
            Self::MultiTurnNotSupported { cli_kind } => {
                write!(f, "Multi-turn sessions not supported for {cli_kind}")
            }
            Self::NoSessionId => {
                write!(f, "Cannot resume session: no session ID available")
            }
            Self::ReceiverDisconnected => {
                write!(f, "Event receiver disconnected")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SpawnFailed { source } | Self::StdinWriteFailed { source } => Some(source),
            _ => None,
        }
    }
}

/// Classification of errors that can appear in the event stream.
///
/// Unlike `Error`, which is returned from API functions, `ErrorKind` is used
/// within `AgentEvent::Error` to classify errors that occur during streaming.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorKind {
    /// Output from stderr.
    Stderr,
    /// Non-JSON output that couldn't be parsed.
    UnparsedOutput,
    /// JSON parsing failed.
    JsonParseError,
    /// Debug diagnostic (only emitted when debug mode is enabled).
    Debug,
    /// The CLI process terminated unexpectedly.
    ProcessTerminated,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stderr => write!(f, "stderr"),
            Self::UnparsedOutput => write!(f, "unparsed output"),
            Self::JsonParseError => write!(f, "JSON parse error"),
            Self::Debug => write!(f, "debug"),
            Self::ProcessTerminated => write!(f, "process terminated"),
        }
    }
}
