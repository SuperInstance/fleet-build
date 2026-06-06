//! Batch command: run build+test+push on multiple repos.

use crate::{CommandResult, FleetState, exit_codes, is_tty, resolve_repo_path};
use anyhow::Result;
use colored::Colorize;
use std::process::Command;

pub fn run(repo_paths: &[String], max_jobs: usize, _state: &mut FleetState) -> Result<CommandResult> {
    if repo_paths.is_empty() {
        return Ok(CommandResult::fail(
            "batch",
            None,
            "No repo paths provided".to_string(),
            exit_codes::BUILD_ERROR,
        ));
    }

    if is_tty() {
        println!(
            "{} Batching {} repos (max {} parallel)",
            "⚡".blue().bold(),
            repo_paths.len().to_string().cyan(),
            max_jobs.to_string().cyan()
        );
    }

    let mut results: Vec<CommandResult> = Vec::new();

    for chunk in repo_paths.chunks(max_jobs) {
        let mut handles = Vec::new();

        for repo_path in chunk {
            let repo = repo_path.clone();
            let handle = std::thread::spawn(move || process_repo(&repo));
            handles.push(handle);
        }

        for handle in handles {
            match handle.join() {
                Ok((build_result, push_result)) => {
                    results.push(build_result);
                    if let Some(pr) = push_result {
                        results.push(pr);
                    }
                }
                Err(_) => {
                    results.push(CommandResult::fail(
                        "batch",
                        None,
                        "Thread panicked".to_string(),
                        exit_codes::BUILD_ERROR,
                    ));
                }
            }
        }
    }

    let total = results.len();
    let succeeded = results.iter().filter(|r| r.success).count();
    let failed = total - succeeded;

    // Print summary
    if is_tty() {
        println!("\n{} Batch Summary:", "📊".blue().bold());
        for r in &results {
            let status = if r.success { "✓".green() } else { "✗".red() };
            println!("  {} [{}] {}", status, r.command, r.message);
        }
        println!(
            "\n  Total: {} | Passed: {} | Failed: {}",
            total.to_string().cyan(),
            succeeded.to_string().green(),
            failed.to_string().red()
        );
    }

    let _exit_code = if failed == 0 { exit_codes::SUCCESS } else { exit_codes::TEST_FAILURE };

    Ok(CommandResult::ok(
        "batch",
        None,
        format!("Batch complete: {}/{} passed", succeeded, total),
    ).with_details(serde_json::json!({
        "total": total,
        "succeeded": succeeded,
        "failed": failed,
        "results": results.iter().map(|r| serde_json::json!({
            "command": r.command,
            "repo": r.repo,
            "success": r.success,
            "message": r.message,
        })).collect::<Vec<_>>(),
    })))
}

fn process_repo(repo_path: &str) -> (CommandResult, Option<CommandResult>) {
    let resolved = resolve_repo_path(repo_path);
    let repo_str = resolved.to_string_lossy().to_string();

    // Build + Test
    let output = Command::new("cargo")
        .args(["test", "--all"])
        .current_dir(&resolved)
        .env("TERM", "dumb")
        .output();

    let build_result = match output {
        Ok(o) => {
            let success = o.status.success();
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            let exit_code = if success { exit_codes::SUCCESS } else { exit_codes::TEST_FAILURE };
            CommandResult {
                command: "build".into(),
                repo: Some(repo_str.clone()),
                success,
                exit_code,
                message: if success { "Build passed".into() } else { format!("Build failed: {}{}", stdout, stderr) },
                details: None,
            }
        }
        Err(e) => CommandResult::fail("build", Some(repo_str.clone()), e.to_string(), exit_codes::BUILD_ERROR),
    };

    // If build passed, try push
    let push_result = if build_result.success {
        let push_output = Command::new("git")
            .args(["push"])
            .current_dir(&resolved)
            .output();

        match push_output {
            Ok(o) if o.status.success() => {
                Some(CommandResult::ok("push", Some(repo_str), "Pushed".to_string()))
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                Some(CommandResult::fail("push", Some(repo_str), format!("Push failed: {}", stderr), exit_codes::PUSH_ERROR))
            }
            Err(e) => {
                Some(CommandResult::fail("push", Some(repo_str), e.to_string(), exit_codes::PUSH_ERROR))
            }
        }
    } else {
        None
    };

    (build_result, push_result)
}
