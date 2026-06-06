# fleet-build

Automated Rust crate build, test, fix, and push CLI for agent workflows.

Built from analysis of 10 real sessions and 652 tool calls. The most repeated workflow in agent-driven Rust development is:
- `cd` into a repo (116 occurrences)
- `cargo test` (78 occurrences)
- Fix errors (45 edits)
- `git push` (22 occurrences)
- Batch loops across repos (30 occurrences)

`fleet-build` automates this entire loop.

## Installation

```bash
cargo install --git https://github.com/SuperInstance/fleet-build
```

Or build from source:

```bash
git clone https://github.com/SuperInstance/fleet-build
cd fleet-build
cargo install --path .
```

## Usage

### Build

Clean build: removes `target/`, runs `cargo test`, captures structured output.

```bash
# Full clean build
fleet-build build ~/repos/my-crate

# Build without cleaning target/
fleet-build build --no-clean ~/repos/my-crate
```

Output (non-TTY, JSON):
```json
{
  "command": "build",
  "repo": "/home/user/repos/my-crate",
  "success": true,
  "exit_code": 0,
  "message": "Build and tests passed",
  "details": {
    "tests_passed": 12,
    "tests_failed": 0
  }
}
```

### Fix

Read error output and apply common automated fixes (missing derives, wrong types, broken imports).

```bash
# Fix using last build errors
fleet-build fix ~/repos/my-crate

# Fix using explicit error text
fleet-build fix --errors "error[E0277]: the trait `Debug` is not implemented" ~/repos/my-crate
```

Auto-fixable patterns:
- Missing `#[derive(Debug)]`, `#[derive(Clone)]`, etc.
- Missing imports (`Serialize`, `Deserialize`, `Result`, `Command`, `Path`)
- Unused imports (flagged for removal)

### Push

Stage all changes, commit, and push with **automatic secret scanning**.

```bash
# Push with default commit message and secret scanning
fleet-build push ~/repos/my-crate

# Custom commit message
fleet-build push -m "feat: add new parser" ~/repos/my-crate

# Skip secret scanning (⚠️ dangerous)
fleet-build push --skip-secret-scan ~/repos/my-crate
```

Secret patterns detected:
- GitHub tokens (`ghp_`, `gho_`)
- OpenAI API keys (`sk-`)
- AWS access keys (`AKIA`)
- Bearer tokens
- Passwords
- RSA private keys
- Slack tokens (`xoxb-`)

### Batch

Run build + test + push on multiple repos in parallel.

```bash
# Process repos with default parallelism (4 jobs)
fleet-build batch ~/repos/crate-a ~/repos/crate-b ~/repos/crate-c

# Limit parallelism
fleet-build batch -j 2 ~/repos/crate-a ~/repos/crate-b
```

### Status

Scan `~/repos` for issues: uncommitted changes, failing tests, missing READMEs, repos not on GitHub.

```bash
fleet-build status
```

Output:
```
🔍 Scanning /home/user/repos

📋 Repository Status (5 repos):

  ✓ fleet-build — 0 issue(s)
  ✗ my-api — 3 issue(s)
    • Uncommitted changes
    • 2 unpushed commits
    • Missing README
  ✓ tools — 0 issue(s)
```

### Clean

Remove all `target/` directories under `~/repos`.

```bash
# Remove target/ dirs
fleet-build clean

# Deep clean (also remove Cargo.lock files)
fleet-build clean --deep
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Test failure |
| 2 | Build error |
| 3 | Push error |

## State File

Results are tracked in `~/.fleet-build-state.json`:

```json
{
  "repos": [
    {
      "path": "/home/user/repos/my-crate",
      "last_build": "2024-01-15T10:30:00Z",
      "last_test_pass": true,
      "last_push": "2024-01-15T10:31:00Z",
      "error_count": 0,
      "fix_count": 2
    }
  ]
}
```

## Output Modes

- **TTY (interactive):** Colorized human-readable output with emoji indicators
- **Non-TTY (piped/agent):** Structured JSON for programmatic consumption

## Architecture

```
src/
├── main.rs           # CLI entry point (clap)
├── lib.rs            # Core types, state management, utilities
└── commands/
    ├── mod.rs        # Command module re-exports
    ├── build.rs      # Clean build + cargo test
    ├── fix.rs        # Error parsing and auto-fix
    ├── push.rs       # Git operations with secret scanning
    ├── batch.rs      # Parallel multi-repo processing
    ├── status.rs     # Repository health scanning
    └── clean.rs      # Target directory cleanup
```

## Dependencies

- **clap** — CLI argument parsing
- **serde/serde_json** — JSON serialization
- **anyhow** — Error handling
- **colored** — Terminal colors
- **chrono** — Timestamp handling
- **regex** — Error pattern matching

## License

MIT
