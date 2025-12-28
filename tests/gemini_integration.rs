//! Integration tests for Gemini CLI.

use agent_cli_runner::{AgentConfig, AgentEvent, AgentKind, AgentSession};

fn has_gemini_cli() -> bool {
    std::process::Command::new("which")
        .arg("gemini")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn has_api_key() -> bool {
    std::env::var("GOOGLE_API_KEY").is_ok() || std::env::var("GEMINI_API_KEY").is_ok()
}

#[test]
fn test_gemini_binary_check() {
    if !has_gemini_cli() {
        eprintln!("Skipping: Gemini CLI not found in PATH");
        return;
    }
    let config = AgentConfig::new(AgentKind::Gemini);
    if !has_api_key() {
        let result = AgentSession::spawn(config, "test");
        assert!(result.is_err());
        return;
    }
}

#[test]
fn test_gemini_missing_api_key() {
    if !has_gemini_cli() {
        eprintln!("Skipping: Gemini CLI not found in PATH");
        return;
    }
    if has_api_key() {
        eprintln!("Skipping: API key is set, cannot test missing key behavior");
        return;
    }
    let config = AgentConfig::new(AgentKind::Gemini);
    let result = AgentSession::spawn(config, "test");
    assert!(result.is_err());
}

#[test]
#[ignore = "requires API key and makes real API calls"]
fn test_gemini_simple_prompt() {
    if !has_gemini_cli() || !has_api_key() {
        eprintln!("Skipping: Gemini CLI or API key not available");
        return;
    }
    let config = AgentConfig::new(AgentKind::Gemini).with_skip_permissions();
    let mut session = AgentSession::spawn(config, "Say 'hello' and nothing else")
        .expect("Failed to spawn Gemini session");
    let events: Vec<AgentEvent> = session.events().expect("Failed to get events").collect();
    assert!(!events.is_empty(), "Expected at least one event");
    let has_text = events.iter().any(|e| matches!(e, AgentEvent::Text { .. }));
    let has_completed = events
        .iter()
        .any(|e| matches!(e, AgentEvent::SessionCompleted { .. }));
    assert!(has_text || has_completed, "Expected text or completion event");
}
