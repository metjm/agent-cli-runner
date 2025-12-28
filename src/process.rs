//! Process spawning and management for agent CLIs.

use crate::config::{AgentConfig, AgentKind};
use crate::error::{Error, Result};
use crate::events::AgentEvent;
use crate::stream::{read_stderr, StreamReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread;

/// Handle to a running CLI process.
pub struct ProcessHandle {
    child: Option<Child>,
    stdout_thread: Option<thread::JoinHandle<()>>,
    stderr_thread: Option<thread::JoinHandle<()>>,
}

impl ProcessHandle {
    /// Spawns a new CLI process with the given configuration and prompt.
    pub fn spawn(config: &AgentConfig, prompt: &str) -> Result<(Self, Receiver<AgentEvent>)> {
        let mut cmd = Self::build_command(config, prompt);
        let mut child = cmd.spawn().map_err(|e| Error::SpawnFailed { source: e })?;
        let buffer_size = config.channel_buffer_size;
        let (sender, receiver) = if buffer_size == 0 {
            let (tx, rx) = std::sync::mpsc::channel();
            (SyncSenderWrapper::Unbounded(tx), rx)
        } else {
            let (tx, rx) = sync_channel(buffer_size);
            (SyncSenderWrapper::Bounded(tx), rx)
        };
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let kind = config.kind;
        let debug = config.debug;
        let stdout_sender = sender.clone();
        let stdout_thread = stdout.map(|out| {
            thread::spawn(move || {
                StreamReader::new(out, kind, debug).read_to_channel(&stdout_sender);
            })
        });
        let stderr_sender = sender;
        let stderr_thread = stderr.map(|err| {
            thread::spawn(move || {
                read_stderr(err, &stderr_sender);
            })
        });
        let handle = Self {
            child: Some(child),
            stdout_thread,
            stderr_thread,
        };
        Ok((handle, receiver))
    }

    fn build_command(config: &AgentConfig, prompt: &str) -> Command {
        match config.kind {
            AgentKind::Claude => Self::build_claude_command(config, prompt),
            AgentKind::Codex => Self::build_codex_command(config, prompt),
            AgentKind::Gemini => Self::build_gemini_command(config, prompt),
        }
    }

    fn build_claude_command(config: &AgentConfig, prompt: &str) -> Command {
        let mut cmd = Command::new("claude");
        cmd.arg("--print");
        cmd.arg("--output-format").arg("stream-json");
        if config.skip_permissions {
            cmd.arg("--dangerously-skip-permissions");
        }
        if let Some(ref model) = config.model {
            cmd.arg("--model").arg(model);
        }
        if let Some(ref session_id) = config.session_id {
            cmd.arg("--resume").arg(session_id);
        }
        cmd.arg(prompt);
        if let Some(ref dir) = config.working_dir {
            cmd.current_dir(dir);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd
    }

    fn build_codex_command(config: &AgentConfig, prompt: &str) -> Command {
        let mut cmd = Command::new("codex");
        cmd.arg("exec");
        cmd.arg("--json");
        if config.skip_permissions {
            cmd.arg("--dangerously-bypass-approvals-and-sandbox");
        }
        if let Some(ref model) = config.model {
            cmd.arg("--model").arg(model);
        }
        cmd.arg(prompt);
        if let Some(ref dir) = config.working_dir {
            cmd.current_dir(dir);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd
    }

    fn build_gemini_command(config: &AgentConfig, prompt: &str) -> Command {
        let mut cmd = Command::new("gemini");
        cmd.arg("-o").arg("stream-json");
        if config.skip_permissions {
            cmd.arg("--yolo");
        }
        if let Some(ref model) = config.model {
            cmd.arg("--model").arg(model);
        }
        if let Some(ref session_id) = config.session_id {
            cmd.arg("--resume").arg(session_id);
        }
        cmd.arg(prompt);
        if let Some(ref dir) = config.working_dir {
            cmd.current_dir(dir);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd
    }

    /// Waits for the process to complete and returns the exit code.
    #[allow(dead_code)]
    pub fn wait(&mut self) -> Option<i32> {
        self.child
            .as_mut()
            .and_then(|child| child.wait().ok())
            .and_then(|s| s.code())
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(handle) = self.stdout_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_thread.take() {
            let _ = handle.join();
        }
    }
}

/// Wrapper to support both bounded and unbounded channels.
#[derive(Clone)]
pub enum SyncSenderWrapper {
    /// Bounded sync channel.
    Bounded(SyncSender<AgentEvent>),
    /// Unbounded channel.
    Unbounded(std::sync::mpsc::Sender<AgentEvent>),
}

impl SyncSenderWrapper {
    /// Sends an event through the channel.
    pub fn send(&self, event: AgentEvent) -> std::result::Result<(), AgentEvent> {
        match self {
            Self::Bounded(tx) => tx.send(event).map_err(|e| e.0),
            Self::Unbounded(tx) => tx.send(event).map_err(|e| e.0),
        }
    }
}
