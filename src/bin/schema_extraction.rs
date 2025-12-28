//! Schema extraction CLI for agent-cli-runner.
//!
//! This tool scans planning-agent log files, extracts JSON streaming output by agent,
//! and generates schema artifacts (raw JSONL + inferred JSON Schema) for validating
//! parser expectations and documenting observed output shapes.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// Default enum threshold - maximum distinct values before falling back to string type.
const DEFAULT_ENUM_THRESHOLD: usize = 10;

/// Minimum sample count required before emitting an enum (to avoid overfitting).
const DEFAULT_MIN_ENUM_SAMPLES: usize = 3;

/// CLI configuration parsed from command-line arguments.
struct Config {
    input_dir: PathBuf,
    output_dir: PathBuf,
    agents_filter: Option<Vec<String>>,
    overwrite: bool,
    emit_schema: bool,
    emit_raw: bool,
    max_samples: usize,
    verbose: bool,
    emit_unparsed: bool,
    /// Enable extraction of nested schemas (content blocks, tool inputs).
    emit_nested_schema: bool,
    /// Threshold for enum inference - max distinct values.
    enum_threshold: usize,
    /// Minimum samples required before emitting enum.
    min_enum_samples: usize,
    /// Enable coverage report generation.
    emit_coverage: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            input_dir: PathBuf::from("."),
            output_dir: PathBuf::from("agent-cli-runner/docs/cli-verification/schemas"),
            agents_filter: None,
            overwrite: false,
            emit_schema: true,
            emit_raw: true,
            max_samples: 100,
            verbose: false,
            emit_unparsed: false,
            emit_nested_schema: true,
            enum_threshold: DEFAULT_ENUM_THRESHOLD,
            min_enum_samples: DEFAULT_MIN_ENUM_SAMPLES,
            emit_coverage: true,
        }
    }
}

/// Result of parsing a log line.
#[derive(Debug)]
struct ParsedLine {
    agent: String,
    kind: String,
    payload: String,
}

/// Statistics for a single log file.
#[derive(Default)]
struct FileStats {
    total_lines: usize,
    stdout_lines: usize,
    json_parsed: usize,
    json_failed: usize,
}

/// Collected samples grouped by agent and event type.
struct SampleCollection {
    /// Map of agent -> event_type -> list of JSON values
    samples: HashMap<String, HashMap<String, Vec<Value>>>,
    /// Map of agent -> list of unparsed lines
    unparsed: HashMap<String, Vec<String>>,
    /// Map of agent -> event_type -> count
    counts: HashMap<String, HashMap<String, usize>>,
    /// List of source files processed
    source_files: Vec<PathBuf>,
    /// Nested content block samples: agent -> block_type -> list of JSON values
    content_blocks: HashMap<String, HashMap<String, Vec<Value>>>,
    /// Tool input samples: agent -> tool_name -> list of JSON values
    tool_inputs: HashMap<String, HashMap<String, Vec<Value>>>,
}

impl SampleCollection {
    fn new() -> Self {
        Self {
            samples: HashMap::new(),
            unparsed: HashMap::new(),
            counts: HashMap::new(),
            source_files: Vec::new(),
            content_blocks: HashMap::new(),
            tool_inputs: HashMap::new(),
        }
    }

    fn add_sample(&mut self, agent: &str, event_type: &str, value: Value, max_samples: usize) {
        let agent_samples = self.samples.entry(agent.to_string()).or_default();
        let event_samples = agent_samples.entry(event_type.to_string()).or_default();

        // Track count regardless of sample limit
        let agent_counts = self.counts.entry(agent.to_string()).or_default();
        *agent_counts.entry(event_type.to_string()).or_insert(0) += 1;

        // Only store up to max_samples
        if event_samples.len() < max_samples {
            event_samples.push(value);
        }
    }

    fn add_content_block(&mut self, agent: &str, block_type: &str, value: Value, max_samples: usize) {
        let agent_blocks = self.content_blocks.entry(agent.to_string()).or_default();
        let type_blocks = agent_blocks.entry(block_type.to_string()).or_default();

        if type_blocks.len() < max_samples {
            type_blocks.push(value);
        }
    }

    fn add_tool_input(&mut self, agent: &str, tool_name: &str, value: Value, max_samples: usize) {
        let agent_tools = self.tool_inputs.entry(agent.to_string()).or_default();
        let tool_inputs = agent_tools.entry(tool_name.to_string()).or_default();

        if tool_inputs.len() < max_samples {
            tool_inputs.push(value);
        }
    }

    fn add_unparsed(&mut self, agent: &str, line: String) {
        self.unparsed.entry(agent.to_string()).or_default().push(line);
    }

    fn add_source_file(&mut self, path: PathBuf) {
        self.source_files.push(path);
    }
}

/// Tracks numeric type details (integer vs float).
#[derive(Debug, Clone, Default)]
struct NumericInfo {
    /// All observed values were integers (i64/u64).
    all_integer: bool,
    /// Number of samples observed.
    count: usize,
}

/// Represents a JSON Schema node for inference.
#[derive(Debug, Clone)]
struct SchemaNode {
    types: BTreeSet<String>,
    properties: BTreeMap<String, SchemaNode>,
    required: BTreeSet<String>,
    items: Option<Box<SchemaNode>>,
    seen_count: usize,
    /// Tracked string values for potential enum inference.
    string_values: BTreeSet<String>,
    /// Numeric type tracking.
    numeric_info: NumericInfo,
}

impl SchemaNode {
    fn new() -> Self {
        Self {
            types: BTreeSet::new(),
            properties: BTreeMap::new(),
            required: BTreeSet::new(),
            items: None,
            seen_count: 0,
            string_values: BTreeSet::new(),
            numeric_info: NumericInfo::default(),
        }
    }

    /// Merge another schema node into this one.
    fn merge(&mut self, other: &SchemaNode) {
        // Merge types
        for t in &other.types {
            self.types.insert(t.clone());
        }

        // Merge properties
        for (key, child) in &other.properties {
            if let Some(existing) = self.properties.get_mut(key) {
                existing.merge(child);
            } else {
                self.properties.insert(key.clone(), child.clone());
            }
        }

        // Required = intersection (field must be in all samples to be required)
        if self.seen_count == 0 {
            self.required = other.required.clone();
        } else {
            self.required = self.required.intersection(&other.required).cloned().collect();
        }

        // Merge array items
        if let Some(other_items) = &other.items {
            if let Some(self_items) = &mut self.items {
                self_items.merge(other_items);
            } else {
                self.items = Some(other_items.clone());
            }
        }

        // Merge string values for enum tracking
        for v in &other.string_values {
            self.string_values.insert(v.clone());
        }

        // Merge numeric info
        if other.numeric_info.count > 0 {
            if self.numeric_info.count == 0 {
                self.numeric_info = other.numeric_info.clone();
            } else {
                // If either has seen a float, mark as not all integer
                self.numeric_info.all_integer =
                    self.numeric_info.all_integer && other.numeric_info.all_integer;
                self.numeric_info.count += other.numeric_info.count;
            }
        }

        self.seen_count += 1;
    }

    /// Convert this schema node to a JSON Schema value with configuration.
    fn to_json_schema_with_config(&self, enum_threshold: usize, min_enum_samples: usize) -> Value {
        let mut schema = serde_json::Map::new();

        // Determine if we have object type in the mix
        let has_object = self.types.contains("object");
        let has_non_object = self.types.iter().any(|t| t != "object");

        // Handle type(s)
        if self.types.len() == 1 {
            if let Some(t) = self.types.iter().next() {
                // Use integer if all observed numbers were integers
                let type_str = if t == "number" && self.numeric_info.all_integer && self.numeric_info.count > 0 {
                    "integer".to_string()
                } else {
                    t.clone()
                };
                schema.insert("type".to_string(), Value::String(type_str));
            }
        } else if self.types.len() > 1 {
            // Build anyOf with proper handling for object + non-object unions
            let type_schemas: Vec<Value> = self
                .types
                .iter()
                .map(|t| {
                    let mut s = serde_json::Map::new();
                    let type_str = if t == "number" && self.numeric_info.all_integer && self.numeric_info.count > 0 {
                        "integer".to_string()
                    } else {
                        t.clone()
                    };
                    s.insert("type".to_string(), Value::String(type_str));

                    // For object type in a union, include properties/required
                    if t == "object" && !self.properties.is_empty() {
                        let mut props = serde_json::Map::new();
                        for (key, child) in &self.properties {
                            props.insert(key.clone(), child.to_json_schema_with_config(enum_threshold, min_enum_samples));
                        }
                        s.insert("properties".to_string(), Value::Object(props));

                        if !self.required.is_empty() {
                            let required: Vec<Value> =
                                self.required.iter().map(|r| Value::String(r.clone())).collect();
                            s.insert("required".to_string(), Value::Array(required));
                        }
                    }

                    Value::Object(s)
                })
                .collect();
            schema.insert("anyOf".to_string(), Value::Array(type_schemas));
        }

        // Handle object properties - only at top level if not a union with non-objects
        if !self.properties.is_empty() && !(has_object && has_non_object) {
            let mut props = serde_json::Map::new();
            for (key, child) in &self.properties {
                props.insert(key.clone(), child.to_json_schema_with_config(enum_threshold, min_enum_samples));
            }
            schema.insert("properties".to_string(), Value::Object(props));

            if !self.required.is_empty() {
                let required: Vec<Value> = self.required.iter().map(|s| Value::String(s.clone())).collect();
                schema.insert("required".to_string(), Value::Array(required));
            }
        }

        // Handle array items
        if let Some(items) = &self.items {
            schema.insert("items".to_string(), items.to_json_schema_with_config(enum_threshold, min_enum_samples));
        }

        // Handle enum for strings
        if self.types.len() == 1
            && self.types.contains("string")
            && !self.string_values.is_empty()
            && self.string_values.len() <= enum_threshold
            && self.seen_count >= min_enum_samples
        {
            let enum_values: Vec<Value> = self
                .string_values
                .iter()
                .map(|s| Value::String(s.clone()))
                .collect();
            schema.insert("enum".to_string(), Value::Array(enum_values));
        }

        Value::Object(schema)
    }

}

/// Infer a schema node from a JSON value.
fn infer_schema(value: &Value) -> SchemaNode {
    let mut node = SchemaNode::new();
    node.seen_count = 1;

    match value {
        Value::Null => {
            node.types.insert("null".to_string());
        }
        Value::Bool(_) => {
            node.types.insert("boolean".to_string());
        }
        Value::Number(n) => {
            node.types.insert("number".to_string());
            // Track if this is an integer
            node.numeric_info.count = 1;
            node.numeric_info.all_integer = n.is_i64() || n.is_u64();
        }
        Value::String(s) => {
            node.types.insert("string".to_string());
            // Track string value for potential enum inference
            node.string_values.insert(s.clone());
        }
        Value::Array(arr) => {
            node.types.insert("array".to_string());
            if !arr.is_empty() {
                let mut items_schema = SchemaNode::new();
                for item in arr {
                    let item_schema = infer_schema(item);
                    items_schema.merge(&item_schema);
                }
                node.items = Some(Box::new(items_schema));
            }
        }
        Value::Object(obj) => {
            node.types.insert("object".to_string());
            for (key, val) in obj {
                node.required.insert(key.clone());
                node.properties.insert(key.clone(), infer_schema(val));
            }
        }
    }

    node
}

/// Parse the new log format: [time][agent][kind] payload
fn parse_new_format(line: &str) -> Option<ParsedLine> {
    // Line must start with '['
    if !line.starts_with('[') {
        return None;
    }

    let mut rest = &line[1..];

    // Extract time (first bracket group)
    let time_end = rest.find(']')?;
    let _time = &rest[..time_end];
    rest = rest.get(time_end + 1..)?;

    // Extract agent (second bracket group) - must start with '['
    if !rest.starts_with('[') {
        return None;
    }
    rest = &rest[1..];
    let agent_end = rest.find(']')?;
    let agent = &rest[..agent_end];

    // Validate agent name (alphanumeric, hyphens, underscores)
    if !agent.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return None;
    }

    rest = rest.get(agent_end + 1..)?;

    // Extract kind (third bracket group) - must start with '['
    if !rest.starts_with('[') {
        return None;
    }
    rest = &rest[1..];
    let kind_end = rest.find(']')?;
    let kind = &rest[..kind_end];
    rest = rest.get(kind_end + 1..)?;

    // Payload follows after space
    let payload = rest.strip_prefix(' ').unwrap_or(rest);

    Some(ParsedLine {
        agent: agent.to_string(),
        kind: kind.to_string(),
        payload: payload.to_string(),
    })
}

/// Parse the legacy log format: [kind] payload
/// Agent is inferred from filename.
fn parse_legacy_format(line: &str, filename_agent: &str) -> Option<ParsedLine> {
    // Line must start with '['
    if !line.starts_with('[') {
        return None;
    }

    let rest = &line[1..];
    let kind_end = rest.find(']')?;
    let kind = &rest[..kind_end];
    let payload = rest.get(kind_end + 1..)?.strip_prefix(' ').unwrap_or(&rest[kind_end + 1..]);

    Some(ParsedLine {
        agent: filename_agent.to_string(),
        kind: kind.to_string(),
        payload: payload.to_string(),
    })
}

/// Extract agent name from legacy filename (e.g., "claude-stream-*.log" -> "claude")
fn agent_from_filename(filename: &str) -> Option<String> {
    // Pattern: <agent>-stream-*.log
    let stem = filename.strip_suffix(".log")?;
    let parts: Vec<&str> = stem.split("-stream-").collect();
    if parts.len() >= 2 {
        return Some(parts[0].to_string());
    }
    None
}

/// Determine log format from filename.
enum LogFormat {
    New,             // agent-stream-*.log
    Legacy(String),  // <agent>-stream-*.log with agent name
}

fn detect_log_format(filename: &str) -> Option<LogFormat> {
    if filename.starts_with("agent-stream-") && filename.ends_with(".log") {
        return Some(LogFormat::New);
    }

    // Legacy format: <agent>-stream-*.log
    if filename.ends_with(".log") && filename.contains("-stream-") {
        if let Some(agent) = agent_from_filename(filename) {
            return Some(LogFormat::Legacy(agent));
        }
    }

    None
}

/// Get the event discriminator value for a given agent and JSON.
fn get_event_discriminator(agent: &str, json: &Value) -> String {
    let field = if agent == "codex" { "event" } else { "type" };

    json.get(field)
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| "unknown".to_string())
}

/// Extract nested content blocks and tool inputs from an event.
///
/// For Claude: looks at `message.content[]` or `content[]` arrays
/// For Codex: looks at `message.content[]` arrays
/// For Gemini: tool_call events are already at top level
fn extract_nested_content(
    agent: &str,
    event_type: &str,
    json: &Value,
    collection: &mut SampleCollection,
    max_samples: usize,
) {
    // Get content array based on agent and event structure
    let content_array = match agent {
        "claude" => {
            // Claude: message.content[] or content[]
            json.get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
                .or_else(|| json.get("content").and_then(|c| c.as_array()))
        }
        "codex" => {
            // Codex: message.content[]
            json.get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
        }
        _ => None,
    };

    if let Some(blocks) = content_array {
        for block in blocks {
            // Get block type
            let block_type = block
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("unknown");

            // Add the content block sample
            collection.add_content_block(agent, block_type, block.clone(), max_samples);

            // For tool_use blocks, extract tool input by name
            if block_type == "tool_use" {
                if let (Some(name), Some(input)) = (
                    block.get("name").and_then(Value::as_str),
                    block.get("input"),
                ) {
                    collection.add_tool_input(agent, name, input.clone(), max_samples);
                }
            }

            // For Codex function_call blocks, extract tool input by name
            if block_type == "function_call" {
                if let (Some(name), Some(args)) = (
                    block.get("name").and_then(Value::as_str),
                    block.get("arguments"),
                ) {
                    collection.add_tool_input(agent, name, args.clone(), max_samples);
                }
            }
        }
    }

    // For Gemini tool_call events (already top-level)
    if agent == "gemini" && event_type == "tool_call" {
        if let (Some(name), Some(input)) = (
            json.get("name").and_then(Value::as_str),
            json.get("input"),
        ) {
            collection.add_tool_input(agent, name, input.clone(), max_samples);
        }
    }

    // For user events with tool_result, also capture by tool_use_id for reference
    if event_type == "user" {
        if let Some(content) = json.get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
        {
            for block in content {
                let block_type = block
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                collection.add_content_block(agent, block_type, block.clone(), max_samples);
            }
        }
    }
}

/// Check if a directory should be skipped during recursive walk.
fn should_skip_dir(name: &str) -> bool {
    matches!(name, "target" | ".git" | "node_modules")
}

/// Recursively find log files in a directory.
fn find_log_files(dir: &Path, files: &mut Vec<(PathBuf, LogFormat)>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            if !should_skip_dir(&name) {
                find_log_files(&path, files)?;
            }
        } else if path.is_file() {
            if let Some(format) = detect_log_format(&name) {
                files.push((path, format));
            }
        }
    }

    Ok(())
}

/// Process a single log file.
fn process_log_file(
    path: &Path,
    format: &LogFormat,
    collection: &mut SampleCollection,
    config: &Config,
) -> std::io::Result<FileStats> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut stats = FileStats::default();

    for line in reader.lines() {
        let line = line?;
        stats.total_lines += 1;

        // Skip header lines
        if line.starts_with("===") {
            continue;
        }

        // Parse the line based on format
        let parsed = match format {
            LogFormat::New => parse_new_format(&line),
            LogFormat::Legacy(agent) => parse_legacy_format(&line, agent),
        };

        let Some(parsed) = parsed else {
            continue;
        };

        // Filter by agent if specified
        if let Some(filter) = &config.agents_filter {
            if !filter.contains(&parsed.agent) {
                continue;
            }
        }

        // Only process stdout lines
        if parsed.kind != "stdout" {
            continue;
        }

        stats.stdout_lines += 1;

        // Try to parse as JSON
        match serde_json::from_str::<Value>(&parsed.payload) {
            Ok(json) => {
                stats.json_parsed += 1;
                let event_type = get_event_discriminator(&parsed.agent, &json);
                collection.add_sample(&parsed.agent, &event_type, json.clone(), config.max_samples);

                // Extract nested content blocks and tool inputs
                if config.emit_nested_schema {
                    extract_nested_content(
                        &parsed.agent,
                        &event_type,
                        &json,
                        collection,
                        config.max_samples,
                    );
                }
            }
            Err(e) => {
                stats.json_failed += 1;
                if config.verbose {
                    eprintln!("JSON parse error in {}: {} - {}", path.display(), e, parsed.payload.chars().take(100).collect::<String>());
                }
                if config.emit_unparsed {
                    collection.add_unparsed(&parsed.agent, parsed.payload);
                }
            }
        }
    }

    collection.add_source_file(path.to_path_buf());
    Ok(stats)
}

/// Helper to write a schema file.
fn write_schema_file(
    path: &Path,
    title: &str,
    description: &str,
    values: &[Value],
    config: &Config,
) -> std::io::Result<()> {
    if path.exists() && !config.overwrite {
        eprintln!("Skipping existing file: {}", path.display());
        return Ok(());
    }

    // Infer schema from all samples
    let mut schema = SchemaNode::new();
    for value in values {
        let sample_schema = infer_schema(value);
        schema.merge(&sample_schema);
    }

    // Build full schema document
    let mut doc = serde_json::Map::new();
    doc.insert(
        "$schema".to_string(),
        Value::String("http://json-schema.org/draft-07/schema#".to_string()),
    );
    doc.insert("title".to_string(), Value::String(title.to_string()));
    doc.insert("description".to_string(), Value::String(description.to_string()));

    // Merge inferred schema into doc with config
    if let Value::Object(inferred) = schema.to_json_schema_with_config(config.enum_threshold, config.min_enum_samples) {
        for (k, v) in inferred {
            doc.insert(k, v);
        }
    }

    let file = File::create(path)?;
    serde_json::to_writer_pretty(file, &Value::Object(doc))?;
    Ok(())
}

/// Write output files for a single agent.
fn write_agent_output(
    agent: &str,
    samples: &HashMap<String, Vec<Value>>,
    counts: &HashMap<String, usize>,
    unparsed: Option<&Vec<String>>,
    content_blocks: Option<&HashMap<String, Vec<Value>>>,
    tool_inputs: Option<&HashMap<String, Vec<Value>>>,
    output_dir: &Path,
    config: &Config,
    source_files: &[PathBuf],
) -> std::io::Result<()> {
    let agent_dir = output_dir.join(agent);
    fs::create_dir_all(&agent_dir)?;

    // Write raw JSONL samples per event type
    if config.emit_raw {
        for (event_type, values) in samples {
            let filename = format!("{}.jsonl", event_type);
            let path = agent_dir.join(&filename);

            if path.exists() && !config.overwrite {
                eprintln!("Skipping existing file: {}", path.display());
                continue;
            }

            let mut file = File::create(&path)?;
            for value in values {
                writeln!(file, "{}", serde_json::to_string(value).unwrap_or_default())?;
            }
        }
    }

    // Write inferred schemas per event type
    if config.emit_schema {
        for (event_type, values) in samples {
            if values.is_empty() {
                continue;
            }

            let filename = format!("{}.schema.json", event_type);
            let path = agent_dir.join(&filename);

            write_schema_file(
                &path,
                &format!("{} {} event", agent, event_type),
                &format!(
                    "Inferred schema for {} agent {} events (from {} samples)",
                    agent, event_type, values.len()
                ),
                values,
                config,
            )?;
        }
    }

    // Write nested content block schemas
    if config.emit_schema && config.emit_nested_schema {
        if let Some(blocks) = content_blocks {
            for (block_type, values) in blocks {
                if values.is_empty() {
                    continue;
                }

                let filename = format!("content_block.{}.schema.json", block_type);
                let path = agent_dir.join(&filename);

                write_schema_file(
                    &path,
                    &format!("{} {} content block", agent, block_type),
                    &format!(
                        "Inferred schema for {} agent {} content blocks (from {} samples)",
                        agent, block_type, values.len()
                    ),
                    values,
                    config,
                )?;
            }
        }

        // Write tool input schemas
        if let Some(tools) = tool_inputs {
            for (tool_name, values) in tools {
                if values.is_empty() {
                    continue;
                }

                let filename = format!("tool_input.{}.schema.json", tool_name);
                let path = agent_dir.join(&filename);

                write_schema_file(
                    &path,
                    &format!("{} {} tool input", agent, tool_name),
                    &format!(
                        "Inferred schema for {} agent {} tool inputs (from {} samples)",
                        agent, tool_name, values.len()
                    ),
                    values,
                    config,
                )?;
            }
        }
    }

    // Write unparsed lines
    if config.emit_unparsed {
        if let Some(lines) = unparsed {
            if !lines.is_empty() {
                let path = agent_dir.join("unparsed.jsonl");
                if !path.exists() || config.overwrite {
                    let mut file = File::create(&path)?;
                    for line in lines {
                        writeln!(file, "{}", line)?;
                    }
                }
            }
        }
    }

    // Write summary
    let summary_path = agent_dir.join("summary.json");
    if !summary_path.exists() || config.overwrite {
        let mut summary = serde_json::Map::new();
        summary.insert("agent".to_string(), Value::String(agent.to_string()));

        // Event counts
        let counts_value: Value = counts
            .iter()
            .map(|(k, v)| (k.clone(), Value::Number((*v as u64).into())))
            .collect::<serde_json::Map<_, _>>()
            .into();
        summary.insert("event_counts".to_string(), counts_value);

        // Total samples stored
        let total_samples: usize = samples.values().map(|v| v.len()).sum();
        summary.insert(
            "total_samples_stored".to_string(),
            Value::Number((total_samples as u64).into()),
        );

        // Add nested schema counts
        if let Some(blocks) = content_blocks {
            let block_counts: Value = blocks
                .iter()
                .map(|(k, v)| (k.clone(), Value::Number((v.len() as u64).into())))
                .collect::<serde_json::Map<_, _>>()
                .into();
            summary.insert("content_block_counts".to_string(), block_counts);
        }

        if let Some(tools) = tool_inputs {
            let tool_counts: Value = tools
                .iter()
                .map(|(k, v)| (k.clone(), Value::Number((v.len() as u64).into())))
                .collect::<serde_json::Map<_, _>>()
                .into();
            summary.insert("tool_input_counts".to_string(), tool_counts);
        }

        // Source files (relative paths if possible)
        let source_list: Vec<Value> = source_files
            .iter()
            .map(|p| Value::String(p.display().to_string()))
            .collect();
        summary.insert("source_files".to_string(), Value::Array(source_list));

        let file = File::create(&summary_path)?;
        serde_json::to_writer_pretty(file, &Value::Object(summary))?;
    }

    Ok(())
}

/// Parse command-line arguments into Config.
fn parse_args() -> Result<Config, String> {
    let mut config = Config::default();
    let args: Vec<String> = env::args().collect();
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--input" | "-i" => {
                i += 1;
                if i >= args.len() {
                    return Err("--input requires a value".to_string());
                }
                config.input_dir = PathBuf::from(&args[i]);
            }
            "--output" | "-o" => {
                i += 1;
                if i >= args.len() {
                    return Err("--output requires a value".to_string());
                }
                config.output_dir = PathBuf::from(&args[i]);
            }
            "--agents" | "-a" => {
                i += 1;
                if i >= args.len() {
                    return Err("--agents requires a value".to_string());
                }
                config.agents_filter = Some(args[i].split(',').map(String::from).collect());
            }
            "--overwrite" => {
                config.overwrite = true;
            }
            "--emit-schema" => {
                config.emit_schema = true;
            }
            "--no-schema" => {
                config.emit_schema = false;
            }
            "--emit-raw" => {
                config.emit_raw = true;
            }
            "--no-raw" => {
                config.emit_raw = false;
            }
            "--emit-unparsed" => {
                config.emit_unparsed = true;
            }
            "--emit-nested-schema" => {
                config.emit_nested_schema = true;
            }
            "--no-nested-schema" => {
                config.emit_nested_schema = false;
            }
            "--emit-coverage" => {
                config.emit_coverage = true;
            }
            "--no-coverage" => {
                config.emit_coverage = false;
            }
            "--enum-threshold" => {
                i += 1;
                if i >= args.len() {
                    return Err("--enum-threshold requires a value".to_string());
                }
                config.enum_threshold = args[i]
                    .parse()
                    .map_err(|_| "Invalid value for --enum-threshold")?;
            }
            "--min-enum-samples" => {
                i += 1;
                if i >= args.len() {
                    return Err("--min-enum-samples requires a value".to_string());
                }
                config.min_enum_samples = args[i]
                    .parse()
                    .map_err(|_| "Invalid value for --min-enum-samples")?;
            }
            "--max-samples" | "-m" => {
                i += 1;
                if i >= args.len() {
                    return Err("--max-samples requires a value".to_string());
                }
                config.max_samples = args[i]
                    .parse()
                    .map_err(|_| "Invalid value for --max-samples")?;
            }
            "--verbose" | "-v" => {
                config.verbose = true;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            arg => {
                return Err(format!("Unknown argument: {}", arg));
            }
        }
        i += 1;
    }

    Ok(config)
}

fn print_help() {
    println!(
        r#"Schema Extraction Tool for agent-cli-runner

USAGE:
    schema_extraction [OPTIONS]

OPTIONS:
    -i, --input <dir>       Input directory to scan (default: current directory)
    -o, --output <dir>      Output directory (default: agent-cli-runner/docs/cli-verification/schemas/)
    -a, --agents <csv>      Filter to specific agents (comma-separated)
    -m, --max-samples <n>   Maximum samples per event type (default: 100)
    --overwrite             Overwrite existing output files
    --emit-schema           Generate JSON Schema files (default: true)
    --no-schema             Skip JSON Schema generation
    --emit-raw              Generate raw JSONL samples (default: true)
    --no-raw                Skip raw JSONL generation
    --emit-unparsed         Save unparsed lines to unparsed.jsonl
    --emit-nested-schema    Generate schemas for content blocks and tool inputs (default: true)
    --no-nested-schema      Skip nested schema generation
    --emit-coverage         Generate coverage report (default: true)
    --no-coverage           Skip coverage report generation
    --enum-threshold <n>    Max distinct values for enum inference (default: 10)
    --min-enum-samples <n>  Min samples required before emitting enum (default: 3)
    -v, --verbose           Enable verbose output
    -h, --help              Show this help message

OUTPUTS:
    <agent>/<event>.schema.json              Schema for each event type
    <agent>/<event>.jsonl                    Raw samples for each event type
    <agent>/content_block.<type>.schema.json Schema for nested content blocks
    <agent>/tool_input.<name>.schema.json    Schema for tool inputs by name
    <agent>/summary.json                     Summary with counts
    coverage.json                            Coverage report (observed vs expected)

EXAMPLES:
    # Scan current directory and output to default location
    schema_extraction

    # Scan specific directory with verbose output
    schema_extraction -i .planning-agent -v

    # Filter to Claude agent only
    schema_extraction -a claude

    # Overwrite existing files with new extraction
    schema_extraction --overwrite
"#
    );
}

/// Expected event types per agent based on parser knowledge.
fn get_expected_event_types(agent: &str) -> Vec<&'static str> {
    match agent {
        "claude" => vec!["system", "assistant", "user", "result"],
        "codex" => vec!["session_start", "message", "exec_result", "session_end"],
        "gemini" => vec!["session_start", "text", "tool_call", "tool_result", "session_end"],
        _ => vec![],
    }
}

/// Expected content block types per agent.
fn get_expected_content_block_types(agent: &str) -> Vec<&'static str> {
    match agent {
        "claude" => vec!["text", "tool_use", "tool_result"],
        "codex" => vec!["text", "function_call"],
        _ => vec![],
    }
}

/// Write coverage report comparing observed vs expected event types.
fn write_coverage_report(collection: &SampleCollection, config: &Config) -> std::io::Result<()> {
    let coverage_path = config.output_dir.join("coverage.json");

    if coverage_path.exists() && !config.overwrite {
        eprintln!("Skipping existing file: {}", coverage_path.display());
        return Ok(());
    }

    let mut coverage = serde_json::Map::new();

    // Per-agent coverage
    let mut agents_coverage = serde_json::Map::new();

    for agent in ["claude", "codex", "gemini"].iter() {
        let expected_events = get_expected_event_types(agent);
        let expected_blocks = get_expected_content_block_types(agent);

        let observed_events: BTreeSet<String> = collection
            .counts
            .get(*agent)
            .map(|c| c.keys().cloned().collect())
            .unwrap_or_default();

        let observed_blocks: BTreeSet<String> = collection
            .content_blocks
            .get(*agent)
            .map(|b| b.keys().cloned().collect())
            .unwrap_or_default();

        let observed_tools: BTreeSet<String> = collection
            .tool_inputs
            .get(*agent)
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default();

        // Calculate missing and unknown
        let expected_event_set: BTreeSet<&str> = expected_events.iter().copied().collect();
        let observed_event_strs: BTreeSet<&str> = observed_events.iter().map(|s| s.as_str()).collect();

        let missing_events: Vec<&str> = expected_event_set
            .difference(&observed_event_strs)
            .copied()
            .collect();

        let unknown_events: Vec<String> = observed_events
            .iter()
            .filter(|e| !expected_event_set.contains(e.as_str()))
            .cloned()
            .collect();

        // Block coverage
        let expected_block_set: BTreeSet<&str> = expected_blocks.iter().copied().collect();
        let observed_block_strs: BTreeSet<&str> = observed_blocks.iter().map(|s| s.as_str()).collect();

        let missing_blocks: Vec<&str> = expected_block_set
            .difference(&observed_block_strs)
            .copied()
            .collect();

        let unknown_blocks: Vec<String> = observed_blocks
            .iter()
            .filter(|b| !expected_block_set.contains(b.as_str()))
            .cloned()
            .collect();

        // Build agent coverage object
        let mut agent_coverage = serde_json::Map::new();

        // Event coverage
        let mut events = serde_json::Map::new();
        events.insert(
            "expected".to_string(),
            Value::Array(expected_events.iter().map(|s| Value::String(s.to_string())).collect()),
        );
        events.insert(
            "observed".to_string(),
            Value::Array(observed_events.iter().map(|s| Value::String(s.clone())).collect()),
        );
        events.insert(
            "missing".to_string(),
            Value::Array(missing_events.iter().map(|s| Value::String(s.to_string())).collect()),
        );
        events.insert(
            "unknown".to_string(),
            Value::Array(unknown_events.iter().map(|s| Value::String(s.clone())).collect()),
        );

        // Sample counts per event
        let sample_counts: Value = collection
            .counts
            .get(*agent)
            .map(|c| {
                c.iter()
                    .map(|(k, v)| (k.clone(), Value::Number((*v as u64).into())))
                    .collect::<serde_json::Map<_, _>>()
                    .into()
            })
            .unwrap_or(Value::Object(serde_json::Map::new()));
        events.insert("sample_counts".to_string(), sample_counts);

        agent_coverage.insert("events".to_string(), Value::Object(events));

        // Content block coverage
        let mut blocks = serde_json::Map::new();
        blocks.insert(
            "expected".to_string(),
            Value::Array(expected_blocks.iter().map(|s| Value::String(s.to_string())).collect()),
        );
        blocks.insert(
            "observed".to_string(),
            Value::Array(observed_blocks.iter().map(|s| Value::String(s.clone())).collect()),
        );
        blocks.insert(
            "missing".to_string(),
            Value::Array(missing_blocks.iter().map(|s| Value::String(s.to_string())).collect()),
        );
        blocks.insert(
            "unknown".to_string(),
            Value::Array(unknown_blocks.iter().map(|s| Value::String(s.clone())).collect()),
        );

        // Block sample counts
        let block_counts: Value = collection
            .content_blocks
            .get(*agent)
            .map(|b| {
                b.iter()
                    .map(|(k, v)| (k.clone(), Value::Number((v.len() as u64).into())))
                    .collect::<serde_json::Map<_, _>>()
                    .into()
            })
            .unwrap_or(Value::Object(serde_json::Map::new()));
        blocks.insert("sample_counts".to_string(), block_counts);

        agent_coverage.insert("content_blocks".to_string(), Value::Object(blocks));

        // Tool inputs
        let mut tools = serde_json::Map::new();
        tools.insert(
            "observed".to_string(),
            Value::Array(observed_tools.iter().map(|s| Value::String(s.clone())).collect()),
        );

        // Tool sample counts
        let tool_counts: Value = collection
            .tool_inputs
            .get(*agent)
            .map(|t| {
                t.iter()
                    .map(|(k, v)| (k.clone(), Value::Number((v.len() as u64).into())))
                    .collect::<serde_json::Map<_, _>>()
                    .into()
            })
            .unwrap_or(Value::Object(serde_json::Map::new()));
        tools.insert("sample_counts".to_string(), tool_counts);

        agent_coverage.insert("tool_inputs".to_string(), Value::Object(tools));

        agents_coverage.insert(agent.to_string(), Value::Object(agent_coverage));
    }

    coverage.insert("agents".to_string(), Value::Object(agents_coverage));

    // Global summary
    let mut summary = serde_json::Map::new();
    summary.insert(
        "total_agents_with_data".to_string(),
        Value::Number((collection.samples.len() as u64).into()),
    );
    summary.insert(
        "source_files_count".to_string(),
        Value::Number((collection.source_files.len() as u64).into()),
    );
    coverage.insert("summary".to_string(), Value::Object(summary));

    let file = File::create(&coverage_path)?;
    serde_json::to_writer_pretty(file, &Value::Object(coverage))?;

    Ok(())
}

fn main() {
    let config = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {}", e);
            eprintln!("Use --help for usage information");
            std::process::exit(1);
        }
    };

    if config.verbose {
        eprintln!("Input directory: {}", config.input_dir.display());
        eprintln!("Output directory: {}", config.output_dir.display());
    }

    // Find all log files
    let mut log_files = Vec::new();
    if let Err(e) = find_log_files(&config.input_dir, &mut log_files) {
        eprintln!("Error scanning directory: {}", e);
        std::process::exit(1);
    }

    if log_files.is_empty() {
        eprintln!("No log files found in {}", config.input_dir.display());
        std::process::exit(0);
    }

    if config.verbose {
        eprintln!("Found {} log files", log_files.len());
    }

    // Process all files
    let mut collection = SampleCollection::new();
    let mut total_stats = FileStats::default();

    for (path, format) in &log_files {
        if config.verbose {
            eprintln!("Processing: {}", path.display());
        }

        match process_log_file(path, format, &mut collection, &config) {
            Ok(stats) => {
                total_stats.total_lines += stats.total_lines;
                total_stats.stdout_lines += stats.stdout_lines;
                total_stats.json_parsed += stats.json_parsed;
                total_stats.json_failed += stats.json_failed;
            }
            Err(e) => {
                eprintln!("Error processing {}: {}", path.display(), e);
            }
        }
    }

    // Print summary
    println!("Processed {} files", log_files.len());
    println!("  Total lines: {}", total_stats.total_lines);
    println!("  Stdout lines: {}", total_stats.stdout_lines);
    println!("  JSON parsed: {}", total_stats.json_parsed);
    println!("  JSON failed: {}", total_stats.json_failed);

    // Create output directory
    if let Err(e) = fs::create_dir_all(&config.output_dir) {
        eprintln!("Error creating output directory: {}", e);
        std::process::exit(1);
    }

    // Write output for each agent
    for (agent, samples) in &collection.samples {
        let counts = collection.counts.get(agent).cloned().unwrap_or_default();
        let unparsed = collection.unparsed.get(agent);
        let content_blocks = collection.content_blocks.get(agent);
        let tool_inputs = collection.tool_inputs.get(agent);

        println!("\nAgent: {}", agent);
        for (event_type, count) in &counts {
            let stored = samples.get(event_type).map(|v| v.len()).unwrap_or(0);
            println!("  {}: {} total, {} stored", event_type, count, stored);
        }

        // Print nested schema info
        if let Some(blocks) = content_blocks {
            println!("  Content blocks:");
            for (block_type, values) in blocks {
                println!("    {}: {} samples", block_type, values.len());
            }
        }
        if let Some(tools) = tool_inputs {
            println!("  Tool inputs:");
            for (tool_name, values) in tools {
                println!("    {}: {} samples", tool_name, values.len());
            }
        }

        if let Err(e) = write_agent_output(
            agent,
            samples,
            &counts,
            unparsed,
            content_blocks,
            tool_inputs,
            &config.output_dir,
            &config,
            &collection.source_files,
        ) {
            eprintln!("Error writing output for {}: {}", agent, e);
        }
    }

    // Write coverage report
    if config.emit_coverage {
        if let Err(e) = write_coverage_report(&collection, &config) {
            eprintln!("Error writing coverage report: {}", e);
        }
    }

    println!("\nOutput written to: {}", config.output_dir.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_new_format() {
        let line = r#"[02:24:08.467][claude][stdout] {"type":"system"}"#;
        let parsed = parse_new_format(line).unwrap();
        assert_eq!(parsed.agent, "claude");
        assert_eq!(parsed.kind, "stdout");
        assert_eq!(parsed.payload, r#"{"type":"system"}"#);
    }

    #[test]
    fn test_parse_new_format_with_hyphen_agent() {
        let line = r#"[12:00:00.000][claude-3][stdout] {"type":"test"}"#;
        let parsed = parse_new_format(line).unwrap();
        assert_eq!(parsed.agent, "claude-3");
        assert_eq!(parsed.kind, "stdout");
    }

    #[test]
    fn test_parse_new_format_with_underscore_agent() {
        let line = r#"[12:00:00.000][my_agent][stdout] {"type":"test"}"#;
        let parsed = parse_new_format(line).unwrap();
        assert_eq!(parsed.agent, "my_agent");
    }

    #[test]
    fn test_parse_new_format_start_line() {
        let line = r#"[02:24:08.467][claude][start] command: claude -p"#;
        let parsed = parse_new_format(line).unwrap();
        assert_eq!(parsed.agent, "claude");
        assert_eq!(parsed.kind, "start");
        assert_eq!(parsed.payload, "command: claude -p");
    }

    #[test]
    fn test_parse_new_format_invalid() {
        assert!(parse_new_format("not a log line").is_none());
        assert!(parse_new_format("[only one bracket]").is_none());
        assert!(parse_new_format("[time][agent] no kind").is_none());
    }

    #[test]
    fn test_parse_legacy_format() {
        let line = r#"[stdout] {"type":"system"}"#;
        let parsed = parse_legacy_format(line, "claude").unwrap();
        assert_eq!(parsed.agent, "claude");
        assert_eq!(parsed.kind, "stdout");
        assert_eq!(parsed.payload, r#"{"type":"system"}"#);
    }

    #[test]
    fn test_parse_legacy_format_invalid() {
        assert!(parse_legacy_format("not a log line", "claude").is_none());
    }

    #[test]
    fn test_agent_from_filename() {
        assert_eq!(
            agent_from_filename("claude-stream-20251222-024235.log"),
            Some("claude".to_string())
        );
        assert_eq!(
            agent_from_filename("codex-stream-123.log"),
            Some("codex".to_string())
        );
        assert_eq!(agent_from_filename("agent-stream-123.log"), Some("agent".to_string()));
        assert_eq!(agent_from_filename("workflow.log"), None);
    }

    #[test]
    fn test_detect_log_format() {
        assert!(matches!(
            detect_log_format("agent-stream-20251223-022408.log"),
            Some(LogFormat::New)
        ));
        assert!(matches!(
            detect_log_format("claude-stream-20251222-024235.log"),
            Some(LogFormat::Legacy(agent)) if agent == "claude"
        ));
        assert!(detect_log_format("workflow.log").is_none());
    }

    #[test]
    fn test_get_event_discriminator() {
        let claude_json: Value = serde_json::from_str(r#"{"type":"assistant"}"#).unwrap();
        assert_eq!(get_event_discriminator("claude", &claude_json), "assistant");

        let codex_json: Value = serde_json::from_str(r#"{"event":"session_start"}"#).unwrap();
        assert_eq!(get_event_discriminator("codex", &codex_json), "session_start");

        let gemini_json: Value = serde_json::from_str(r#"{"type":"text"}"#).unwrap();
        assert_eq!(get_event_discriminator("gemini", &gemini_json), "text");

        let unknown_json: Value = serde_json::from_str(r#"{"foo":"bar"}"#).unwrap();
        assert_eq!(get_event_discriminator("claude", &unknown_json), "unknown");
    }

    #[test]
    fn test_infer_schema_primitives() {
        let null_schema = infer_schema(&Value::Null);
        assert!(null_schema.types.contains("null"));

        let bool_schema = infer_schema(&Value::Bool(true));
        assert!(bool_schema.types.contains("boolean"));

        let num_schema = infer_schema(&Value::Number(42.into()));
        assert!(num_schema.types.contains("number"));

        let str_schema = infer_schema(&Value::String("test".to_string()));
        assert!(str_schema.types.contains("string"));
    }

    #[test]
    fn test_infer_schema_object() {
        let json: Value = serde_json::from_str(r#"{"name":"test","count":42}"#).unwrap();
        let schema = infer_schema(&json);

        assert!(schema.types.contains("object"));
        assert!(schema.properties.contains_key("name"));
        assert!(schema.properties.contains_key("count"));
        assert!(schema.required.contains("name"));
        assert!(schema.required.contains("count"));
    }

    #[test]
    fn test_infer_schema_array() {
        let json: Value = serde_json::from_str(r#"[1, 2, 3]"#).unwrap();
        let schema = infer_schema(&json);

        assert!(schema.types.contains("array"));
        assert!(schema.items.is_some());
        let items = schema.items.as_ref().unwrap();
        assert!(items.types.contains("number"));
    }

    #[test]
    fn test_schema_merge() {
        let json1: Value = serde_json::from_str(r#"{"a":1,"b":"x"}"#).unwrap();
        let json2: Value = serde_json::from_str(r#"{"a":2,"c":true}"#).unwrap();

        let mut schema = infer_schema(&json1);
        schema.merge(&infer_schema(&json2));

        assert!(schema.properties.contains_key("a"));
        assert!(schema.properties.contains_key("b"));
        assert!(schema.properties.contains_key("c"));
        // Only "a" is required (present in both)
        assert!(schema.required.contains("a"));
        assert!(!schema.required.contains("b"));
        assert!(!schema.required.contains("c"));
    }

    #[test]
    fn test_schema_to_json_schema() {
        let json: Value = serde_json::from_str(r#"{"type":"test","count":42}"#).unwrap();
        let schema = infer_schema(&json);
        let json_schema = schema.to_json_schema_with_config(DEFAULT_ENUM_THRESHOLD, DEFAULT_MIN_ENUM_SAMPLES);

        assert!(json_schema.get("type").is_some());
        assert!(json_schema.get("properties").is_some());
        assert!(json_schema.get("required").is_some());
    }

    #[test]
    fn test_should_skip_dir() {
        assert!(should_skip_dir("target"));
        assert!(should_skip_dir(".git"));
        assert!(should_skip_dir("node_modules"));
        assert!(!should_skip_dir(".planning-agent"));
        assert!(!should_skip_dir("src"));
    }

    #[test]
    fn test_infer_schema_integer_detection() {
        // Integer values should be detected
        let json: Value = serde_json::from_str(r#"42"#).unwrap();
        let schema = infer_schema(&json);

        assert!(schema.types.contains("number"));
        assert!(schema.numeric_info.all_integer);
        assert_eq!(schema.numeric_info.count, 1);

        // Merge with another integer should stay as integer
        let json2: Value = serde_json::from_str(r#"100"#).unwrap();
        let mut merged = schema.clone();
        merged.merge(&infer_schema(&json2));
        assert!(merged.numeric_info.all_integer);
    }

    #[test]
    fn test_infer_schema_float_detection() {
        // Float values should NOT be marked as all_integer
        let json: Value = serde_json::from_str(r#"3.14"#).unwrap();
        let schema = infer_schema(&json);

        assert!(schema.types.contains("number"));
        assert!(!schema.numeric_info.all_integer);
    }

    #[test]
    fn test_infer_schema_mixed_numeric() {
        // Mixing integer and float should result in not all_integer
        let int_json: Value = serde_json::from_str(r#"42"#).unwrap();
        let float_json: Value = serde_json::from_str(r#"3.14"#).unwrap();

        let mut schema = infer_schema(&int_json);
        schema.merge(&infer_schema(&float_json));

        assert!(schema.types.contains("number"));
        assert!(!schema.numeric_info.all_integer);
    }

    #[test]
    fn test_infer_schema_string_values_tracking() {
        // String values should be tracked for enum inference
        let json: Value = serde_json::from_str(r#""hello""#).unwrap();
        let schema = infer_schema(&json);

        assert!(schema.types.contains("string"));
        assert!(schema.string_values.contains("hello"));
    }

    #[test]
    fn test_schema_enum_inference() {
        // With few distinct values, enum should be emitted
        let mut schema = SchemaNode::new();
        schema.types.insert("string".to_string());
        schema.string_values.insert("a".to_string());
        schema.string_values.insert("b".to_string());
        schema.string_values.insert("c".to_string());
        schema.seen_count = 5; // More than min_enum_samples

        let json_schema = schema.to_json_schema_with_config(10, 3);

        // Should have enum
        let enum_values = json_schema.get("enum");
        assert!(enum_values.is_some(), "Should have enum field");
        let enum_arr = enum_values.unwrap().as_array().unwrap();
        assert_eq!(enum_arr.len(), 3);
    }

    #[test]
    fn test_schema_no_enum_when_too_many_values() {
        // With many distinct values, no enum should be emitted
        let mut schema = SchemaNode::new();
        schema.types.insert("string".to_string());
        for i in 0..15 {
            schema.string_values.insert(format!("value_{}", i));
        }
        schema.seen_count = 20;

        let json_schema = schema.to_json_schema_with_config(10, 3); // threshold is 10

        // Should NOT have enum (15 values > 10 threshold)
        assert!(json_schema.get("enum").is_none(), "Should not have enum when values exceed threshold");
    }

    #[test]
    fn test_schema_no_enum_when_too_few_samples() {
        // With few samples, enum should not be emitted to avoid overfitting
        let mut schema = SchemaNode::new();
        schema.types.insert("string".to_string());
        schema.string_values.insert("a".to_string());
        schema.string_values.insert("b".to_string());
        schema.seen_count = 2; // Less than min_enum_samples (3)

        let json_schema = schema.to_json_schema_with_config(10, 3);

        // Should NOT have enum (2 samples < 3 required)
        assert!(json_schema.get("enum").is_none(), "Should not have enum when samples below minimum");
    }

    #[test]
    fn test_schema_integer_type_in_output() {
        // When all numbers are integers, output should say "integer" not "number"
        let mut schema = SchemaNode::new();
        schema.types.insert("number".to_string());
        schema.numeric_info.all_integer = true;
        schema.numeric_info.count = 5;
        schema.seen_count = 5;

        let json_schema = schema.to_json_schema_with_config(10, 3);

        assert_eq!(
            json_schema.get("type").and_then(|v| v.as_str()),
            Some("integer"),
            "Should emit 'integer' when all numbers are integers"
        );
    }

    #[test]
    fn test_schema_union_with_object_and_null() {
        // When we have object + null, object should include properties in anyOf
        let mut schema = SchemaNode::new();
        schema.types.insert("object".to_string());
        schema.types.insert("null".to_string());
        schema.properties.insert("name".to_string(), {
            let mut prop = SchemaNode::new();
            prop.types.insert("string".to_string());
            prop.seen_count = 1;
            prop
        });
        schema.required.insert("name".to_string());
        schema.seen_count = 2;

        let json_schema = schema.to_json_schema_with_config(10, 3);

        // Should have anyOf
        let any_of = json_schema.get("anyOf");
        assert!(any_of.is_some(), "Should have anyOf for object + null");

        let any_of_arr = any_of.unwrap().as_array().unwrap();
        assert_eq!(any_of_arr.len(), 2);

        // The object variant in anyOf should include properties
        let object_variant = any_of_arr.iter().find(|v| {
            v.get("type").and_then(|t| t.as_str()) == Some("object")
        });
        assert!(object_variant.is_some(), "Should have object variant");
        assert!(
            object_variant.unwrap().get("properties").is_some(),
            "Object variant should include properties"
        );

        // Top-level should NOT have properties (since it's a union with non-object)
        assert!(
            json_schema.get("properties").is_none(),
            "Top-level should not have properties when union includes non-objects"
        );
    }

    #[test]
    fn test_expected_event_types() {
        // Verify expected event types are correctly defined
        let claude_events = get_expected_event_types("claude");
        assert!(claude_events.contains(&"system"));
        assert!(claude_events.contains(&"assistant"));
        assert!(claude_events.contains(&"user"));
        assert!(claude_events.contains(&"result"));

        let codex_events = get_expected_event_types("codex");
        assert!(codex_events.contains(&"session_start"));
        assert!(codex_events.contains(&"message"));

        let gemini_events = get_expected_event_types("gemini");
        assert!(gemini_events.contains(&"tool_call"));
        assert!(gemini_events.contains(&"tool_result"));
    }

    #[test]
    fn test_expected_content_block_types() {
        let claude_blocks = get_expected_content_block_types("claude");
        assert!(claude_blocks.contains(&"text"));
        assert!(claude_blocks.contains(&"tool_use"));
        assert!(claude_blocks.contains(&"tool_result"));

        let codex_blocks = get_expected_content_block_types("codex");
        assert!(codex_blocks.contains(&"text"));
        assert!(codex_blocks.contains(&"function_call"));

        // Unknown agent should return empty
        let unknown_blocks = get_expected_content_block_types("unknown");
        assert!(unknown_blocks.is_empty());
    }
}
