use clap::Parser;
use fleet_build::commands;
use fleet_build::{print_result, FleetState};

/// fleet-build: Automated Rust crate build, test, fix, and push CLI.
#[derive(Parser, Debug)]
#[command(name = "fleet-build", version, about = "Automated Rust crate build/test/fix/push CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// Clean build: remove target dir, run cargo test, capture output
    Build {
        /// Path to the Rust crate
        repo_path: String,
        /// Keep target directory (don't clean before building)
        #[arg(long)]
        no_clean: bool,
    },
    /// Read error output and apply common automated fixes
    Fix {
        /// Path to the Rust crate
        repo_path: String,
        /// Error output to parse (reads from last build if not provided)
        #[arg(long)]
        errors: Option<String>,
    },
    /// git add -A, commit, push with secret scanning
    Push {
        /// Path to the Rust crate
        repo_path: String,
        /// Commit message
        #[arg(short, long, default_value = "Automated commit via fleet-build")]
        message: String,
        /// Skip secret scanning (dangerous!)
        #[arg(long)]
        skip_secret_scan: bool,
    },
    /// Run build+test+push on multiple repos in parallel
    Batch {
        /// Paths to Rust crates
        repo_paths: Vec<String>,
        /// Maximum parallel jobs
        #[arg(short, long, default_value_t = 4)]
        jobs: usize,
    },
    /// Scan ~/repos for uncommitted changes, failing tests, missing READMEs, etc.
    Status,
    /// Remove all target/ directories under ~/repos
    Clean {
        /// Also clean registry cache
        #[arg(long)]
        deep: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let mut state = FleetState::load().unwrap_or_else(|e| {
        eprintln!("Warning: could not load state: {}", e);
        FleetState { repos: vec![] }
    });

    let result = match cli.command {
        Commands::Build { repo_path, no_clean } => {
            commands::build::run(&repo_path, !no_clean, &mut state)
        }
        Commands::Fix { repo_path, errors } => {
            commands::fix::run(&repo_path, errors.as_deref(), &mut state)
        }
        Commands::Push { repo_path, message, skip_secret_scan } => {
            commands::push::run(&repo_path, &message, !skip_secret_scan, &mut state)
        }
        Commands::Batch { repo_paths, jobs } => {
            commands::batch::run(&repo_paths, jobs, &mut state)
        }
        Commands::Status => {
            commands::status::run(&mut state)
        }
        Commands::Clean { deep } => {
            commands::clean::run(deep, &mut state)
        }
    };

    if let Err(e) = state.save() {
        eprintln!("Warning: could not save state: {}", e);
    }

    let exit_code = match &result {
        Ok(r) => {
            print_result(r);
            r.exit_code
        }
        Err(e) => {
            let r = fleet_build::CommandResult::fail(
                "unknown",
                None,
                e.to_string(),
                fleet_build::exit_codes::BUILD_ERROR,
            );
            print_result(&r);
            r.exit_code
        }
    };

    std::process::exit(exit_code);
}
