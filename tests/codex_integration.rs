//! Integration tests for Codex CLI.

use agent_cli_runner::{AgentConfig, AgentEvent, AgentKind, AgentSession};

fn has_codex_cli() -> bool {
    std::process::Command::new("which")
        .arg("codex")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn has_api_key() -> bool {
    std::env::var("OPENAI_API_KEY").is_ok()
}

#[test]
fn test_codex_binary_check() {
    if !has_codex_cli() {
        eprintln!("Skipping: Codex CLI not found in PATH");
        return;
    }
    let config = AgentConfig::new(AgentKind::Codex);
    if !has_api_key() {
        let result = AgentSession::spawn(config, "test");
        assert!(result.is_err());
        return;
    }
}

#[test]
fn test_codex_missing_api_key() {
    if !has_codex_cli() {
        eprintln!("Skipping: Codex CLI not found in PATH");
        return;
    }
    if has_api_key() {
        eprintln!("Skipping: API key is set, cannot test missing key behavior");
        return;
    }
    let config = AgentConfig::new(AgentKind::Codex);
    let result = AgentSession::spawn(config, "test");
    assert!(result.is_err());
}

#[test]
#[ignore = "requires API key and makes real API calls"]
fn test_codex_simple_prompt() {
    if !has_codex_cli() || !has_api_key() {
        eprintln!("Skipping: Codex CLI or API key not available");
        return;
    }
    let config = AgentConfig::new(AgentKind::Codex).with_skip_permissions();
    let mut session = AgentSession::spawn(config, "Say 'hello' and nothing else")
        .expect("Failed to spawn Codex session");
    let events: Vec<AgentEvent> = session.events().expect("Failed to get events").collect();
    assert!(!events.is_empty(), "Expected at least one event");
    let has_text = events.iter().any(|e| matches!(e, AgentEvent::Text { .. }));
    let has_completed = events
        .iter()
        .any(|e| matches!(e, AgentEvent::SessionCompleted { .. }));
    assert!(has_text || has_completed, "Expected text or completion event");
}
