//! fleet-build: Automated Rust crate build, test, fix, and push CLI.
//!
//! This library provides the core types, state management, and command
//! implementations for the `fleet-build` CLI tool.

pub mod commands;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Exit codes used by the CLI.
pub mod exit_codes {
    pub const SUCCESS: i32 = 0;
    pub const TEST_FAILURE: i32 = 1;
    pub const BUILD_ERROR: i32 = 2;
    pub const PUSH_ERROR: i32 = 3;
}

/// Structured result for any fleet-build command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub command: String,
    pub repo: Option<String>,
    pub success: bool,
    pub exit_code: i32,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

impl CommandResult {
    pub fn ok(command: impl Into<String>, repo: Option<String>, message: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            repo,
            success: true,
            exit_code: exit_codes::SUCCESS,
            message: message.into(),
            details: None,
        }
    }

    pub fn fail(command: impl Into<String>, repo: Option<String>, message: impl Into<String>, exit_code: i32) -> Self {
        Self {
            command: command.into(),
            repo,
            success: false,
            exit_code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

/// Per-repo state tracked across invocations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoState {
    pub path: String,
    pub last_build: Option<String>,
    pub last_test_pass: Option<bool>,
    pub last_push: Option<String>,
    pub error_count: u32,
    pub fix_count: u32,
}

/// Global state file contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetState {
    pub repos: Vec<RepoState>,
}

impl FleetState {
    /// Path to the state file.
    pub fn state_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".fleet-build-state.json")
    }

    /// Load state from disk. Returns empty state if file doesn't exist.
    pub fn load() -> anyhow::Result<Self> {
        let path = Self::state_path();
        if !path.exists() {
            return Ok(Self { repos: vec![] });
        }
        let data = std::fs::read_to_string(&path)?;
        let state: FleetState = serde_json::from_str(&data)?;
        Ok(state)
    }

    /// Save state to disk.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::state_path();
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    /// Update or insert state for a repo.
    pub fn upsert_repo(&mut self, repo: RepoState) {
        if let Some(existing) = self.repos.iter_mut().find(|r| r.path == repo.path) {
            *existing = repo;
        } else {
            self.repos.push(repo);
        }
    }

    /// Get state for a specific repo.
    pub fn get_repo(&self, path: &str) -> Option<&RepoState> {
        self.repos.iter().find(|r| r.path == path)
    }
}

/// Helper to detect if stdout is a TTY.
pub fn is_tty() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

/// Print output: JSON if not TTY, colorized human output if TTY.
pub fn print_result(result: &CommandResult) {
    if is_tty() {
        print_result_human(result);
    } else {
        println!("{}", serde_json::to_string(result).unwrap());
    }
}

fn print_result_human(result: &CommandResult) {
    use colored::Colorize;
    let status = if result.success {
        "✓".green().bold()
    } else {
        "✗".red().bold()
    };
    let repo_display = result.repo.as_deref().unwrap_or("N/A");
    println!(
        "{} [{}] {} — {}",
        status, result.command, repo_display, result.message
    );
    if let Some(details) = &result.details {
        if let Some(obj) = details.as_object() {
            for (key, val) in obj {
                println!("  {}: {}", key.cyan(), val);
            }
        }
    }
}

/// Expand `~/repos` path.
pub fn repos_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("repos")
}

/// Check if a path looks like a Rust crate (has Cargo.toml).
pub fn is_rust_crate(path: &Path) -> bool {
    path.join("Cargo.toml").exists()
}

/// Resolve a repo path: if relative, try ~/repos/<path>.
pub fn resolve_repo_path(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        p
    } else {
        repos_dir().join(path)
    }
}

/// A simplified home_dir helper (avoids extra dep).
mod dirs {
    use std::path::PathBuf;
    pub fn home_dir() -> Option<PathBuf> {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_result_ok() {
        let r = CommandResult::ok("build", Some("/tmp/test".into()), "All good");
        assert!(r.success);
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.command, "build");
    }

    #[test]
    fn test_command_result_fail() {
        let r = CommandResult::fail("push", None, "Push failed", 3);
        assert!(!r.success);
        assert_eq!(r.exit_code, 3);
    }

    #[test]
    fn test_command_result_with_details() {
        let r = CommandResult::ok("build", None, "ok")
            .with_details(serde_json::json!({"tests_run": 42}));
        assert!(r.details.is_some());
        let d = r.details.unwrap();
        assert_eq!(d["tests_run"], 42);
    }

    #[test]
    fn test_fleet_state_default() {
        let state = FleetState { repos: vec![] };
        assert!(state.repos.is_empty());
    }

    #[test]
    fn test_fleet_state_upsert() {
        let mut state = FleetState { repos: vec![] };
        let repo = RepoState {
            path: "/tmp/foo".into(),
            last_build: None,
            last_test_pass: None,
            last_push: None,
            error_count: 0,
            fix_count: 0,
        };
        state.upsert_repo(repo.clone());
        assert_eq!(state.repos.len(), 1);

        let mut updated = repo.clone();
        updated.error_count = 5;
        state.upsert_repo(updated);
        assert_eq!(state.repos.len(), 1);
        assert_eq!(state.repos[0].error_count, 5);
    }

    #[test]
    fn test_fleet_state_get_repo() {
        let mut state = FleetState { repos: vec![] };
        state.upsert_repo(RepoState {
            path: "/tmp/bar".into(),
            last_build: None,
            last_test_pass: None,
            last_push: None,
            error_count: 0,
            fix_count: 0,
        });
        assert!(state.get_repo("/tmp/bar").is_some());
        assert!(state.get_repo("/tmp/missing").is_none());
    }

    #[test]
    fn test_resolve_repo_path_absolute() {
        let p = resolve_repo_path("/absolute/path");
        assert_eq!(p, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn test_resolve_repo_path_relative() {
        let p = resolve_repo_path("my-repo");
        assert!(p.to_string_lossy().contains("repos"));
        assert!(p.to_string_lossy().ends_with("my-repo"));
    }

    #[test]
    fn test_is_rust_crate_with_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_rust_crate(dir.path()));
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        assert!(is_rust_crate(dir.path()));
    }

    #[test]
    fn test_state_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".fleet-build-state.json");
        let state = FleetState {
            repos: vec![RepoState {
                path: "/tmp/test".into(),
                last_build: Some("2024-01-01".into()),
                last_test_pass: Some(true),
                last_push: None,
                error_count: 2,
                fix_count: 1,
            }],
        };
        let data = serde_json::to_string_pretty(&state).unwrap();
        std::fs::write(&path, &data).unwrap();
        let loaded: FleetState = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.repos.len(), 1);
        assert_eq!(loaded.repos[0].error_count, 2);
    }

    #[test]
    fn test_exit_codes() {
        assert_eq!(exit_codes::SUCCESS, 0);
        assert_eq!(exit_codes::TEST_FAILURE, 1);
        assert_eq!(exit_codes::BUILD_ERROR, 2);
        assert_eq!(exit_codes::PUSH_ERROR, 3);
    }

    #[test]
    fn test_command_result_serialization() {
        let r = CommandResult::ok("build", Some("/tmp".into()), "ok");
        let json = serde_json::to_string(&r).unwrap();
        let parsed: CommandResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.command, "build");
        assert!(parsed.success);
    }
}
