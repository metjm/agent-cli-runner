//! Integration tests for the schema extraction CLI tool.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema_extraction")
}

fn temp_output_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("schema_extraction_test_{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    dir
}

fn cleanup_temp_dir(dir: &PathBuf) {
    let _ = fs::remove_dir_all(dir);
}

fn build_binary() -> PathBuf {
    // Build the binary in release mode for faster tests
    let status = Command::new("cargo")
        .args(["build", "--bin", "schema_extraction"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("Failed to build binary");
    assert!(status.success(), "Binary build failed");

    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/debug/schema_extraction")
}

#[test]
fn test_extract_from_new_format_log() {
    let binary = build_binary();
    let output_dir = temp_output_dir();
    let fixtures = fixtures_dir();

    let status = Command::new(&binary)
        .args([
            "--input",
            fixtures.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--overwrite",
            "-v",
        ])
        .status()
        .expect("Failed to run binary");

    assert!(status.success(), "Binary execution failed");

    // Check Claude output files exist
    let claude_dir = output_dir.join("claude");
    assert!(claude_dir.exists(), "Claude output directory should exist");
    assert!(
        claude_dir.join("system.jsonl").exists(),
        "system.jsonl should exist"
    );
    assert!(
        claude_dir.join("system.schema.json").exists(),
        "system.schema.json should exist"
    );
    assert!(
        claude_dir.join("assistant.jsonl").exists(),
        "assistant.jsonl should exist"
    );
    assert!(
        claude_dir.join("user.jsonl").exists(),
        "user.jsonl should exist"
    );
    assert!(
        claude_dir.join("result.jsonl").exists(),
        "result.jsonl should exist"
    );
    assert!(
        claude_dir.join("summary.json").exists(),
        "summary.json should exist"
    );

    // Check Gemini output files exist
    let gemini_dir = output_dir.join("gemini");
    assert!(gemini_dir.exists(), "Gemini output directory should exist");
    assert!(
        gemini_dir.join("session_start.jsonl").exists(),
        "session_start.jsonl should exist for Gemini"
    );
    assert!(
        gemini_dir.join("text.jsonl").exists(),
        "text.jsonl should exist for Gemini"
    );
    assert!(
        gemini_dir.join("tool_call.jsonl").exists(),
        "tool_call.jsonl should exist for Gemini"
    );
    assert!(
        gemini_dir.join("tool_result.jsonl").exists(),
        "tool_result.jsonl should exist for Gemini"
    );
    assert!(
        gemini_dir.join("session_end.jsonl").exists(),
        "session_end.jsonl should exist for Gemini"
    );

    // Check Codex output files exist
    let codex_dir = output_dir.join("codex");
    assert!(codex_dir.exists(), "Codex output directory should exist");
    assert!(
        codex_dir.join("session_start.jsonl").exists(),
        "session_start.jsonl should exist for Codex"
    );
    assert!(
        codex_dir.join("message.jsonl").exists(),
        "message.jsonl should exist for Codex"
    );
    assert!(
        codex_dir.join("exec_result.jsonl").exists(),
        "exec_result.jsonl should exist for Codex"
    );
    assert!(
        codex_dir.join("session_end.jsonl").exists(),
        "session_end.jsonl should exist for Codex"
    );

    cleanup_temp_dir(&output_dir);
}

#[test]
fn test_schema_content_is_valid_json_schema() {
    let binary = build_binary();
    let output_dir = temp_output_dir();
    let fixtures = fixtures_dir();

    let status = Command::new(&binary)
        .args([
            "--input",
            fixtures.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--overwrite",
        ])
        .status()
        .expect("Failed to run binary");

    assert!(status.success(), "Binary execution failed");

    // Read and validate a schema file
    let schema_path = output_dir.join("claude/system.schema.json");
    let schema_content = fs::read_to_string(&schema_path).expect("Failed to read schema file");
    let schema: serde_json::Value =
        serde_json::from_str(&schema_content).expect("Schema should be valid JSON");

    // Verify schema structure
    assert_eq!(
        schema.get("$schema").and_then(|v| v.as_str()),
        Some("http://json-schema.org/draft-07/schema#"),
        "Schema should have $schema field"
    );
    assert!(
        schema.get("title").is_some(),
        "Schema should have title field"
    );
    assert!(
        schema.get("properties").is_some(),
        "Schema should have properties field"
    );

    // Verify properties include expected fields
    let props = schema.get("properties").unwrap().as_object().unwrap();
    assert!(props.contains_key("type"), "Should have 'type' property");
    assert!(
        props.contains_key("session_id"),
        "Should have 'session_id' property"
    );

    cleanup_temp_dir(&output_dir);
}

#[test]
fn test_jsonl_content_is_valid() {
    let binary = build_binary();
    let output_dir = temp_output_dir();
    let fixtures = fixtures_dir();

    let status = Command::new(&binary)
        .args([
            "--input",
            fixtures.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--overwrite",
        ])
        .status()
        .expect("Failed to run binary");

    assert!(status.success(), "Binary execution failed");

    // Read and validate JSONL file
    let jsonl_path = output_dir.join("claude/system.jsonl");
    let jsonl_content = fs::read_to_string(&jsonl_path).expect("Failed to read JSONL file");

    for (i, line) in jsonl_content.lines().enumerate() {
        let parsed: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("Line {} is not valid JSON: {}", i, e));
        assert!(
            parsed.get("type").is_some(),
            "Line {} should have 'type' field",
            i
        );
    }

    cleanup_temp_dir(&output_dir);
}

#[test]
fn test_summary_json_structure() {
    let binary = build_binary();
    let output_dir = temp_output_dir();
    let fixtures = fixtures_dir();

    let status = Command::new(&binary)
        .args([
            "--input",
            fixtures.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--overwrite",
        ])
        .status()
        .expect("Failed to run binary");

    assert!(status.success(), "Binary execution failed");

    // Read and validate summary file
    let summary_path = output_dir.join("claude/summary.json");
    let summary_content = fs::read_to_string(&summary_path).expect("Failed to read summary file");
    let summary: serde_json::Value =
        serde_json::from_str(&summary_content).expect("Summary should be valid JSON");

    assert_eq!(
        summary.get("agent").and_then(|v| v.as_str()),
        Some("claude"),
        "Summary should have agent field"
    );
    assert!(
        summary.get("event_counts").is_some(),
        "Summary should have event_counts field"
    );
    assert!(
        summary.get("total_samples_stored").is_some(),
        "Summary should have total_samples_stored field"
    );
    assert!(
        summary.get("source_files").is_some(),
        "Summary should have source_files field"
    );

    // Check event counts
    let counts = summary.get("event_counts").unwrap().as_object().unwrap();
    assert!(counts.contains_key("system"), "Should have system count");
    assert!(
        counts.contains_key("assistant"),
        "Should have assistant count"
    );

    cleanup_temp_dir(&output_dir);
}

#[test]
fn test_agent_filter() {
    let binary = build_binary();
    let output_dir = temp_output_dir();
    let fixtures = fixtures_dir();

    let status = Command::new(&binary)
        .args([
            "--input",
            fixtures.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--agents",
            "claude",
            "--overwrite",
        ])
        .status()
        .expect("Failed to run binary");

    assert!(status.success(), "Binary execution failed");

    // Claude should exist
    assert!(
        output_dir.join("claude").exists(),
        "Claude output should exist"
    );

    // Gemini and Codex should NOT exist
    assert!(
        !output_dir.join("gemini").exists(),
        "Gemini output should not exist when filtered"
    );
    assert!(
        !output_dir.join("codex").exists(),
        "Codex output should not exist when filtered"
    );

    cleanup_temp_dir(&output_dir);
}

#[test]
fn test_no_overwrite_by_default() {
    let binary = build_binary();
    let output_dir = temp_output_dir();
    let fixtures = fixtures_dir();

    // First run - creates files
    let status = Command::new(&binary)
        .args([
            "--input",
            fixtures.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--overwrite",
        ])
        .status()
        .expect("Failed to run binary");
    assert!(status.success());

    // Get the original content
    let original = fs::read_to_string(output_dir.join("claude/summary.json")).unwrap();

    // Modify the file
    fs::write(output_dir.join("claude/summary.json"), "modified").unwrap();

    // Second run without --overwrite - should skip
    let status = Command::new(&binary)
        .args([
            "--input",
            fixtures.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to run binary");
    assert!(status.success());

    // File should still be modified
    let content = fs::read_to_string(output_dir.join("claude/summary.json")).unwrap();
    assert_eq!(content, "modified", "File should not be overwritten");

    // Third run with --overwrite - should overwrite
    let status = Command::new(&binary)
        .args([
            "--input",
            fixtures.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--overwrite",
        ])
        .status()
        .expect("Failed to run binary");
    assert!(status.success());

    // File should be restored to original
    let content = fs::read_to_string(output_dir.join("claude/summary.json")).unwrap();
    assert_eq!(content, original, "File should be overwritten");

    cleanup_temp_dir(&output_dir);
}

#[test]
fn test_max_samples_limit() {
    let binary = build_binary();
    let output_dir = temp_output_dir();
    let fixtures = fixtures_dir();

    // Run with max-samples 1
    let status = Command::new(&binary)
        .args([
            "--input",
            fixtures.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--max-samples",
            "1",
            "--overwrite",
        ])
        .status()
        .expect("Failed to run binary");

    assert!(status.success(), "Binary execution failed");

    // Check that assistant.jsonl has only 1 line (there are multiple assistant events in fixtures)
    let jsonl_path = output_dir.join("claude/assistant.jsonl");
    let jsonl_content = fs::read_to_string(&jsonl_path).expect("Failed to read JSONL file");
    let line_count = jsonl_content.lines().count();
    assert_eq!(line_count, 1, "Should have only 1 sample due to --max-samples 1");

    cleanup_temp_dir(&output_dir);
}

#[test]
fn test_emit_unparsed() {
    let binary = build_binary();
    let output_dir = temp_output_dir();
    let fixtures = fixtures_dir();

    // Run with --emit-unparsed
    let status = Command::new(&binary)
        .args([
            "--input",
            fixtures.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--emit-unparsed",
            "--overwrite",
        ])
        .status()
        .expect("Failed to run binary");

    assert!(status.success(), "Binary execution failed");

    // Check that unparsed.jsonl exists for claude (has non-JSON stdout lines)
    let unparsed_path = output_dir.join("claude/unparsed.jsonl");
    assert!(unparsed_path.exists(), "unparsed.jsonl should exist");

    let content = fs::read_to_string(&unparsed_path).unwrap();
    assert!(!content.is_empty(), "unparsed.jsonl should not be empty");

    cleanup_temp_dir(&output_dir);
}

#[test]
fn test_no_schema_flag() {
    let binary = build_binary();
    let output_dir = temp_output_dir();
    let fixtures = fixtures_dir();

    let status = Command::new(&binary)
        .args([
            "--input",
            fixtures.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--no-schema",
            "--overwrite",
        ])
        .status()
        .expect("Failed to run binary");

    assert!(status.success(), "Binary execution failed");

    // JSONL should exist
    assert!(output_dir.join("claude/system.jsonl").exists());

    // Schema should NOT exist
    assert!(!output_dir.join("claude/system.schema.json").exists());

    cleanup_temp_dir(&output_dir);
}

#[test]
fn test_no_raw_flag() {
    let binary = build_binary();
    let output_dir = temp_output_dir();
    let fixtures = fixtures_dir();

    let status = Command::new(&binary)
        .args([
            "--input",
            fixtures.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--no-raw",
            "--overwrite",
        ])
        .status()
        .expect("Failed to run binary");

    assert!(status.success(), "Binary execution failed");

    // Schema should exist
    assert!(output_dir.join("claude/system.schema.json").exists());

    // JSONL should NOT exist
    assert!(!output_dir.join("claude/system.jsonl").exists());

    cleanup_temp_dir(&output_dir);
}

#[test]
fn test_help_flag() {
    let binary = build_binary();

    let output = Command::new(&binary)
        .arg("--help")
        .output()
        .expect("Failed to run binary");

    assert!(output.status.success(), "Help should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("USAGE:"), "Help should show usage");
    assert!(stdout.contains("--input"), "Help should show --input");
    assert!(stdout.contains("--output"), "Help should show --output");
    assert!(
        stdout.contains("--emit-nested-schema"),
        "Help should show --emit-nested-schema"
    );
    assert!(
        stdout.contains("--emit-coverage"),
        "Help should show --emit-coverage"
    );
    assert!(
        stdout.contains("--enum-threshold"),
        "Help should show --enum-threshold"
    );
}

#[test]
fn test_nested_content_block_schemas() {
    let binary = build_binary();
    let output_dir = temp_output_dir();
    let fixtures = fixtures_dir();

    let status = Command::new(&binary)
        .args([
            "--input",
            fixtures.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--overwrite",
        ])
        .status()
        .expect("Failed to run binary");

    assert!(status.success(), "Binary execution failed");

    // Check Claude content block schemas exist
    let claude_dir = output_dir.join("claude");
    assert!(
        claude_dir.join("content_block.text.schema.json").exists(),
        "content_block.text.schema.json should exist"
    );
    assert!(
        claude_dir.join("content_block.tool_use.schema.json").exists(),
        "content_block.tool_use.schema.json should exist"
    );
    assert!(
        claude_dir.join("content_block.tool_result.schema.json").exists(),
        "content_block.tool_result.schema.json should exist"
    );

    // Check Codex content block schemas exist
    let codex_dir = output_dir.join("codex");
    assert!(
        codex_dir.join("content_block.text.schema.json").exists(),
        "Codex content_block.text.schema.json should exist"
    );
    assert!(
        codex_dir.join("content_block.function_call.schema.json").exists(),
        "Codex content_block.function_call.schema.json should exist"
    );

    // Validate Claude tool_use content block schema structure
    let tool_use_schema_path = claude_dir.join("content_block.tool_use.schema.json");
    let schema_content = fs::read_to_string(&tool_use_schema_path).expect("Failed to read schema");
    let schema: serde_json::Value = serde_json::from_str(&schema_content).expect("Invalid JSON");

    // Should have properties like id, name, type, input
    let props = schema.get("properties").unwrap().as_object().unwrap();
    assert!(props.contains_key("type"), "tool_use should have 'type' property");
    assert!(props.contains_key("id"), "tool_use should have 'id' property");
    assert!(props.contains_key("name"), "tool_use should have 'name' property");
    assert!(props.contains_key("input"), "tool_use should have 'input' property");

    cleanup_temp_dir(&output_dir);
}

#[test]
fn test_tool_input_schemas() {
    let binary = build_binary();
    let output_dir = temp_output_dir();
    let fixtures = fixtures_dir();

    let status = Command::new(&binary)
        .args([
            "--input",
            fixtures.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--overwrite",
        ])
        .status()
        .expect("Failed to run binary");

    assert!(status.success(), "Binary execution failed");

    // Check Claude tool input schemas exist
    let claude_dir = output_dir.join("claude");
    assert!(
        claude_dir.join("tool_input.Read.schema.json").exists(),
        "tool_input.Read.schema.json should exist"
    );
    assert!(
        claude_dir.join("tool_input.Bash.schema.json").exists(),
        "tool_input.Bash.schema.json should exist"
    );

    // Check Gemini tool input schemas exist
    let gemini_dir = output_dir.join("gemini");
    assert!(
        gemini_dir.join("tool_input.read_file.schema.json").exists(),
        "Gemini tool_input.read_file.schema.json should exist"
    );

    // Check Codex tool input schemas exist
    let codex_dir = output_dir.join("codex");
    assert!(
        codex_dir.join("tool_input.shell.schema.json").exists(),
        "Codex tool_input.shell.schema.json should exist"
    );

    // Validate Read tool input schema has file_path property
    let read_schema_path = claude_dir.join("tool_input.Read.schema.json");
    let schema_content = fs::read_to_string(&read_schema_path).expect("Failed to read schema");
    let schema: serde_json::Value = serde_json::from_str(&schema_content).expect("Invalid JSON");

    let props = schema.get("properties").unwrap().as_object().unwrap();
    assert!(
        props.contains_key("file_path"),
        "Read tool input should have 'file_path' property"
    );

    cleanup_temp_dir(&output_dir);
}

#[test]
fn test_coverage_report() {
    let binary = build_binary();
    let output_dir = temp_output_dir();
    let fixtures = fixtures_dir();

    let status = Command::new(&binary)
        .args([
            "--input",
            fixtures.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--overwrite",
        ])
        .status()
        .expect("Failed to run binary");

    assert!(status.success(), "Binary execution failed");

    // Check coverage.json exists
    let coverage_path = output_dir.join("coverage.json");
    assert!(coverage_path.exists(), "coverage.json should exist");

    // Validate coverage.json structure
    let coverage_content = fs::read_to_string(&coverage_path).expect("Failed to read coverage");
    let coverage: serde_json::Value = serde_json::from_str(&coverage_content).expect("Invalid JSON");

    // Check structure
    assert!(coverage.get("agents").is_some(), "Should have agents field");
    assert!(coverage.get("summary").is_some(), "Should have summary field");

    // Check Claude agent coverage
    let agents = coverage.get("agents").unwrap().as_object().unwrap();
    assert!(agents.contains_key("claude"), "Should have claude agent");
    let claude = agents.get("claude").unwrap().as_object().unwrap();

    // Check events section
    let events = claude.get("events").unwrap().as_object().unwrap();
    assert!(events.get("expected").is_some(), "Should have expected events");
    assert!(events.get("observed").is_some(), "Should have observed events");
    assert!(events.get("missing").is_some(), "Should have missing events");

    // Check content_blocks section
    let blocks = claude.get("content_blocks").unwrap().as_object().unwrap();
    assert!(blocks.get("expected").is_some(), "Should have expected blocks");
    assert!(blocks.get("observed").is_some(), "Should have observed blocks");

    // Check tool_inputs section
    let tools = claude.get("tool_inputs").unwrap().as_object().unwrap();
    assert!(tools.get("observed").is_some(), "Should have observed tools");
    assert!(tools.get("sample_counts").is_some(), "Should have tool sample counts");

    cleanup_temp_dir(&output_dir);
}

#[test]
fn test_no_coverage_flag() {
    let binary = build_binary();
    let output_dir = temp_output_dir();
    let fixtures = fixtures_dir();

    let status = Command::new(&binary)
        .args([
            "--input",
            fixtures.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--no-coverage",
            "--overwrite",
        ])
        .status()
        .expect("Failed to run binary");

    assert!(status.success(), "Binary execution failed");

    // coverage.json should NOT exist
    assert!(
        !output_dir.join("coverage.json").exists(),
        "coverage.json should not exist when --no-coverage is used"
    );

    cleanup_temp_dir(&output_dir);
}

#[test]
fn test_no_nested_schema_flag() {
    let binary = build_binary();
    let output_dir = temp_output_dir();
    let fixtures = fixtures_dir();

    let status = Command::new(&binary)
        .args([
            "--input",
            fixtures.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--no-nested-schema",
            "--overwrite",
        ])
        .status()
        .expect("Failed to run binary");

    assert!(status.success(), "Binary execution failed");

    let claude_dir = output_dir.join("claude");

    // Regular schema should exist
    assert!(
        claude_dir.join("assistant.schema.json").exists(),
        "assistant.schema.json should exist"
    );

    // Nested schemas should NOT exist
    assert!(
        !claude_dir.join("content_block.text.schema.json").exists(),
        "content_block.text.schema.json should not exist with --no-nested-schema"
    );
    assert!(
        !claude_dir.join("tool_input.Read.schema.json").exists(),
        "tool_input.Read.schema.json should not exist with --no-nested-schema"
    );

    cleanup_temp_dir(&output_dir);
}

#[test]
fn test_summary_includes_nested_counts() {
    let binary = build_binary();
    let output_dir = temp_output_dir();
    let fixtures = fixtures_dir();

    let status = Command::new(&binary)
        .args([
            "--input",
            fixtures.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--overwrite",
        ])
        .status()
        .expect("Failed to run binary");

    assert!(status.success(), "Binary execution failed");

    // Read and validate Claude summary
    let summary_path = output_dir.join("claude/summary.json");
    let summary_content = fs::read_to_string(&summary_path).expect("Failed to read summary");
    let summary: serde_json::Value = serde_json::from_str(&summary_content).expect("Invalid JSON");

    // Should have content_block_counts
    assert!(
        summary.get("content_block_counts").is_some(),
        "Summary should have content_block_counts"
    );
    let block_counts = summary.get("content_block_counts").unwrap().as_object().unwrap();
    assert!(
        block_counts.contains_key("text"),
        "Should have text block count"
    );
    assert!(
        block_counts.contains_key("tool_use"),
        "Should have tool_use block count"
    );

    // Should have tool_input_counts
    assert!(
        summary.get("tool_input_counts").is_some(),
        "Summary should have tool_input_counts"
    );
    let tool_counts = summary.get("tool_input_counts").unwrap().as_object().unwrap();
    assert!(
        tool_counts.contains_key("Read"),
        "Should have Read tool count"
    );

    cleanup_temp_dir(&output_dir);
}
