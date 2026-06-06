//! Build command: clean build + cargo test.

use crate::{CommandResult, FleetState, RepoState, exit_codes, is_tty, resolve_repo_path};
use anyhow::{Context, Result};
use colored::Colorize;
use std::process::Command;


pub fn run(repo_path: &str, clean: bool, state: &mut FleetState) -> Result<CommandResult> {
    let resolved = resolve_repo_path(repo_path);
    let repo_str = resolved.to_string_lossy().to_string();

    if !resolved.exists() {
        return Ok(CommandResult::fail(
            "build",
            Some(repo_str),
            format!("Path does not exist: {}", resolved.display()),
            exit_codes::BUILD_ERROR,
        ));
    }

    if !resolved.join("Cargo.toml").exists() {
        return Ok(CommandResult::fail(
            "build",
            Some(repo_str.clone()),
            "Not a Rust crate (no Cargo.toml)".to_string(),
            exit_codes::BUILD_ERROR,
        ));
    }

    if is_tty() {
        println!("{} Building {}", "🔧".to_string().blue().bold(), repo_str.cyan());
    }

    // Clean target directory if requested
    let target_dir = resolved.join("target");
    if clean && target_dir.exists() {
        if is_tty() {
            println!("  {} Cleaning target/", "🗑️".to_string().yellow());
        }
        std::fs::remove_dir_all(&target_dir)
            .with_context(|| format!("Failed to remove target/ in {}", resolved.display()))?;
    }

    // Run cargo test
    let output = Command::new("cargo")
        .args(["test", "--all"])
        .current_dir(&resolved)
        .env("TERM", "dumb")
        .output()
        .with_context(|| format!("Failed to run cargo test in {}", resolved.display()))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{}\n{}", stdout, stderr);

    let success = output.status.success();
    let exit_code = if success {
        exit_codes::SUCCESS
    } else if combined.contains("test result:") && combined.contains("FAILED") {
        exit_codes::TEST_FAILURE
    } else {
        exit_codes::BUILD_ERROR
    };

    // Parse test summary
    let test_summary = parse_test_summary(&combined);

    // Update state
    let now = chrono::Utc::now().to_rfc3339();
    let existing = state.get_repo(&repo_str);
    let repo_state = RepoState {
        path: repo_str.clone(),
        last_build: Some(now),
        last_test_pass: Some(success),
        last_push: existing.and_then(|r| r.last_push.clone()),
        error_count: if !success { existing.map(|r| r.error_count + 1).unwrap_or(1) } else { 0 },
        fix_count: existing.map(|r| r.fix_count).unwrap_or(0),
    };
    state.upsert_repo(repo_state);

    if is_tty() {
        if success {
            println!("  {} Build & tests passed", "✓".green().bold());
            if let Some(ts) = &test_summary {
                println!("    Tests: {} passed, {} failed", ts.passed.to_string().green(), ts.failed.to_string().red());
            }
        } else {
            println!("  {} Build/test failed", "✗".red().bold());
            // Print last few lines of error
            let lines: Vec<&str> = combined.lines().collect();
            let start = lines.len().saturating_sub(15);
            for line in &lines[start..] {
                if !line.is_empty() {
                    println!("    {}", line);
                }
            }
        }
    }

    let mut result = if success {
        CommandResult::ok("build", Some(repo_str.clone()), "Build and tests passed")
    } else {
        CommandResult::fail(
            "build",
            Some(repo_str.clone()),
            "Build or tests failed",
            exit_code,
        )
    };

    let mut details = serde_json::Map::new();
    details.insert("stdout".into(), serde_json::Value::String(stdout));
    details.insert("stderr".into(), serde_json::Value::String(stderr));
    if let Some(ts) = test_summary {
        details.insert("tests_passed".into(), serde_json::Value::Number(ts.passed.into()));
        details.insert("tests_failed".into(), serde_json::Value::Number(ts.failed.into()));
    }
    result = result.with_details(serde_json::Value::Object(details));

    Ok(result)
}

struct TestSummary {
    passed: usize,
    failed: usize,
}

fn parse_test_summary(output: &str) -> Option<TestSummary> {
    // Parse lines like "test result: ok. 5 passed; 0 failed; 0 ignored;"
    // or "test result: FAILED. 3 passed; 2 failed; 0 ignored;"
    for line in output.lines().rev() {
        if line.contains("test result:") {
            let passed = extract_count(line, "passed")?;
            let failed = extract_count(line, "failed").unwrap_or(0);
            return Some(TestSummary { passed, failed });
        }
    }
    None
}

fn extract_count(line: &str, keyword: &str) -> Option<usize> {
    // Look for pattern like "5 passed" or "0 failed"
    // Keyword may be followed by space, semicolon, or end of line
    for suffix in &[" ", ";", ""] {
        let pattern = format!(" {}{}", keyword, suffix);
        if let Some(idx) = line.find(&pattern) {
            let before = &line[..idx];
            let num_str: String = before.chars()
                .rev()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            if let Ok(n) = num_str.parse() {
                return Some(n);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_test_summary_passed() {
        let output = "running 5 tests\ntest result: ok. 5 passed; 0 failed; 0 ignored;";
        let summary = parse_test_summary(output).unwrap();
        assert_eq!(summary.passed, 5);
        assert_eq!(summary.failed, 0);
    }

    #[test]
    fn test_parse_test_summary_failed() {
        let output = "test result: FAILED. 3 passed; 2 failed; 0 ignored;";
        let summary = parse_test_summary(output).unwrap();
        assert_eq!(summary.passed, 3);
        assert_eq!(summary.failed, 2);
    }

    #[test]
    fn test_parse_test_summary_none() {
        let output = "no test results here";
        assert!(parse_test_summary(output).is_none());
    }

    #[test]
    fn test_extract_count() {
        assert_eq!(extract_count("5 passed; 0 failed", "passed"), Some(5));
        assert_eq!(extract_count("5 passed; 0 failed", "failed"), Some(0));
    }
}
