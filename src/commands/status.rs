//! Status command: scan ~/repos for issues.

use crate::{CommandResult, FleetState, RepoState, exit_codes, is_tty, repos_dir};
use anyhow::Result;
use colored::Colorize;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, serde::Serialize)]
pub struct RepoStatus {
    pub path: String,
    pub name: String,
    pub is_git_repo: bool,
    pub has_uncommitted: bool,
    pub has_remote: bool,
    pub has_readme: bool,
    pub has_cargo_toml: bool,
    pub test_status: Option<bool>,
    pub issues: Vec<String>,
}

pub fn run(state: &mut FleetState) -> Result<CommandResult> {
    let repos_dir = repos_dir();

    if !repos_dir.exists() {
        return Ok(CommandResult::fail(
            "status",
            None,
            format!("~/repos directory not found: {}", repos_dir.display()),
            exit_codes::BUILD_ERROR,
        ));
    }

    if is_tty() {
        println!("{} Scanning {}", "🔍".to_string().blue().bold(), repos_dir.display());
    }

    let entries = std::fs::read_dir(&repos_dir)?;
    let mut repo_statuses: Vec<RepoStatus> = Vec::new();
    let mut total_issues = 0;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let path_str = path.to_string_lossy().to_string();

        let mut status = RepoStatus {
            path: path_str.clone(),
            name,
            is_git_repo: path.join(".git").exists(),
            has_uncommitted: false,
            has_remote: false,
            has_readme: has_readme(&path),
            has_cargo_toml: path.join("Cargo.toml").exists(),
            test_status: None,
            issues: Vec::new(),
        };

        if status.is_git_repo {
            // Check uncommitted changes
            if let Ok(output) = Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(&path)
                .output()
            {
                status.has_uncommitted = !output.stdout.is_empty();
                if status.has_uncommitted {
                    status.issues.push("Uncommitted changes".into());
                }
            }

            // Check remote
            if let Ok(output) = Command::new("git")
                .args(["remote"])
                .current_dir(&path)
                .output()
            {
                let remotes = String::from_utf8_lossy(&output.stdout);
                status.has_remote = !remotes.trim().is_empty();
                if !status.has_remote {
                    status.issues.push("No remote configured".into());
                }
            }

            // Check if ahead of remote
            if let Ok(output) = Command::new("git")
                .args(["log", "--oneline", "@{push}..HEAD"])
                .current_dir(&path)
                .output()
            {
                let unpushed = String::from_utf8_lossy(&output.stdout);
                if !unpushed.trim().is_empty() {
                    let count = unpushed.lines().count();
                    status.issues.push(format!("{} unpushed commits", count));
                }
            }
        }

        if !status.has_readme {
            status.issues.push("Missing README".into());
        }

        // Check test status from state
        if let Some(repo_state) = state.get_repo(&path_str) {
            status.test_status = repo_state.last_test_pass;
            if repo_state.last_test_pass == Some(false) {
                status.issues.push("Last test run failed".into());
            }
        }

        total_issues += status.issues.len();
        repo_statuses.push(status);
    }

    // Print results
    if is_tty() {
        println!("\n{} Repository Status ({} repos):\n", "📋".to_string().blue().bold(), repo_statuses.len());
        for s in &repo_statuses {
            let health = if s.issues.is_empty() {
                "✓".green()
            } else {
                "✗".red()
            };
            println!(
                "  {} {} — {} issue(s)",
                health,
                s.name.cyan(),
                s.issues.len().to_string().yellow()
            );
            for issue in &s.issues {
                println!("    • {}", issue.yellow());
            }
        }
        println!(
            "\n  Total repos: {} | Total issues: {}",
            repo_statuses.len().to_string().cyan(),
            total_issues.to_string().yellow()
        );
    }

    // Update state with discovered repos
    for s in &repo_statuses {
        let existing = state.get_repo(&s.path);
        let repo_state = RepoState {
            path: s.path.clone(),
            last_build: existing.and_then(|r| r.last_build.clone()),
            last_test_pass: existing.and_then(|r| r.last_test_pass),
            last_push: existing.and_then(|r| r.last_push.clone()),
            error_count: existing.map(|r| r.error_count).unwrap_or(0),
            fix_count: existing.map(|r| r.fix_count).unwrap_or(0),
        };
        state.upsert_repo(repo_state);
    }

    let details = serde_json::json!({
        "total_repos": repo_statuses.len(),
        "total_issues": total_issues,
        "repos": repo_statuses,
    });

    Ok(CommandResult::ok("status", None, format!("Scanned {} repos, found {} issues", repo_statuses.len(), total_issues))
        .with_details(details))
}

fn has_readme(path: &Path) -> bool {
    path.join("README.md").exists()
        || path.join("README.txt").exists()
        || path.join("README").exists()
        || path.join("readme.md").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_readme_md() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Test").unwrap();
        assert!(has_readme(dir.path()));
    }

    #[test]
    fn test_has_readme_txt() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.txt"), "Test").unwrap();
        assert!(has_readme(dir.path()));
    }

    #[test]
    fn test_no_readme() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!has_readme(dir.path()));
    }

    #[test]
    fn test_repo_status_default() {
        let s = RepoStatus {
            path: "/tmp/test".into(),
            name: "test".into(),
            is_git_repo: false,
            has_uncommitted: false,
            has_remote: false,
            has_readme: true,
            has_cargo_toml: false,
            test_status: None,
            issues: vec![],
        };
        assert!(s.issues.is_empty());
        assert!(s.has_readme);
        assert!(!s.is_git_repo);
    }
}
