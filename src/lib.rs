//! # agent-cli-runner
//!
//! A Rust library for spawning and interacting with Codex, Gemini CLI, and Claude Code,
//! parsing their output into a unified event stream.
//!
//! ## Features
//!
//! - Unified event model for text, tool calls/results, token usage, and status
//! - Support for Claude Code, Codex CLI, and Gemini CLI
//! - Per-turn session management with resume capabilities
//! - Minimal dependencies (`serde`, `serde_json` only)
//!
//! ## Example
//!
//! ```no_run
//! use agent_cli_runner::{AgentConfig, AgentKind, AgentSession};
//!
//! let config = AgentConfig::new(AgentKind::Claude);
//! let mut session = AgentSession::spawn(config, "Hello, world!").unwrap();
//!
//! for event in session.events().unwrap() {
//!     println!("{:?}", event);
//! }
//! ```

#![deny(missing_docs)]
#![deny(clippy::all)]

mod config;
mod error;
mod events;
mod parsers;
mod process;
mod session;
mod stream;

pub use config::{AgentConfig, AgentKind};
pub use error::{Error, ErrorKind, Result};
pub use events::{AgentEvent, ToolCall, ToolResult, Usage};
pub use session::AgentSession;
