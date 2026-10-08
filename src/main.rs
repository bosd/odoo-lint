use clap::{Parser, Subcommand};
use odoo_lint::diagnostics::Violation;
use odoo_lint::fix::{Applicability, FixMode};
use odoo_lint::output::{self, OutputFormat};
use odoo_lint::settings::{CliOverrides, Settings};
use odoo_lint::{fixer, linter, rules};
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
        /// Apply safe fixes
        #[arg(long)]
        fix: bool,
        /// Apply `exclude` to files given on the command line as well (for pre-commit)
        #[arg(long)]
        force_exclude: bool,
        /// Also apply fixes that may change behaviour or lose information
        #[arg(long)]
        unsafe_fixes: bool,
        /// Show the fixes as a diff instead of writing them
        #[arg(long)]
        diff: bool,
    },
    /// Lint the file an AI coding agent just edited (hook event JSON on stdin)
    Hook,
    /// Run a language server on stdin/stdout, for editors
    Server,
    /// Run a Model Context Protocol server on stdin/stdout, for AI coding agents
    Mcp,
    /// Explain a rule, or list all rules when no code is given
    Rule {
        /// Rule code or name, e.g. C8101 or manifest-required-author
        code: Option<String>,
        /// Output format
        #[arg(long, value_enum, default_value_t)]
        output_format: RuleFormat,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
enum RuleFormat {
    /// Table of rules, or the Markdown documentation of one rule
    #[default]
    Text,
    /// JSON with code, name, summary, Odoo version range and documentation
    Json,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Commands::Hook => {
            let mut event = String::new();
            // A hook must never break the agent: problems mean "nothing to report".
            if std::io::Read::read_to_string(&mut std::io::stdin(), &mut event).is_ok() {
                if let Some(output) = odoo_lint::hook::run(&event) {
                    println!("{output}");
                }
            }
            ExitCode::SUCCESS
        }
        Commands::Server => match odoo_lint::server::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::from(2)
            }
        },
        Commands::Mcp => match odoo_lint::mcp::serve(std::io::stdin().lock(), std::io::stdout().lock()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::from(2)
            }
        },
        Commands::Rule {
            code: None,
            output_format,
        } => {
            if output_format == RuleFormat::Json {
                let all: Vec<_> = rules::ALL.iter().map(rules::Rule::to_json).collect();
                println!("{}", serde_json::to_string_pretty(&all).expect("rules serialize"));
            } else {
                for rule in rules::ALL {
                    println!("{:<8} {:<26} {}", rule.code, rule.name, rule.summary);
                }
            }
            ExitCode::SUCCESS
        }
        Commands::Rule {
            code: Some(code),
            output_format,
        } => match rules::find(&code) {
            Some(rule) => {
                if output_format == RuleFormat::Json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&rule.to_json()).expect("rule serializes")
                    );
                } else {
                    print!("{}", rule.to_markdown());
                }
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
            fix,
            unsafe_fixes,
            diff,
            force_exclude,
        } => {
            let overrides = CliOverrides {
                target_version: version,
                select,
                ignore,
                force_exclude,
            };
            let loaded = match Settings::load(&paths[0], config.as_deref(), overrides) {
                Ok(loaded) => loaded,
                Err(err) => {
                    eprintln!("error: {err}");
                    return ExitCode::from(2);
                }
            };
            let (settings, config_path, warnings) = (loaded.settings, loaded.config_path, loaded.warnings);
            for warning in &warnings {
                eprintln!("warning: {warning}");
            }

            if output_format == OutputFormat::Text {
                eprintln!("🚀 Running odl check (Odoo {})...", settings.target_version);
                if let Some(config_path) = &config_path {
                    eprintln!("⚙️  Using config {}", config_path.display());
                }
            }
            let mode = if unsafe_fixes { FixMode::Unsafe } else { FixMode::Safe };
            if diff {
                let result = fixer::fix_paths(&paths, &settings, mode);
                for (path, old, new) in &result.changed {
                    print!("{}", fixer::unified_diff(path, old, new));
                }
                eprintln!(
                    "{} fix(es) would change {} file(s).",
                    result.fixed,
                    result.changed.len()
                );
                return if result.changed.is_empty() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                };
            }
            let (violations, fixed) = if fix {
                let result = fixer::fix_paths(&paths, &settings, mode);
                for (path, _, new) in &result.changed {
                    if let Err(err) = std::fs::write(path, new) {
                        eprintln!("error: cannot write {}: {err}", path.display());
                        return ExitCode::from(2);
                    }
                }
                (result.remaining, Some(result.fixed))
            } else {
                (linter::lint_paths(&paths, &settings), None)
            };
            print!("{}", output::render(output_format, &violations));
            if output_format == OutputFormat::Text {
                if let Some(fixed) = fixed {
                    println!("Fixed {fixed} violation(s).");
                }
                if violations.is_empty() {
                    println!("✨ No violations found!");
                } else {
                    println!(
                        "Found {} violation(s).{}",
                        violations.len(),
                        fixable_hint(&violations, fix)
                    );
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

/// " 3 fixable with `--fix` (1 more with `--unsafe-fixes`)." for the summary.
fn fixable_hint(violations: &[Violation], fixing: bool) -> String {
    let count = |applicability| {
        violations
            .iter()
            .filter(|v| v.fix.as_ref().is_some_and(|f| f.applicability == applicability))
            .count()
    };
    let (safe, unsafe_) = (count(Applicability::Safe), count(Applicability::Unsafe));
    match (safe, unsafe_, fixing) {
        (0, 0, _) => String::new(),
        (0, u, _) => format!(" {u} fixable with `--fix --unsafe-fixes`."),
        (s, 0, false) => format!(" {s} fixable with `--fix`."),
        (s, u, false) => format!(" {s} fixable with `--fix` ({u} more with `--unsafe-fixes`)."),
        (_, u, true) => format!(" {u} fixable with `--unsafe-fixes`."),
    }
}
