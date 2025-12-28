#![allow(clippy::panic, clippy::branches_sharing_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_LINES: usize = 500;
const MAX_FILES_PER_FOLDER: usize = 10;
const EXCLUDED_DIRS: &[&str] = &["target", ".git", "node_modules", ".planning-agent"];
const FOLDER_LIMIT_DIRS: &[&str] = &["src", "tests"];

fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads/main");
    println!("cargo:rerun-if-changed=.git/packed-refs");
    enforce_policies();
}

fn enforce_policies() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let root = PathBuf::from(&manifest_dir);
    let files = collect_rs_files(&root);
    for file in &files {
        println!("cargo:rerun-if-changed={}", file.display());
    }
    strip_comments_from_files(&root, &files);
    enforce_line_limits(&root, &files);
    enforce_folder_limits(&root);
}

fn strip_comments_from_files(root: &Path, files: &[PathBuf]) {
    for file in files {
        if let Err(e) = strip_comments_from_file(file) {
            let rel_path = file.strip_prefix(root).unwrap_or(file);
            println!("cargo:warning=Could not strip comments from {}: {e}", rel_path.display());
        }
    }
}

fn strip_comments_from_file(path: &Path) -> std::io::Result<()> {
    let content = std::fs::read_to_string(path)?;
    let stripped = strip_rust_comments(&content);
    if stripped != content {
        std::fs::write(path, &stripped)?;
        println!("cargo:warning=Stripped comments from {}", path.display());
    }
    Ok(())
}

fn collect_rs_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(output) = Command::new("git").args(["ls-files"]).current_dir(root).output() {
        if output.status.success() {
            if let Ok(stdout) = String::from_utf8(output.stdout) {
                for line in stdout.lines() {
                    let path = root.join(line);
                    if should_check_file(&path, root) {
                        files.push(path);
                    }
                }
                return files;
            }
        }
    }
    walk_directory(root, root, &mut files);
    files
}

fn walk_directory(dir: &Path, root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if EXCLUDED_DIRS.contains(&name) {
                    continue;
                }
            }
            walk_directory(&path, root, files);
        } else if should_check_file(&path, root) {
            files.push(path);
        }
    }
}

fn should_check_file(path: &Path, root: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    if ext != "rs" {
        return false;
    }
    if let Ok(rel_path) = path.strip_prefix(root) {
        for component in rel_path.components() {
            if let Some(name) = component.as_os_str().to_str() {
                if EXCLUDED_DIRS.contains(&name) {
                    return false;
                }
            }
        }
    }
    true
}

fn enforce_line_limits(root: &Path, files: &[PathBuf]) {
    let mut violations = Vec::new();
    for file in files {
        match count_lines(file) {
            Ok(line_count) if line_count > MAX_LINES => {
                let rel_path = file.strip_prefix(root).unwrap_or(file);
                violations.push((rel_path.to_path_buf(), line_count));
            }
            Ok(_) => {}
            Err(e) => {
                let rel_path = file.strip_prefix(root).unwrap_or(file);
                println!("cargo:warning=Could not read file {}: {e}", rel_path.display());
            }
        }
    }
    if !violations.is_empty() {
        eprintln!("\n========================================");
        eprintln!("FILE LINE LIMIT EXCEEDED (max {MAX_LINES} lines)");
        eprintln!("========================================");
        for (path, lines) in &violations {
            eprintln!(
                "  {} - {} lines (exceeds by {})",
                path.display(),
                lines,
                lines - MAX_LINES
            );
        }
        eprintln!("========================================\n");
        eprintln!("Please split these files into smaller modules.\n");
        panic!(
            "Build failed: {} file(s) exceed the {MAX_LINES} line limit",
            violations.len()
        );
    }
}

fn enforce_folder_limits(root: &Path) {
    let mut folder_counts: HashMap<PathBuf, usize> = HashMap::new();
    for dir_name in FOLDER_LIMIT_DIRS {
        let dir = root.join(dir_name);
        if dir.is_dir() {
            count_files_in_folders(&dir, &mut folder_counts);
        }
    }
    let mut violations: Vec<(PathBuf, usize)> = folder_counts
        .into_iter()
        .filter(|(_, count)| *count > MAX_FILES_PER_FOLDER)
        .collect();
    violations.sort_by(|a, b| a.0.cmp(&b.0));
    if !violations.is_empty() {
        eprintln!("\n========================================");
        eprintln!("FOLDER FILE LIMIT EXCEEDED (max {MAX_FILES_PER_FOLDER} files)");
        eprintln!("========================================");
        for (path, count) in &violations {
            let rel = path.strip_prefix(root).unwrap_or(path);
            eprintln!(
                "  {} - {} files (exceeds by {})",
                rel.display(),
                count,
                count - MAX_FILES_PER_FOLDER
            );
        }
        eprintln!("========================================\n");
        eprintln!("Please organize files into subfolders.\n");
        panic!(
            "Build failed: {} folder(s) exceed the {MAX_FILES_PER_FOLDER} file limit",
            violations.len()
        );
    }
}

fn count_files_in_folders(dir: &Path, counts: &mut HashMap<PathBuf, usize>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut file_count = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if !EXCLUDED_DIRS.contains(&name) {
                    count_files_in_folders(&path, counts);
                }
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            file_count += 1;
        }
    }
    if file_count > 0 {
        counts.insert(dir.to_path_buf(), file_count);
    }
}

fn count_lines(path: &Path) -> std::io::Result<usize> {
    let content = std::fs::read_to_string(path)?;
    let stripped = strip_rust_comments(&content);
    Ok(count_non_empty_lines(&stripped))
}

fn count_non_empty_lines(content: &str) -> usize {
    content.lines().filter(|line| !line.trim().is_empty()).count()
}

#[allow(clippy::too_many_lines)]
fn strip_rust_comments(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let chars: Vec<char> = content.chars().collect();
    let len = chars.len();
    let mut i = 0;
    while i < len {
        if chars[i] == 'b' && i + 1 < len {
            if chars[i + 1] == '"' {
                result.push(chars[i]);
                i += 1;
                i = handle_regular_string(&chars, i, len, &mut result);
                continue;
            } else if chars[i + 1] == 'r' {
                result.push(chars[i]);
                i += 1;
                i = handle_raw_string(&chars, i, len, &mut result);
                continue;
            }
        }
        if chars[i] == 'r' && i + 1 < len && (chars[i + 1] == '"' || chars[i + 1] == '#') {
            i = handle_raw_string(&chars, i, len, &mut result);
            continue;
        }
        if chars[i] == '"' {
            i = handle_regular_string(&chars, i, len, &mut result);
            continue;
        }
        if chars[i] == '\'' {
            i = handle_char_literal(&chars, i, len, &mut result);
            continue;
        }
        if chars[i] == '/' && i + 1 < len && chars[i + 1] == '/' {
            if i + 2 < len && (chars[i + 2] == '/' || chars[i + 2] == '!') {
                i = preserve_line_comment(&chars, i, len, &mut result);
            } else {
                i = skip_line_comment(&chars, i, len, &mut result);
            }
            continue;
        }
        if chars[i] == '/' && i + 1 < len && chars[i + 1] == '*' {
            let is_doc_comment = if i + 2 < len {
                if chars[i + 2] == '!' {
                    true
                } else if chars[i + 2] == '*' {
                    i + 3 < len && chars[i + 3] != '/'
                } else {
                    false
                }
            } else {
                false
            };
            if is_doc_comment {
                i = preserve_block_comment(&chars, i, len, &mut result);
            } else {
                i = skip_block_comment(&chars, i, len, &mut result);
            }
            continue;
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

fn handle_raw_string(chars: &[char], start: usize, len: usize, result: &mut String) -> usize {
    let mut i = start + 1;
    let mut hash_count = 0;
    while i < len && chars[i] == '#' {
        hash_count += 1;
        i += 1;
    }
    if i < len && chars[i] == '"' {
        result.push_str(&chars[start..=i].iter().collect::<String>());
        i += 1;
        while i < len {
            if chars[i] == '"' {
                let close_start = i;
                i += 1;
                let mut close_hashes = 0;
                while i < len && chars[i] == '#' && close_hashes < hash_count {
                    close_hashes += 1;
                    i += 1;
                }
                result.push_str(&chars[close_start..i].iter().collect::<String>());
                if close_hashes == hash_count {
                    break;
                }
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }
    } else {
        result.push_str(&chars[start..i].iter().collect::<String>());
    }
    i
}

fn handle_regular_string(chars: &[char], start: usize, len: usize, result: &mut String) -> usize {
    result.push(chars[start]);
    let mut i = start + 1;
    while i < len {
        if chars[i] == '\\' && i + 1 < len {
            result.push(chars[i]);
            result.push(chars[i + 1]);
            i += 2;
        } else if chars[i] == '"' {
            result.push(chars[i]);
            i += 1;
            break;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    i
}

fn handle_char_literal(chars: &[char], start: usize, len: usize, result: &mut String) -> usize {
    result.push(chars[start]);
    let mut i = start + 1;
    if i < len {
        if chars[i] == '\\' && i + 1 < len {
            result.push(chars[i]);
            result.push(chars[i + 1]);
            i += 2;
        } else {
            result.push(chars[i]);
            i += 1;
        }
        if i < len && chars[i] == '\'' {
            result.push(chars[i]);
            i += 1;
        }
    }
    i
}

fn preserve_line_comment(chars: &[char], start: usize, len: usize, result: &mut String) -> usize {
    let mut i = start;
    while i < len && chars[i] != '\n' {
        result.push(chars[i]);
        i += 1;
    }
    if i < len {
        result.push('\n');
        i += 1;
    }
    i
}

fn skip_line_comment(chars: &[char], start: usize, len: usize, result: &mut String) -> usize {
    let mut i = start;
    while i < len && chars[i] != '\n' {
        i += 1;
    }
    if i < len {
        result.push('\n');
        i += 1;
    }
    i
}

fn skip_block_comment(chars: &[char], start: usize, len: usize, result: &mut String) -> usize {
    let mut i = start + 2;
    let mut depth = 1;
    while i < len && depth > 0 {
        if chars[i] == '/' && i + 1 < len && chars[i + 1] == '*' {
            depth += 1;
            i += 2;
        } else if chars[i] == '*' && i + 1 < len && chars[i + 1] == '/' {
            depth -= 1;
            i += 2;
        } else {
            if chars[i] == '\n' {
                result.push('\n');
            }
            i += 1;
        }
    }
    i
}

fn preserve_block_comment(chars: &[char], start: usize, len: usize, result: &mut String) -> usize {
    let mut i = start;
    let mut depth = 0;
    while i < len {
        if chars[i] == '/' && i + 1 < len && chars[i + 1] == '*' {
            result.push(chars[i]);
            result.push(chars[i + 1]);
            depth += 1;
            i += 2;
        } else if chars[i] == '*' && i + 1 < len && chars[i + 1] == '/' {
            result.push(chars[i]);
            result.push(chars[i + 1]);
            depth -= 1;
            i += 2;
            if depth == 0 {
                break;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    i
}
