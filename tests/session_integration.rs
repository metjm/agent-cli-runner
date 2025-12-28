//! Integration tests for session management across CLIs.

use agent_cli_runner::{AgentConfig, AgentKind};

#[test]
fn test_config_builder() {
    let config = AgentConfig::new(AgentKind::Claude)
        .with_skip_permissions()
        .with_model("sonnet")
        .with_debug()
        .with_channel_buffer_size(50);
    assert_eq!(config.kind, AgentKind::Claude);
    assert!(config.skip_permissions);
    assert_eq!(config.model, Some("sonnet".to_string()));
    assert!(config.debug);
    assert_eq!(config.channel_buffer_size, 50);
}

#[test]
fn test_agent_kind_properties() {
    assert_eq!(AgentKind::Claude.binary_name(), "claude");
    assert_eq!(AgentKind::Codex.binary_name(), "codex");
    assert_eq!(AgentKind::Gemini.binary_name(), "gemini");
    assert_eq!(AgentKind::Claude.api_key_env_var(), "ANTHROPIC_API_KEY");
    assert_eq!(AgentKind::Codex.api_key_env_var(), "OPENAI_API_KEY");
    assert_eq!(AgentKind::Gemini.api_key_env_var(), "GOOGLE_API_KEY");
    assert_eq!(AgentKind::Claude.display_name(), "Claude Code");
    assert_eq!(AgentKind::Codex.display_name(), "Codex CLI");
    assert_eq!(AgentKind::Gemini.display_name(), "Gemini CLI");
}

#[test]
fn test_config_with_working_dir() {
    use std::path::PathBuf;
    let dir = PathBuf::from("/tmp");
    let config = AgentConfig::new(AgentKind::Codex).with_working_dir(dir.clone());
    assert_eq!(config.working_dir, Some(dir));
}

#[test]
fn test_config_with_session_id() {
    let config = AgentConfig::new(AgentKind::Gemini)
        .with_session_id("test-session-123");
    assert_eq!(config.session_id, Some("test-session-123".to_string()));
}
