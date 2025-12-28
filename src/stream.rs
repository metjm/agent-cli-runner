//! Stream reading and parsing for agent CLI output.

use crate::config::AgentKind;
use crate::error::ErrorKind;
use crate::events::AgentEvent;
use crate::parsers;
use crate::process::SyncSenderWrapper;
use std::io::{BufRead, BufReader, Read};

/// Reads and parses the stdout stream from an agent CLI.
pub struct StreamReader<R: Read> {
    reader: BufReader<R>,
    kind: AgentKind,
    debug: bool,
}

impl<R: Read> StreamReader<R> {
    /// Creates a new stream reader.
    pub fn new(reader: R, kind: AgentKind, debug: bool) -> Self {
        Self {
            reader: BufReader::new(reader),
            kind,
            debug,
        }
    }

    /// Reads the stream and sends events to the channel.
    pub fn read_to_channel(mut self, sender: &SyncSenderWrapper) {
        let mut line = String::new();
        loop {
            line.clear();
            match self.reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    self.parse_and_send(trimmed, sender);
                }
                Err(e) => {
                    if self.debug {
                        let _ = sender.send(AgentEvent::Error {
                            kind: ErrorKind::Debug,
                            message: format!("Read error: {e}"),
                        });
                    }
                    break;
                }
            }
        }
    }

    fn parse_and_send(&self, line: &str, sender: &SyncSenderWrapper) {
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(json) => {
                let events = self.parse_json(&json);
                for event in events {
                    if sender.send(event).is_err() {
                        return;
                    }
                }
            }
            Err(e) => {
                if self.debug {
                    let _ = sender.send(AgentEvent::Error {
                        kind: ErrorKind::Debug,
                        message: format!("JSON parse debug: {e}"),
                    });
                }
                let _ = sender.send(AgentEvent::Error {
                    kind: ErrorKind::UnparsedOutput,
                    message: line.to_string(),
                });
            }
        }
    }

    fn parse_json(&self, json: &serde_json::Value) -> Vec<AgentEvent> {
        match self.kind {
            AgentKind::Claude => parsers::claude::parse(json),
            AgentKind::Codex => parsers::codex::parse(json),
            AgentKind::Gemini => parsers::gemini::parse(json),
        }
    }
}

/// Reads stderr and sends error events to the channel.
pub fn read_stderr<S: Read>(reader: S, sender: &SyncSenderWrapper) {
    let buf_reader = BufReader::new(reader);
    for line in buf_reader.lines() {
        match line {
            Ok(text) if !text.trim().is_empty() => {
                if sender
                    .send(AgentEvent::Error {
                        kind: ErrorKind::Stderr,
                        message: text,
                    })
                    .is_err()
                {
                    return;
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
}
