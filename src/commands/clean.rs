//! Clean command: remove all target/ directories under ~/repos.

use crate::{CommandResult, FleetState, exit_codes, is_tty, repos_dir};
use anyhow::Result;
use colored::Colorize;
use std::path::Path;

pub fn run(deep: bool, _state: &mut FleetState) -> Result<CommandResult> {
    let repos_dir = repos_dir();

    if !repos_dir.exists() {
        return Ok(CommandResult::fail(
            "clean",
            None,
            format!("~/repos directory not found: {}", repos_dir.display()),
            exit_codes::BUILD_ERROR,
        ));
    }

    if is_tty() {
        let mode = if deep { "deep clean" } else { "clean" };
        println!("{} Running {} on {}", "🧹".to_string().blue().bold(), mode, repos_dir.display());
    }

    let entries = std::fs::read_dir(&repos_dir)?;
    let mut cleaned = 0;
    let mut freed_bytes: u64 = 0;
    let mut errors = 0;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // Clean target/ directory
        let target = path.join("target");
        if target.exists() {
            let size = dir_size(&target);
            match std::fs::remove_dir_all(&target) {
                Ok(()) => {
                    cleaned += 1;
                    freed_bytes += size;
                    if is_tty() {
                        println!(
                            "  {} Removed {} ({})",
                            "🗑️".to_string().yellow(),
                            target.display(),
                            format_size(size).green()
                        );
                    }
                }
                Err(e) => {
                    errors += 1;
                    if is_tty() {
                        println!(
                            "  {} Failed to remove {}: {}",
                            "✗".red(),
                            target.display(),
                            e
                        );
                    }
                }
            }
        }

        // Deep clean: also remove Cargo.lock
        if deep {
            let lock = path.join("Cargo.lock");
            if lock.exists() {
                let _ = std::fs::remove_file(&lock);
            }
        }
    }

    let message = format!(
        "Cleaned {} repos, freed {} ({} errors)",
        cleaned,
        format_size(freed_bytes),
        errors
    );

    let details = serde_json::json!({
        "repos_cleaned": cleaned,
        "bytes_freed": freed_bytes,
        "human_freed": format_size(freed_bytes),
        "errors": errors,
        "deep": deep,
    });

    Ok(CommandResult::ok("clean", None, message).with_details(details))
}

/// Calculate directory size in bytes.
fn dir_size(path: &Path) -> u64 {
    let mut total: u64 = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(meta) = p.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

/// Format bytes as human-readable string.
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(512), "512 B");
    }

    #[test]
    fn test_format_size_kb() {
        assert_eq!(format_size(2048), "2.0 KB");
    }

    #[test]
    fn test_format_size_mb() {
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn test_format_size_gb() {
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn test_format_size_zero() {
        assert_eq!(format_size(0), "0 B");
    }

    #[test]
    fn test_dir_size_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(dir_size(dir.path()), 0);
    }

    #[test]
    fn test_dir_size_with_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.bin"), vec![0u8; 1024]).unwrap();
        assert_eq!(dir_size(dir.path()), 1024);
    }

    #[test]
    fn test_dir_size_nested() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("sub");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("file.bin"), vec![0u8; 2048]).unwrap();
        std::fs::write(dir.path().join("top.bin"), vec![0u8; 512]).unwrap();
        assert_eq!(dir_size(dir.path()), 2560);
    }
}
