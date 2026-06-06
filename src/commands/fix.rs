//! Fix command: parse errors and apply common automated fixes.

use crate::{CommandResult, FleetState, RepoState, exit_codes, is_tty, resolve_repo_path};
use anyhow::Result;
use colored::Colorize;
use std::path::Path;

/// A fix that can be automatically applied.
#[derive(Debug, Clone)]
pub struct AutoFix {
    pub description: String,
    pub file: String,
    pub action: FixAction,
}

#[derive(Debug, Clone)]
pub enum FixAction {
    /// Add a derive macro: #[derive(...)]
    AddDerive { derive: String, struct_name: String },
    /// Add an import: use ...;
    AddImport { path: String, file: String },
    /// Fix a type mismatch
    FixType { file: String, old: String, new: String },
}

pub fn run(repo_path: &str, errors: Option<&str>, state: &mut FleetState) -> Result<CommandResult> {
    let resolved = resolve_repo_path(repo_path);
    let repo_str = resolved.to_string_lossy().to_string();

    if !resolved.exists() {
        return Ok(CommandResult::fail(
            "fix",
            Some(repo_str),
            format!("Path does not exist: {}", resolved.display()),
            exit_codes::BUILD_ERROR,
        ));
    }

    if is_tty() {
        println!("{} Analyzing errors in {}", "🔧".to_string().blue().bold(), repo_str.cyan());
    }

    // Get error text: from argument, or from last build state
    let error_text = match errors {
        Some(e) => e.to_string(),
        None => {
            // Try to get from last cargo test output
            let output = std::process::Command::new("cargo")
                .args(["test", "--all", "--no-run"])
                .current_dir(&resolved)
                .env("TERM", "dumb")
                .output();
            match output {
                Ok(o) => {
                    let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                    format!("{}\n{}", stdout, stderr)
                }
                Err(_) => {
                    return Ok(CommandResult::fail(
                        "fix",
                        Some(repo_str.clone()),
                        "No error output available. Run 'fleet-build build' first or pass --errors".to_string(),
                        exit_codes::BUILD_ERROR,
                    ));
                }
            }
        }
    };

    // Parse errors and generate fixes
    let fixes = parse_errors(&error_text);

    if fixes.is_empty() {
        if is_tty() {
            println!("  {} No auto-fixable errors found", "ℹ️".to_string().yellow());
        }
        return Ok(CommandResult::ok(
            "fix",
            Some(repo_str),
            "No auto-fixable errors found",
        ));
    }

    // Apply fixes
    let mut applied = 0;
    let mut failed = 0;
    for fix in &fixes {
        match apply_fix(&resolved, fix) {
            Ok(true) => {
                applied += 1;
                if is_tty() {
                    println!("  {} Applied: {}", "✓".green(), fix.description);
                }
            }
            Ok(false) => {
                // Fix not applicable
            }
            Err(e) => {
                failed += 1;
                if is_tty() {
                    println!("  {} Failed: {} — {}", "✗".red(), fix.description, e);
                }
            }
        }
    }

    // Update state
    let existing = state.get_repo(&repo_str);
    let repo_state = RepoState {
        path: repo_str.clone(),
        last_build: existing.and_then(|r| r.last_build.clone()),
        last_test_pass: existing.and_then(|r| r.last_test_pass),
        last_push: existing.and_then(|r| r.last_push.clone()),
        error_count: existing.map(|r| r.error_count).unwrap_or(0),
        fix_count: existing.map(|r| r.fix_count).unwrap_or(0) + applied as u32,
    };
    state.upsert_repo(repo_state);

    let message = format!("Applied {}/{} fixes ({} failed)", applied, fixes.len(), failed);
    let details = serde_json::json!({
        "applied": applied,
        "total_fixes": fixes.len(),
        "failed": failed,
        "fixes": fixes.iter().map(|f| &f.description).collect::<Vec<_>>(),
    });

    Ok(CommandResult::ok("fix", Some(repo_str), message).with_details(details))
}

/// Parse error output and return a list of auto-fixable issues.
pub fn parse_errors(errors: &str) -> Vec<AutoFix> {
    let mut fixes = Vec::new();

    for line in errors.lines() {
        // Missing derive: "the trait `Debug` is not implemented for `Foo`"
        if let Some(fix) = parse_missing_derive(line) {
            fixes.push(fix);
            continue;
        }

        // Missing derive: "`Foo` doesn't implement `std::fmt::Display`"
        if let Some(fix) = parse_missing_display(line) {
            fixes.push(fix);
            continue;
        }

        // Unused import: "unused import: `foo`"
        if let Some(fix) = parse_unused_import(line) {
            fixes.push(fix);
            continue;
        }

        // Missing import pattern: "cannot find type `Serialize` in scope"
        if let Some(fix) = parse_missing_type(line) {
            fixes.push(fix);
            continue;
        }

        // E0599: no method named `foo` found for struct `Bar`
        if let Some(fix) = parse_missing_method(line) {
            fixes.push(fix);
            continue;
        }
    }

    fixes
}

fn parse_missing_derive(line: &str) -> Option<AutoFix> {
    let re = regex::Regex::new(r"the trait `(\w+)` is not implemented for `(\w+)`").ok()?;
    let caps = re.captures(line)?;
    let trait_name = caps.get(1)?.as_str();
    let struct_name = caps.get(2)?.as_str();

    // Extract file from line prefix: "src/foo.rs:10:5"
    let file = extract_file(line).unwrap_or_else(|| "src/lib.rs".to_string());

    Some(AutoFix {
        description: format!("Add #[derive({})] to {}", trait_name, struct_name),
        file,
        action: FixAction::AddDerive {
            derive: trait_name.to_string(),
            struct_name: struct_name.to_string(),
        },
    })
}

fn parse_missing_display(line: &str) -> Option<AutoFix> {
    if line.contains("doesn't implement") && line.contains("std::fmt::Display") {
        let re = regex::Regex::new(r"`(\w+)` doesn't implement").ok()?;
        let caps = re.captures(line)?;
        let struct_name = caps.get(1)?.as_str();
        let file = extract_file(line).unwrap_or_else(|| "src/lib.rs".to_string());
        Some(AutoFix {
            description: format!("Add #[derive(Display)] or impl Display for {}", struct_name),
            file,
            action: FixAction::AddDerive {
                derive: "Display".into(),
                struct_name: struct_name.into(),
            },
        })
    } else {
        None
    }
}

fn parse_unused_import(line: &str) -> Option<AutoFix> {
    if line.contains("unused import:") {
        let file = extract_file(line).unwrap_or_else(|| "src/lib.rs".to_string());
        Some(AutoFix {
            description: format!("Remove unused import in {}", file),
            file: file.clone(),
            action: FixAction::AddImport {
                path: "// remove unused".to_string(),
                file,
            },
        })
    } else {
        None
    }
}

fn parse_missing_type(line: &str) -> Option<AutoFix> {
    let re = regex::Regex::new(r"cannot find (?:type|function|module) `(\w+)` in scope").ok()?;
    let caps = re.captures(line)?;
    let type_name = caps.get(1)?.as_str();
    let file = extract_file(line).unwrap_or_else(|| "src/lib.rs".to_string());

    // Suggest common imports
    let import = match type_name {
        "Serialize" | "Deserialize" => format!("use serde::{{{}}};", type_name),
        "Result" => "use anyhow::Result;".to_string(),
        "Command" => "use std::process::Command;".to_string(),
        "Path" => "use std::path::Path;".to_string(),
        "PathBuf" => "use std::path::PathBuf;".to_string(),
        _ => format!("// TODO: add import for {}", type_name),
    };

    Some(AutoFix {
        description: format!("Add import for {}", type_name),
        file: file.clone(),
        action: FixAction::AddImport { path: import, file },
    })
}

fn parse_missing_method(line: &str) -> Option<AutoFix> {
    let re = regex::Regex::new(r"no method named `(\w+)` found").ok()?;
    let caps = re.captures(line)?;
    let method = caps.get(1)?.as_str();
    let file = extract_file(line).unwrap_or_else(|| "src/lib.rs".to_string());
    Some(AutoFix {
        description: format!("Missing method '{}' — may need import or trait impl", method),
        file: file.clone(),
        action: FixAction::FixType {
            file,
            old: String::new(),
            new: format!("// TODO: implement or import method '{}'", method),
        },
    })
}

fn extract_file(line: &str) -> Option<String> {
    // Try to extract file path from start of line: "src/foo.rs:10:5: error"
    let re = regex::Regex::new(r"^([^\s:]+\.rs)").ok()?;
    let caps = re.captures(line)?;
    Some(caps.get(1)?.as_str().to_string())
}

/// Apply a single fix. Returns Ok(true) if applied, Ok(false) if not applicable.
fn apply_fix(repo_path: &Path, fix: &AutoFix) -> Result<bool> {
    let file_path = repo_path.join(&fix.file);
    if !file_path.exists() {
        return Ok(false);
    }

    let content = std::fs::read_to_string(&file_path)?;

    match &fix.action {
        FixAction::AddDerive { derive, struct_name } => {
            // Find the struct definition and add derive
            let new_content = add_derive_to_struct(&content, derive, struct_name)?;
            if new_content != content {
                std::fs::write(&file_path, new_content)?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
        FixAction::AddImport { path, .. } => {
            if path.starts_with("//") {
                // This is a TODO, skip
                return Ok(false);
            }
            // Add import at the top of the file (after existing use statements)
            let new_content = add_import(&content, path);
            if new_content != content {
                std::fs::write(&file_path, new_content)?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
        FixAction::FixType { .. } => {
            // Type fixes require manual intervention
            Ok(false)
        }
    }
}

fn add_derive_to_struct(content: &str, derive: &str, struct_name: &str) -> Result<String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut result = String::new();

    for i in 0..lines.len() {
        let line = lines[i];
        // Find the struct definition
        if line.contains("struct") && line.contains(struct_name) {
            // Check if there's already a derive above
            if i > 0 && lines[i - 1].contains("#[derive(") {
                // Add to existing derive
                let prev = lines[i - 1];
                let updated = prev.replace(")]", &format!(", {})])", derive))
                    .replace("#[derive(, ", "#[derive(");
                // Replace last line we added
                result = result.lines().collect::<Vec<_>>().iter()
                    .map(|l| l.to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                result.push_str(&format!("{}\n", updated));
            } else {
                // Add new derive
                result.push_str(&format!("#[derive({})]\n", derive));
            }
        }
        result.push_str(&format!("{}\n", line));
    }

    Ok(result)
}

fn add_import(content: &str, import: &str) -> String {
    // Find the last use statement and insert after it
    let lines: Vec<&str> = content.lines().collect();
    let mut last_use = 0;
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("use ") {
            last_use = i + 1;
        }
    }

    let mut result = String::new();
    for (i, line) in lines.iter().enumerate() {
        result.push_str(&format!("{}\n", line));
        if i + 1 == last_use {
            result.push_str(&format!("{}\n", import));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_missing_derive_error() {
        let errors = "error[E0277]: `Foo` doesn't implement `Debug`\nsrc/lib.rs:10:5: the trait `Debug` is not implemented for `Foo`";
        let fixes = parse_errors(errors);
        assert!(!fixes.is_empty());
        assert!(fixes[0].description.contains("Debug"));
    }

    #[test]
    fn test_parse_missing_type_error() {
        let errors = "error[E0412]: cannot find type `Serialize` in scope\nsrc/lib.rs:5:10";
        let fixes = parse_errors(errors);
        assert!(!fixes.is_empty());
        assert!(fixes[0].description.contains("Serialize"));
    }

    #[test]
    fn test_parse_unused_import() {
        let errors = "warning: unused import: `foo`\nsrc/lib.rs:1:5";
        let fixes = parse_errors(errors);
        assert!(!fixes.is_empty());
    }

    #[test]
    fn test_parse_no_errors() {
        let fixes = parse_errors("Compiling foo v0.1.0\nFinished dev [unoptimized + debuginfo]");
        assert!(fixes.is_empty());
    }

    #[test]
    fn test_add_import_basic() {
        let content = "use std::io;\n\nfn main() {}\n";
        let result = add_import(content, "use anyhow::Result;");
        assert!(result.contains("use anyhow::Result;"));
        // Should be after existing use
        let io_pos = result.find("use std::io;").unwrap();
        let result_pos = result.find("use anyhow::Result;").unwrap();
        assert!(result_pos > io_pos);
    }

    #[test]
    fn test_extract_file() {
        assert_eq!(extract_file("src/lib.rs:10:5: error"), Some("src/lib.rs".into()));
        assert_eq!(extract_file("no file here"), None);
    }

    #[test]
    fn test_add_derive_new() {
        let content = "pub struct Foo {\n    x: i32,\n}\n";
        let result = add_derive_to_struct(content, "Debug", "Foo").unwrap();
        assert!(result.contains("#[derive(Debug)]"));
    }
}
