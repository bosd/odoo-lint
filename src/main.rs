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
        #[arg(long, visible_alias = "odoo-version")]
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
        /// Exit with 0 even when there are violations (advisory hooks); errors still exit with 2
        #[arg(long)]
        exit_zero: bool,
        /// Folders with addons for checks across modules; replaces `addons-path` from the config
        #[arg(long, value_delimiter = ',')]
        addons_path: Option<Vec<String>>,
        /// Also apply fixes that may change behaviour or lose information
        #[arg(long)]
        unsafe_fixes: bool,
        /// Show the fixes as a diff instead of writing them
        #[arg(long)]
        diff: bool,
        /// Also print the Odoo version and the config used
        #[arg(short, long, conflicts_with = "quiet")]
        verbose: bool,
        /// Print only the findings, without the summary
        #[arg(short, long)]
        quiet: bool,
    },
    /// Report what modules need to run on a newer Odoo version, per version step
    UpgradeCheck {
        /// Files or directories to check
        #[arg(default_value = ".")]
        paths: Vec<PathBuf>,
        /// The Odoo version to upgrade to, e.g. 19.0
        #[arg(long)]
        target: String,
        /// Config file (`odoo-lint.toml` or `pyproject.toml`); skips discovery
        #[arg(long)]
        config: Option<PathBuf>,
        /// Rules to leave out; added to `ignore` from the config
        #[arg(long, value_delimiter = ',')]
        ignore: Vec<String>,
        /// Output format
        #[arg(long, value_enum, default_value_t)]
        output_format: ReportFormat,
        /// List every finding under its module
        #[arg(long)]
        show_findings: bool,
        /// Apply the safe fixes of the upgrade
        #[arg(long)]
        fix: bool,
        /// Also apply the unsafe fixes
        #[arg(long)]
        unsafe_fixes: bool,
        /// Set the manifest version of modules older than the target to `<target>.1.0.0`
        #[arg(long)]
        bump_version: bool,
        /// Folders with the addons of the target version (Odoo's included): also check views, fields, XML ids and models against them
        #[arg(long, value_delimiter = ',')]
        addons_path: Option<Vec<String>>,
        /// Show the fixes as a diff instead of writing them
        #[arg(long)]
        diff: bool,
    },
    /// Write a README badge (shields.io endpoint JSON): share of clean modules, or upgrade readiness
    Badge {
        /// Files or directories to check
        #[arg(default_value = ".")]
        paths: Vec<PathBuf>,
        /// Upgrade readiness for this Odoo version instead of the share of clean modules
        #[arg(long)]
        upgrade: Option<String>,
        /// Config file (`odoo-lint.toml` or `pyproject.toml`); skips discovery
        #[arg(long)]
        config: Option<PathBuf>,
        /// Write the JSON to this file instead of stdout
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Format XML files and stylesheets, moving only whitespace (lax), and never their content
    Format {
        /// Files or directories to format
        #[arg(default_value = ".")]
        paths: Vec<PathBuf>,
        /// Write nothing; exit with 1 when a file is not formatted (for CI)
        #[arg(long)]
        check: bool,
        /// Config file (`odoo-lint.toml` or `pyproject.toml`); skips discovery
        #[arg(long)]
        config: Option<PathBuf>,
        /// Skip excluded files even when passed explicitly (for pre-commit)
        #[arg(long)]
        force_exclude: bool,
    },
    /// Build each module's README.md and static/description/index.html from its readme/ fragments
    Readme {
        /// Modules, folders with modules, or files of modules (as hooks pass them)
        #[arg(default_value = ".")]
        paths: Vec<PathBuf>,
        /// Write nothing; exit with 1 when a file is out of date (for CI)
        #[arg(long)]
        check: bool,
    },
    /// Write each module's translation template (i18n/<module>.pot) from its source
    #[command(hide = true)]
    Pot {
        /// Modules, folders with modules, or files of modules (as hooks pass them)
        #[arg(default_value = ".")]
        paths: Vec<PathBuf>,
        /// Write nothing; exit with 1 when a template is out of date (for CI)
        #[arg(long)]
        check: bool,
        /// Write the templates into this folder as <module>.pot instead of each module's i18n/
        #[arg(long)]
        output_dir: Option<PathBuf>,
        /// Folders with the modules' dependencies, Odoo's included; replaces `addons-path` from the config
        #[arg(long, value_delimiter = ',')]
        addons_path: Option<Vec<String>>,
        /// Folders whose modules count as installed too, as in a database with all of them
        #[arg(long, value_delimiter = ',')]
        installed: Vec<PathBuf>,
        /// Install no module automatically (Odoo's --skip-auto-install, as OCA's CI does)
        #[arg(long)]
        skip_auto_install: bool,
    },
    /// Write requirements.txt from the Python external_dependencies of the addons in a folder
    Requirements {
        /// The folder with the addons
        #[arg(default_value = ".")]
        dir: PathBuf,
        /// Write nothing; exit with 1 when requirements.txt is out of date (for CI)
        #[arg(long)]
        check: bool,
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
enum ReportFormat {
    /// A table per version step
    #[default]
    Text,
    /// JSON with the modules and their findings per version step
    Json,
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
    // Stop quietly when the reader goes away (`odl check | head`), as other
    // command-line tools do, instead of panicking on the failed write.
    #[cfg(unix)]
    // SAFETY: restores the default action before any thread is started.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = Cli::parse();
    match cli.command {
        Commands::UpgradeCheck {
            paths,
            target,
            config,
            ignore,
            output_format,
            show_findings,
            fix,
            unsafe_fixes,
            bump_version,
            addons_path,
            diff,
        } => upgrade_check(UpgradeArgs {
            paths,
            target,
            config,
            ignore,
            json: output_format == ReportFormat::Json,
            show_findings,
            fix,
            unsafe_fixes,
            bump_version,
            addons_path,
            diff,
        }),
        Commands::Badge {
            paths,
            upgrade,
            config,
            output,
        } => badge(&paths, upgrade, config, output),
        Commands::Format {
            paths,
            check,
            config,
            force_exclude,
        } => format(&paths, check, config, force_exclude),
        Commands::Readme { paths, check } => match readme(&paths, check) {
            Ok(changes) => {
                for change in &changes {
                    let verb = match (change.written, change.removed) {
                        (true, true) => "removed",
                        (false, true) => "to remove:",
                        (true, false) => "wrote",
                        (false, false) => "out of date:",
                    };
                    eprintln!("{verb} {}", change.path.display());
                }
                if check && !changes.is_empty() {
                    eprintln!("Run `odl readme` to update {} file(s).", changes.len());
                    ExitCode::from(1)
                } else {
                    ExitCode::SUCCESS
                }
            }
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::from(2)
            }
        },
        Commands::Pot {
            paths,
            check,
            output_dir,
            addons_path,
            installed,
            skip_auto_install,
        } => match pot(
            &paths,
            check,
            output_dir.as_deref(),
            addons_path,
            &installed,
            skip_auto_install,
        ) {
            Ok(changes) => {
                for change in &changes {
                    let verb = if change.written { "wrote" } else { "out of date:" };
                    let note = if change.incomplete {
                        " (dependencies not found on the addons path: labels may be missing)"
                    } else {
                        ""
                    };
                    eprintln!("{verb} {}{note}", change.path.display());
                }
                if check && !changes.is_empty() {
                    eprintln!("Run `odl pot` to update {} file(s).", changes.len());
                    ExitCode::from(1)
                } else {
                    ExitCode::SUCCESS
                }
            }
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::from(2)
            }
        },
        Commands::Requirements { dir, check } => match odoo_lint::requirements::run(&dir, check) {
            Ok(change) => {
                use odoo_lint::requirements::Change;
                match (&change, check) {
                    (Some(Change::Written(path)), false) => eprintln!("wrote {}", path.display()),
                    (Some(Change::Removed(path)), false) => eprintln!("removed {}", path.display()),
                    (Some(Change::Written(path) | Change::Removed(path)), true) => {
                        eprintln!("out of date: {}", path.display());
                    }
                    (None, _) => {}
                }
                if check && change.is_some() {
                    eprintln!("Run `odl requirements` to update it.");
                    ExitCode::from(1)
                } else {
                    ExitCode::SUCCESS
                }
            }
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::from(2)
            }
        },
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
            exit_zero,
            addons_path,
            verbose,
            quiet,
        } => {
            let overrides = CliOverrides {
                target_version: version,
                select,
                ignore,
                force_exclude,
                addons_path,
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

            if verbose {
                if settings.target_explicit {
                    eprintln!("Odoo version: {}", settings.target_version);
                } else {
                    eprintln!("Odoo version: each module's manifest, else {}", settings.target_version);
                }
                match &config_path {
                    Some(config_path) => eprintln!("Config: {}", config_path.display()),
                    None => eprintln!("Config: none found (odoo-lint.toml or [tool.odoo-lint] in pyproject.toml)"),
                }
            }
            for warning in linter::addons_path_warnings(&paths, &settings) {
                eprintln!("warning: {warning}");
            }
            let mode = if unsafe_fixes { FixMode::Unsafe } else { FixMode::Safe };
            if diff {
                let result = fixer::fix_paths(&paths, &settings, mode);
                print!("{}", result.diff());
                eprintln!("{} fix(es) would change {} file(s).", result.fixed, result.files());
                return if result.files() == 0 || exit_zero {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                };
            }
            let (violations, fixed) = if fix {
                let result = fixer::fix_paths(&paths, &settings, mode);
                if let Err(err) = result.write() {
                    eprintln!("error: {err}");
                    return ExitCode::from(2);
                }
                (result.remaining, Some(result.fixed))
            } else {
                (linter::lint_paths(&paths, &settings), None)
            };
            print!("{}", output::render(output_format, &violations));
            if output_format == OutputFormat::Text && !quiet {
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
            if violations.is_empty() || exit_zero {
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

struct UpgradeArgs {
    paths: Vec<PathBuf>,
    target: String,
    config: Option<PathBuf>,
    ignore: Vec<String>,
    json: bool,
    show_findings: bool,
    fix: bool,
    unsafe_fixes: bool,
    bump_version: bool,
    addons_path: Option<Vec<String>>,
    diff: bool,
}

fn upgrade_check(args: UpgradeArgs) -> ExitCode {
    let target: odoo_lint::odoo_version::OdooVersion = match args.target.parse() {
        Ok(version) => version,
        Err(err) => {
            eprintln!("error: --target: {err}");
            return ExitCode::from(2);
        }
    };
    let overrides = CliOverrides {
        ignore: args.ignore,
        ..CliOverrides::default()
    };
    let mut settings = match Settings::load(&args.paths[0], args.config.as_deref(), overrides) {
        Ok(loaded) => loaded.settings,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };
    settings.target_addons_path = args
        .addons_path
        .map(|paths| odoo_lint::settings::expand_paths(std::path::Path::new("."), &paths));
    let sources = odoo_lint::sources::Sources::default();
    let mut changed: Vec<(PathBuf, String, String)> = Vec::new();
    let mut fixed = 0;
    if args.fix || args.diff {
        let mode = if args.unsafe_fixes {
            FixMode::Unsafe
        } else {
            FixMode::Safe
        };
        let (upgrade_settings, _) = odoo_lint::upgrade::settings_for(&settings, &args.paths, target, &sources);
        let result = fixer::fix_paths_with(&args.paths, &upgrade_settings, mode, &sources);
        fixed = result.fixed;
        changed = result.changed;
    }
    // On top of the fixes; written after the report, which reads the
    // version the module had.
    let bumps = if args.bump_version {
        odoo_lint::upgrade::bump_versions(&settings, &args.paths, target, &sources)
    } else {
        Vec::new()
    };
    if args.diff {
        for (path, _, new) in &bumps {
            match changed.iter_mut().find(|(p, _, _)| p == path) {
                Some(entry) => entry.2 = new.clone(),
                None => changed.push((
                    path.clone(),
                    sources.read_to_string(path).unwrap_or_default(),
                    new.clone(),
                )),
            }
        }
        changed.sort();
        for (path, old, new) in &changed {
            print!("{}", fixer::unified_diff(path, old, new));
        }
        eprintln!("{fixed} fix(es) would change {} file(s).", changed.len());
        return if changed.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        };
    }
    for (path, _, new) in &changed {
        if let Err(err) = std::fs::write(path, new) {
            eprintln!("error: cannot write {}: {err}", path.display());
            return ExitCode::from(2);
        }
    }
    if args.fix {
        eprintln!("Fixed {fixed} finding(s) in {} file(s).", changed.len());
    }
    let report = odoo_lint::upgrade::check(&settings, &args.paths, target);
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report).expect("report serializes"));
    } else {
        print!("{}", odoo_lint::upgrade::render_text(&report, args.show_findings));
    }
    for (path, _, new) in &bumps {
        if let Err(err) = std::fs::write(path, new) {
            eprintln!("error: cannot write {}: {err}", path.display());
            return ExitCode::from(2);
        }
    }
    if !bumps.is_empty() {
        eprintln!("Set the version of {} module(s) to {target}.1.0.0.", bumps.len());
    }
    if report.effort.changes == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn format(paths: &[PathBuf], check: bool, config: Option<PathBuf>, force_exclude: bool) -> ExitCode {
    let mut settings = match Settings::load(&paths[0], config.as_deref(), CliOverrides::default()) {
        Ok(loaded) => loaded.settings,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };
    settings.force_exclude |= force_exclude;
    let results = match odoo_lint::format::run(paths, &settings, check) {
        Ok(results) => results,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };
    let mut pending = 0;
    for (path, outcome) in &results {
        match outcome {
            odoo_lint::format::Outcome::Formatted(_) if check => {
                pending += 1;
                eprintln!("would format {}", path.display());
            }
            odoo_lint::format::Outcome::Formatted(_) => eprintln!("formatted {}", path.display()),
            odoo_lint::format::Outcome::Refused(reason) => eprintln!("left as is {}: {reason}", path.display()),
            odoo_lint::format::Outcome::Invalid(reason) => eprintln!("not XML {}: {reason}", path.display()),
            odoo_lint::format::Outcome::Unchanged => {}
        }
    }
    if pending > 0 {
        eprintln!("Run `odl format` to format {pending} file(s).");
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// `odl readme` in the style the configuration found from `paths[0]` asks for.
fn readme(paths: &[PathBuf], check: bool) -> std::io::Result<Vec<odoo_lint::readme::Change>> {
    let loaded = Settings::load(&paths[0], None, CliOverrides::default()).map_err(std::io::Error::other)?;
    let target = loaded.settings.target_version.to_string();
    let style = odoo_lint::readme::style(&loaded.settings.config, &target).map_err(std::io::Error::other)?;
    odoo_lint::readme::run_with(paths, check, &style)
}

fn pot(
    paths: &[PathBuf],
    check: bool,
    output_dir: Option<&std::path::Path>,
    addons_path: Option<Vec<String>>,
    installed: &[PathBuf],
    skip_auto_install: bool,
) -> std::io::Result<Vec<odoo_lint::pot::Change>> {
    let overrides = CliOverrides {
        addons_path,
        ..CliOverrides::default()
    };
    let settings = Settings::load(&paths[0], None, overrides)
        .map_err(std::io::Error::other)?
        .settings;
    let installation = odoo_lint::pot::Installation {
        addons_path: &settings.addons_path,
        installed,
        skip_auto_install,
    };
    odoo_lint::pot::run(paths, check, output_dir, settings.target_version, &installation)
}

fn badge(paths: &[PathBuf], upgrade: Option<String>, config: Option<PathBuf>, output: Option<PathBuf>) -> ExitCode {
    let settings = match Settings::load(&paths[0], config.as_deref(), CliOverrides::default()) {
        Ok(loaded) => loaded.settings,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };
    let value = match upgrade {
        Some(target) => match target.parse() {
            Ok(target) => odoo_lint::badge::upgrade_ready(&settings, paths, target),
            Err(err) => {
                eprintln!("error: --upgrade: {err}");
                return ExitCode::from(2);
            }
        },
        None => odoo_lint::badge::clean(&settings, paths),
    };
    let text = serde_json::to_string_pretty(&value).expect("badge serializes") + "\n";
    match output {
        Some(path) => {
            if let Err(err) = std::fs::write(&path, text) {
                eprintln!("error: cannot write {}: {err}", path.display());
                return ExitCode::from(2);
            }
        }
        None => print!("{text}"),
    }
    ExitCode::SUCCESS
}
