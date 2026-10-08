use clap::{Parser, Subcommand};
use odoo_lint::config::OdooLintConfig;
use odoo_lint::output::{self, OutputFormat};
use odoo_lint::settings::{CliOverrides, Settings};
use odoo_lint::{linter, rules};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "odl", version, about = "Blazing fast Odoo linter in Rust")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Lint Odoo addon directories or files
    Check {
        /// Files or directories to lint
        #[arg(default_value = ".")]
        paths: Vec<PathBuf>,
        /// Target Odoo version, e.g. 17.0; overrides `target-version` from the config
        #[arg(short, long, visible_alias = "odoo-version")]
        version: Option<String>,
        /// Config file (`odoo-lint.toml` or `pyproject.toml`); skips discovery
        #[arg(long)]
        config: Option<PathBuf>,
        /// Rules to enable (codes, names or code prefixes); replaces `select` from the config
        #[arg(long, value_delimiter = ',')]
        select: Option<Vec<String>>,
        /// Rules to disable; added to `ignore` from the config
        #[arg(long, value_delimiter = ',')]
        ignore: Vec<String>,
        /// Output format
        #[arg(long, value_enum, default_value_t)]
        output_format: OutputFormat,
    },
    /// Explain a rule, or list all rules when no code is given
    Rule {
        /// Rule code or name, e.g. C8101 or manifest-required-author
        code: Option<String>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Commands::Rule { code: None } => {
            for rule in rules::ALL {
                println!("{:<8} {:<26} {}", rule.code, rule.name, rule.summary);
            }
            ExitCode::SUCCESS
        }
        Commands::Rule { code: Some(code) } => match rules::find(&code) {
            Some(rule) => {
                print!("{}", rule.to_markdown());
                ExitCode::SUCCESS
            }
            None => {
                eprintln!("error: unknown rule '{code}' (run `odl rule` to list all rules)");
                ExitCode::from(2)
            }
        },
        Commands::Check {
            paths,
            version,
            config,
            select,
            ignore,
            output_format,
        } => {
            let loaded = match config {
                Some(file) => OdooLintConfig::from_file(&file).map(|c| (c, Some(file))),
                None => OdooLintConfig::discover(&paths[0]),
            };
            let (config, config_path) = match loaded {
                Ok(loaded) => loaded,
                Err(err) => {
                    eprintln!("error: {err}");
                    return ExitCode::from(2);
                }
            };
            let overrides = CliOverrides {
                target_version: version,
                select,
                ignore,
            };
            let (settings, warnings) = match Settings::new(config, config_path.as_deref(), overrides) {
                Ok(result) => result,
                Err(err) => {
                    eprintln!("error: {err}");
                    return ExitCode::from(2);
                }
            };
            for warning in &warnings {
                eprintln!("warning: {warning}");
            }

            if output_format == OutputFormat::Text {
                eprintln!("🚀 Running odl check (Odoo {})...", settings.target_version);
                if let Some(config_path) = &config_path {
                    eprintln!("⚙️  Using config {}", config_path.display());
                }
            }
            let violations = linter::lint_paths(&paths, &settings);
            print!("{}", output::render(output_format, &violations));
            if output_format == OutputFormat::Text {
                if violations.is_empty() {
                    println!("✨ No violations found!");
                } else {
                    println!("Found {} violation(s).", violations.len());
                }
            }
            if violations.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
    }
}
