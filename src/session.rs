//! Agent session management.

use crate::config::{AgentConfig, AgentKind};
use crate::error::{Error, Result};
use crate::events::AgentEvent;
use crate::process::ProcessHandle;
use std::sync::mpsc::Receiver;

/// A session with an agent CLI.
///
/// The session manages the lifecycle of the CLI process and provides
/// access to the event stream.
pub struct AgentSession {
    config: AgentConfig,
    process: Option<ProcessHandle>,
    receiver: Option<Receiver<AgentEvent>>,
    session_id: Option<String>,
}

impl AgentSession {
    /// Spawns a new agent session with the given prompt.
    ///
    /// # Errors
    ///
    /// Returns an error if the CLI binary is not found, the API key is missing,
    /// or the process fails to spawn.
    pub fn spawn(config: AgentConfig, prompt: &str) -> Result<Self> {
        Self::validate_environment(&config)?;
        let (process, receiver) = ProcessHandle::spawn(&config, prompt)?;
        Ok(Self {
            config,
            process: Some(process),
            receiver: Some(receiver),
            session_id: None,
        })
    }

    /// Returns an iterator over events from the agent.
    ///
    /// This consumes the receiver, so it can only be called once.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiver has already been consumed.
    pub fn events(&mut self) -> Result<EventIterator<'_>> {
        let receiver = self.receiver.take().ok_or(Error::ReceiverDisconnected)?;
        Ok(EventIterator {
            receiver,
            session: self,
        })
    }

    /// Sends a follow-up message to continue the conversation.
    ///
    /// This spawns a new process with the resume flag and session ID.
    ///
    /// # Errors
    ///
    /// Returns an error if multi-turn is not supported, no session ID is
    /// available, or the process fails to spawn.
    pub fn send_input(&mut self, prompt: &str) -> Result<()> {
        let session_id = self.session_id.clone().ok_or(Error::NoSessionId)?;
        let config = AgentConfig {
            session_id: Some(session_id),
            ..self.config.clone()
        };
        Self::validate_environment(&config)?;
        let (process, receiver) = ProcessHandle::spawn(&config, prompt)?;
        self.process = Some(process);
        self.receiver = Some(receiver);
        Ok(())
    }

    /// Returns the session ID if available.
    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Returns the agent kind for this session.
    #[must_use]
    pub const fn kind(&self) -> AgentKind {
        self.config.kind
    }

    /// Updates the session ID (called internally when discovered from events).
    pub(crate) fn set_session_id(&mut self, id: String) {
        self.session_id = Some(id);
    }

    fn validate_environment(config: &AgentConfig) -> Result<()> {
        let binary = config.kind.binary_name();
        if !Self::binary_exists(binary) {
            return Err(Error::BinaryNotFound {
                cli_name: binary.to_string(),
            });
        }
        let env_var = config.kind.api_key_env_var();
        if std::env::var(env_var).is_err() {
            return Err(Error::ApiKeyMissing {
                env_var: env_var.to_string(),
            });
        }
        Ok(())
    }

    fn binary_exists(name: &str) -> bool {
        std::process::Command::new("which")
            .arg(name)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

/// An iterator over events from an agent session.
pub struct EventIterator<'a> {
    receiver: Receiver<AgentEvent>,
    session: &'a mut AgentSession,
}

impl Iterator for EventIterator<'_> {
    type Item = AgentEvent;

    fn next(&mut self) -> Option<Self::Item> {
        match self.receiver.recv() {
            Ok(event) => {
                if let AgentEvent::SessionStarted { session_id: Some(ref id) } = event {
                    self.session.set_session_id(id.clone());
                }
                Some(event)
            }
            Err(_) => None,
        }
    }
}
