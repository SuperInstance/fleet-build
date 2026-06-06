//! Push command: git add, commit, push with secret scanning.

use crate::{CommandResult, FleetState, RepoState, exit_codes, is_tty, resolve_repo_path};
use anyhow::{Context, Result};
use colored::Colorize;
use std::process::Command;

/// Patterns that look like secrets.
const SECRET_PATTERNS: &[&str] = &[
    "ghp_", "gho_", "sk-", "api_key", "api.key", "secret", "token=", "password", "Bearer ",
    "AKIA", // AWS access key
    "-----BEGIN RSA PRIVATE KEY-----",
    "xoxb-", // Slack token
    "hooks.slack.com",
];

pub fn run(repo_path: &str, message: &str, scan_secrets: bool, state: &mut FleetState) -> Result<CommandResult> {
    let resolved = resolve_repo_path(repo_path);
    let repo_str = resolved.to_string_lossy().to_string();

    if !resolved.exists() {
        return Ok(CommandResult::fail(
            "push",
            Some(repo_str),
            format!("Path does not exist: {}", resolved.display()),
            exit_codes::PUSH_ERROR,
        ));
    }

    if !resolved.join(".git").exists() {
        return Ok(CommandResult::fail(
            "push",
            Some(repo_str.clone()),
            "Not a git repository".to_string(),
            exit_codes::PUSH_ERROR,
        ));
    }

    if is_tty() {
        println!("{} Pushing {}", "🚀".to_string().blue().bold(), repo_str.cyan());
    }

    // Check for uncommitted changes
    let status_output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&resolved)
        .output()
        .context("Failed to run git status")?;

    let has_changes = !status_output.stdout.is_empty();

    if has_changes {
        // Stage all changes
        if is_tty() {
            println!("  {} Staging changes", "📦".to_string().yellow());
        }
        let add_output = Command::new("git")
            .args(["add", "-A"])
            .current_dir(&resolved)
            .output()
            .context("Failed to run git add")?;

        if !add_output.status.success() {
            return Ok(CommandResult::fail(
                "push",
                Some(repo_str.clone()),
                "git add failed".to_string(),
                exit_codes::PUSH_ERROR,
            ));
        }

        // Secret scanning
        if scan_secrets {
            if is_tty() {
                println!("  {} Scanning for secrets", "🔒".to_string().yellow());
            }
            let diff_output = Command::new("git")
                .args(["diff", "--cached"])
                .current_dir(&resolved)
                .output()
                .context("Failed to run git diff")?;

            let diff = String::from_utf8_lossy(&diff_output.stdout);
            if let Some(secret) = scan_for_secrets(&diff) {
                return Ok(CommandResult::fail(
                    "push",
                    Some(repo_str.clone()),
                    format!("BLOCKED: Secret detected in diff: {}", secret),
                    exit_codes::PUSH_ERROR,
                ).with_details(serde_json::json!({
                    "secret_pattern": secret,
                    "action": "blocked_push"
                })));
            }
        }

        // Commit
        if is_tty() {
            println!("  {} Committing: {}", "📝".to_string().yellow(), message);
        }
        let commit_output = Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(&resolved)
            .output()
            .context("Failed to run git commit")?;

        if !commit_output.status.success() {
            let stderr = String::from_utf8_lossy(&commit_output.stderr);
            return Ok(CommandResult::fail(
                "push",
                Some(repo_str.clone()),
                format!("git commit failed: {}", stderr.trim()),
                exit_codes::PUSH_ERROR,
            ));
        }
    } else if is_tty() {
        println!("  {} No uncommitted changes", "ℹ️".to_string().yellow());
    }

    // Push
    if is_tty() {
        println!("  {} Pushing to remote", "⬆️".to_string().yellow());
    }
    let push_output = Command::new("git")
        .args(["push"])
        .current_dir(&resolved)
        .output()
        .context("Failed to run git push")?;

    if !push_output.status.success() {
        let stderr = String::from_utf8_lossy(&push_output.stderr);
        return Ok(CommandResult::fail(
            "push",
            Some(repo_str.clone()),
            format!("git push failed: {}", stderr.trim()),
            exit_codes::PUSH_ERROR,
        ));
    }

    // Update state
    let now = chrono::Utc::now().to_rfc3339();
    let existing = state.get_repo(&repo_str);
    let repo_state = RepoState {
        path: repo_str.clone(),
        last_build: existing.and_then(|r| r.last_build.clone()),
        last_test_pass: existing.and_then(|r| r.last_test_pass),
        last_push: Some(now),
        error_count: 0,
        fix_count: existing.map(|r| r.fix_count).unwrap_or(0),
    };
    state.upsert_repo(repo_state);

    if is_tty() {
        println!("  {} Push complete", "✓".green().bold());
    }

    Ok(CommandResult::ok("push", Some(repo_str), "Pushed successfully")
        .with_details(serde_json::json!({
            "commit_message": message,
            "had_changes": has_changes,
        })))
}

/// Scan text for secret patterns. Returns the first match pattern name.
pub fn scan_for_secrets(text: &str) -> Option<String> {
    for pattern in SECRET_PATTERNS {
        if text.contains(pattern) {
            return Some(pattern.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_clean_text() {
        assert!(scan_for_secrets("just normal code here").is_none());
    }

    #[test]
    fn test_scan_github_token() {
        assert_eq!(
            scan_for_secrets("ghp_abc123def456"),
            Some("ghp_".to_string())
        );
    }

    #[test]
    fn test_scan_sk_prefix() {
        assert_eq!(
            scan_for_secrets("sk-proj-abc123"),
            Some("sk-".to_string())
        );
    }

    #[test]
    fn test_scan_bearer() {
        assert_eq!(
            scan_for_secrets("Authorization: Bearer token123"),
            Some("Bearer ".to_string())
        );
    }

    #[test]
    fn test_scan_aws_key() {
        assert_eq!(
            scan_for_secrets("AWS_ACCESS_KEY=AKIAIOSFODNN7EXAMPLE"),
            Some("AKIA".to_string())
        );
    }

    #[test]
    fn test_scan_slack_token() {
        assert_eq!(
            scan_for_secrets("xoxb-123456789-abcdef"),
            Some("xoxb-".to_string())
        );
    }

    #[test]
    fn test_scan_password() {
        assert_eq!(
            scan_for_secrets("password=hunter2"),
            Some("password".to_string())
        );
    }

    #[test]
    fn test_scan_private_key() {
        assert!(scan_for_secrets("-----BEGIN RSA PRIVATE KEY-----\nMIIE...").is_some());
    }

    #[test]
    fn test_scan_false_positive_safe() {
        // "secret" in a comment should still be caught — that's the point
        assert!(scan_for_secrets("// secret sauce recipe").is_some());
    }
}
