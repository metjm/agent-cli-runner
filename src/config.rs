//! Configuration for agent CLI sessions.


use std::path::PathBuf;

/// The type of agent CLI to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentKind {
    /// Claude Code CLI.
    Claude,
    /// Codex CLI.
    Codex,
    /// Gemini CLI.
    Gemini,
}

impl AgentKind {
    /// Returns the binary name for this CLI.
    #[must_use]
    pub const fn binary_name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
        }
    }

    /// Returns the required API key environment variable name.
    #[must_use]
    pub const fn api_key_env_var(self) -> &'static str {
        match self {
            Self::Claude => "ANTHROPIC_API_KEY",
            Self::Codex => "OPENAI_API_KEY",
            Self::Gemini => "GOOGLE_API_KEY",
        }
    }

    /// Returns a human-readable name for this CLI.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex CLI",
            Self::Gemini => "Gemini CLI",
        }
    }
}

/// Configuration for an agent session.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// The type of agent CLI to use.
    pub kind: AgentKind,
    /// Working directory for the CLI process.
    pub working_dir: Option<PathBuf>,
    /// Whether to skip permission prompts (dangerous mode).
    pub skip_permissions: bool,
    /// Optional model override.
    pub model: Option<String>,
    /// Session ID for resuming a previous session.
    pub session_id: Option<String>,
    /// Whether to enable debug output.
    pub debug: bool,
    /// Channel buffer size for event streaming (0 = unbounded).
    pub channel_buffer_size: usize,
}

impl AgentConfig {
    /// Creates a new configuration for the specified agent kind.
    #[must_use]
    pub const fn new(kind: AgentKind) -> Self {
        Self {
            kind,
            working_dir: None,
            skip_permissions: false,
            model: None,
            session_id: None,
            debug: false,
            channel_buffer_size: 100,
        }
    }

    /// Sets the working directory for the CLI process.
    #[must_use]
    pub fn with_working_dir(mut self, dir: PathBuf) -> Self {
        self.working_dir = Some(dir);
        self
    }

    /// Enables dangerous mode to skip permission prompts.
    #[must_use]
    pub const fn with_skip_permissions(mut self) -> Self {
        self.skip_permissions = true;
        self
    }

    /// Sets the model to use for the session.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Sets the session ID for resuming a previous session.
    #[must_use]
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Enables debug output.
    #[must_use]
    pub const fn with_debug(mut self) -> Self {
        self.debug = true;
        self
    }

    /// Sets the channel buffer size for event streaming.
    #[must_use]
    pub const fn with_channel_buffer_size(mut self, size: usize) -> Self {
        self.channel_buffer_size = size;
        self
    }
}
