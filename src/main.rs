use clap::{Parser, Subcommand};
use odoo_lint::config::OdooLintConfig;
use odoo_lint::linter;
use std::path::PathBuf;
use std::process::ExitCode;

const DEFAULT_ODOO_VERSION: &str = "17.0";

#[derive(Parser)]
#[command(name = "odl", version, about = "Blazing fast Odoo linter in Rust")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Lint an Odoo addon directory or file
    Check {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Target Odoo version; overrides `target-version` from the config
        #[arg(short, long)]
        version: Option<String>,
        /// Config file (`odoo-lint.toml` or `pyproject.toml`); skips discovery
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Commands::Check { path, version, config } => {
            let loaded = match config {
                Some(file) => OdooLintConfig::from_file(&file).map(|c| (c, Some(file))),
                None => OdooLintConfig::discover(&path),
            };
            let (config, config_path) = match loaded {
                Ok(loaded) => loaded,
                Err(err) => {
                    eprintln!("error: {err}");
                    return ExitCode::from(2);
                }
            };
            let version = version
                .or_else(|| config.target_version.clone())
                .unwrap_or_else(|| DEFAULT_ODOO_VERSION.to_string());

            println!("🚀 Running odl check on {:?} (v{})...", path, version);
            if let Some(config_path) = &config_path {
                println!("⚙️  Using config {}", config_path.display());
            }
            let violations = linter::lint_directory(&path, &version, &config);
            if violations.is_empty() {
                println!("✨ No violations found!");
                ExitCode::SUCCESS
            } else {
                for v in &violations {
                    println!("{}", v);
                }
                ExitCode::from(1)
            }
        }
    }
}
