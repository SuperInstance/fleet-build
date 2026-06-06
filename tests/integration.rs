//! Integration tests for fleet-build CLI.
//!
//! These tests verify the CLI behavior end-to-end by creating temporary
//! Rust crate projects and running commands against them.

use fleet_build::*;
use std::fs;
use std::path::PathBuf;

/// Helper to create a minimal Rust crate in a temp directory.
fn create_test_crate() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let cargo_toml = r#"
[package]
name = "test-crate"
version = "0.1.0"
edition = "2024"

[dependencies]
"#;
    fs::write(dir.path().join("Cargo.toml"), cargo_toml).unwrap();
    let main_rs = r#"
fn main() {
    println!("hello");
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() {
        assert_eq!(1 + 1, 2);
    }

    #[test]
    fn test_another() {
        assert!(true);
    }
}
"#;
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/main.rs"), main_rs).unwrap();
    dir
}

/// Create a crate that will fail to compile.
fn create_broken_crate() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let cargo_toml = r#"
[package]
name = "broken-crate"
version = "0.1.0"
edition = "2024"

[dependencies]
"#;
    fs::write(dir.path().join("Cargo.toml"), cargo_toml).unwrap();
    let main_rs = r#"
fn main() {
    let x: String = 42; // type mismatch
}
"#;
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/main.rs"), main_rs).unwrap();
    dir
}

#[test]
fn test_command_result_json_serialization() {
    let result = CommandResult::ok(
        "build",
        Some("/tmp/test".to_string()),
        "Build passed".to_string(),
    );
    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("\"success\":true"));
    assert!(json.contains("\"command\":\"build\""));
    assert!(json.contains("\"exit_code\":0"));
}

#[test]
fn test_command_result_fail_json() {
    let result = CommandResult::fail(
        "push",
        None,
        "Push failed".to_string(),
        exit_codes::PUSH_ERROR,
    );
    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("\"success\":false"));
    assert!(json.contains("\"exit_code\":3"));
}

#[test]
fn test_state_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let state = FleetState {
        repos: vec![
            RepoState {
                path: "/tmp/repo1".into(),
                last_build: Some("2024-01-01T00:00:00Z".into()),
                last_test_pass: Some(true),
                last_push: Some("2024-01-01T01:00:00Z".into()),
                error_count: 0,
                fix_count: 2,
            },
            RepoState {
                path: "/tmp/repo2".into(),
                last_build: Some("2024-01-02T00:00:00Z".into()),
                last_test_pass: Some(false),
                last_push: None,
                error_count: 3,
                fix_count: 1,
            },
        ],
    };
    let data = serde_json::to_string_pretty(&state).unwrap();
    fs::write(&path, &data).unwrap();
    let loaded: FleetState = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(loaded.repos.len(), 2);
    assert_eq!(loaded.repos[0].fix_count, 2);
    assert_eq!(loaded.repos[1].error_count, 3);
    assert!(loaded.repos[0].last_test_pass.unwrap());
    assert!(!loaded.repos[1].last_test_pass.unwrap());
}

#[test]
fn test_is_rust_crate() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!is_rust_crate(dir.path()));
    fs::write(dir.path().join("Cargo.toml"), "").unwrap();
    assert!(is_rust_crate(dir.path()));
}

#[test]
fn test_resolve_path_absolute() {
    let p = resolve_repo_path("/absolute/path/to/repo");
    assert_eq!(p, PathBuf::from("/absolute/path/to/repo"));
}

#[test]
fn test_resolve_path_relative() {
    let p = resolve_repo_path("my-repo");
    assert!(p.to_string_lossy().contains("repos"));
    assert!(p.to_string_lossy().ends_with("my-repo"));
}

#[test]
fn test_repos_dir() {
    let dir = repos_dir();
    assert!(dir.to_string_lossy().ends_with("repos"));
}

#[test]
fn test_exit_code_values() {
    assert_eq!(exit_codes::SUCCESS, 0);
    assert_eq!(exit_codes::TEST_FAILURE, 1);
    assert_eq!(exit_codes::BUILD_ERROR, 2);
    assert_eq!(exit_codes::PUSH_ERROR, 3);
}

#[test]
fn test_fleet_state_upsert_insert() {
    let mut state = FleetState { repos: vec![] };
    state.upsert_repo(RepoState {
        path: "/tmp/new".into(),
        last_build: None,
        last_test_pass: None,
        last_push: None,
        error_count: 0,
        fix_count: 0,
    });
    assert_eq!(state.repos.len(), 1);
}

#[test]
fn test_fleet_state_upsert_update() {
    let mut state = FleetState { repos: vec![] };
    state.upsert_repo(RepoState {
        path: "/tmp/repo".into(),
        last_build: None,
        last_test_pass: None,
        last_push: None,
        error_count: 1,
        fix_count: 0,
    });
    state.upsert_repo(RepoState {
        path: "/tmp/repo".into(),
        last_build: Some("2024-01-01".into()),
        last_test_pass: Some(true),
        last_push: None,
        error_count: 0,
        fix_count: 3,
    });
    assert_eq!(state.repos.len(), 1);
    assert_eq!(state.repos[0].fix_count, 3);
    assert!(state.repos[0].last_test_pass.unwrap());
}

#[test]
fn test_fleet_state_get() {
    let mut state = FleetState { repos: vec![] };
    state.upsert_repo(RepoState {
        path: "/tmp/exists".into(),
        last_build: None,
        last_test_pass: None,
        last_push: None,
        error_count: 0,
        fix_count: 0,
    });
    assert!(state.get_repo("/tmp/exists").is_some());
    assert!(state.get_repo("/tmp/missing").is_none());
}

#[test]
fn test_command_result_with_details() {
    let r = CommandResult::ok("test", None, "ok".to_string())
        .with_details(serde_json::json!({"key": "value"}));
    let details = r.details.unwrap();
    assert_eq!(details["key"], "value");
}

#[test]
fn test_multiple_details() {
    let r = CommandResult::ok("test", None, "ok".to_string())
        .with_details(serde_json::json!({
            "tests_run": 10,
            "tests_passed": 10,
            "tests_failed": 0,
        }));
    let d = r.details.unwrap();
    assert_eq!(d["tests_run"], 10);
}

#[test]
fn test_result_serde_roundtrip_with_details() {
    let r = CommandResult::ok("build", Some("/path".to_string()), "passed".to_string())
        .with_details(serde_json::json!({"duration_ms": 1500}));
    let json = serde_json::to_string(&r).unwrap();
    let parsed: CommandResult = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.command, "build");
    assert_eq!(parsed.repo, Some("/path".to_string()));
    assert!(parsed.success);
    assert_eq!(parsed.details.unwrap()["duration_ms"], 1500);
}

#[test]
fn test_create_test_crate_is_valid() {
    let crate_dir = create_test_crate();
    assert!(crate_dir.path().join("Cargo.toml").exists());
    assert!(crate_dir.path().join("src/main.rs").exists());
    assert!(is_rust_crate(crate_dir.path()));
}
