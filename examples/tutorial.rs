//! Tutorial: fleet-build — Automated Rust crate build, test, fix, and push
//!
//! Fleet-build manages the full lifecycle: clone → build → test → fix → push.

use fleet_build::{CommandResult, FleetState, RepoState};

fn main() {
    println!("=== Fleet Build Tutorial ===\n");

    // Part 1: Command results
    println!("Part 1: Command results");
    let ok = CommandResult::ok("cargo test", Some("fleet-midi".into()), "All 18 tests passed");
    let fail = CommandResult::fail("cargo build", Some("broken-crate".into()), "E0433: missing mod", 2);
    println!("  OK: {} → {}", ok.command, ok.message);
    println!("  Fail: {} → {} (exit {})", fail.command, fail.message, fail.exit_code);
    
    let with_details = ok.clone().with_details(serde_json::json!({"tests": 18, "duration_ms": 4200}));
    println!("  With details: {:?}", with_details.details);
    println!();

    // Part 2: Fleet state management
    println!("Part 2: Fleet state");
    let mut state = FleetState::load().unwrap_or_else(|_| FleetState {
        repos: vec![],
    });
    println!("  {} repos tracked", state.repos.len());
    
    let repo = RepoState {
        path: "/tmp/my-crate".into(),
        last_build: None,
        last_test_pass: None,
        last_push: None,
        error_count: 0,
        fix_count: 0,
    };
    state.upsert_repo(repo);
    println!("  Added repo, now {} tracked", state.repos.len());
    
    if let Some(r) = state.get_repo("/tmp/my-crate") {
        println!("  Found: {}", r.path);
    }
    println!();

    // Part 3: Utility functions
    println!("Part 3: Utilities");
    println!("  Repos dir: {:?}", fleet_build::repos_dir());
    println!("  Is TTY: {}", fleet_build::is_tty());
}
