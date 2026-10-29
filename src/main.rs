use clap::{Parser, Subcommand};
use odoo_lint::diagnostics::Violation;
use odoo_lint::fix::{Applicability, FixMode};
use odoo_lint::output::{self, OutputFormat};
use odoo_lint::settings::{CliOverrides, Loaded, Settings};
use odoo_lint::{fixer, linter, rules};
use std::path::{Path, PathBuf};
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
    Check(CheckArgs),
    /// Report what modules need to run on a newer Odoo version, per version step
    UpgradeCheck(UpgradeArgs),
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
    /// Write each module's translation template (i18n/<module>.pot) from its source; with --merge, update its .po files
    Pot(PotArgs),
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
    /// Explain a rule, or list the rules of a code prefix, or all of them
    Rule {
        /// Rule code or name, e.g. C8101 or manifest-required-author; a code
        /// prefix (W83) or number (8101) lists the rules it matches
        code: Option<String>,
        /// Output format
        #[arg(long, value_enum, default_value_t)]
        output_format: RuleFormat,
    },
}

/// The options of `odl check`.
#[derive(clap::Args)]
struct CheckArgs {
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
}

/// The options of `odl upgrade-check`.
#[derive(clap::Args)]
struct UpgradeArgs {
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
}

/// The options of `odl pot`.
#[derive(clap::Args)]
struct PotArgs {
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
    /// Count demo data as installed (Odoo's default up to 18.0)
    #[arg(long, conflicts_with = "without_demo")]
    with_demo: bool,
    /// Count demo data as not installed (Odoo's default from 19.0)
    #[arg(long)]
    without_demo: bool,
    /// Also update the module's .po files: new messages untranslated, removed ones obsolete (#~)
    #[arg(long, conflicts_with = "output_dir")]
    merge: bool,
    /// With --merge, drop removed messages instead of keeping them as obsolete
    #[arg(long, requires = "merge")]
    no_obsolete: bool,
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

/// A command's exit code, or the error that stopped it (exit code 2).
type Outcome = Result<ExitCode, String>;

fn main() -> ExitCode {
    // Stop quietly when the reader goes away (`odl check | head`), as other
    // command-line tools do, instead of panicking on the failed write.
    #[cfg(unix)]
    // SAFETY: restores the default action before any thread is started.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let outcome = match Cli::parse().command {
        Commands::Check(args) => check(args),
        Commands::UpgradeCheck(args) => upgrade_check(args),
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
        Commands::Readme { paths, check } => readme(&paths, check),
        Commands::Pot(args) => pot(&args),
        Commands::Requirements { dir, check } => requirements(&dir, check),
        Commands::Hook => {
            let mut event = String::new();
            // A hook must never break the agent: problems mean "nothing to report".
            if std::io::Read::read_to_string(&mut std::io::stdin(), &mut event).is_ok() {
                if let Some(output) = odoo_lint::hook::run(&event) {
                    println!("{output}");
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Server => odoo_lint::server::run()
            .map(|()| ExitCode::SUCCESS)
            .map_err(|err| err.to_string()),
        Commands::Mcp => odoo_lint::mcp::serve(std::io::stdin().lock(), std::io::stdout().lock())
            .map(|()| ExitCode::SUCCESS)
            .map_err(|err| err.to_string()),
        Commands::Rule { code, output_format } => rule(code.as_deref(), output_format),
    };
    outcome.unwrap_or_else(|err| {
        eprintln!("error: {err}");
        ExitCode::from(2)
    })
}

/// Exit code 1 when a check found something, else 0.
fn found(anything: bool) -> ExitCode {
    if anything {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// The settings for `paths`, with the configuration's warnings printed.
fn load(paths: &[PathBuf], config: Option<&Path>, overrides: CliOverrides) -> Result<Loaded, String> {
    let loaded = Settings::load(&paths[0], config, overrides)?;
    for warning in &loaded.warnings {
        eprintln!("warning: {warning}");
    }
    Ok(loaded)
}

fn write(path: &Path, contents: &str) -> Result<(), String> {
    std::fs::write(path, contents).map_err(|err| format!("cannot write {}: {err}", path.display()))
}

fn check(args: CheckArgs) -> Outcome {
    let missing = linter::missing_paths(&args.paths);
    if !missing.is_empty() {
        let missing: Vec<String> = missing
            .iter()
            .map(|path| format!("{}: no such file or directory", path.display()))
            .collect();
        return Err(missing.join("\nerror: "));
    }
    let overrides = CliOverrides {
        target_version: args.version,
        select: args.select,
        ignore: args.ignore,
        force_exclude: args.force_exclude,
        addons_path: args.addons_path,
    };
    let Loaded {
        settings, config_path, ..
    } = load(&args.paths, args.config.as_deref(), overrides)?;
    let paths = &args.paths;
    if args.verbose {
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
    for warning in linter::addons_path_warnings(paths, &settings) {
        eprintln!("warning: {warning}");
    }
    let mode = if args.unsafe_fixes {
        FixMode::Unsafe
    } else {
        FixMode::Safe
    };
    if args.diff {
        let result = fixer::fix_paths(paths, &settings, mode);
        print!("{}", result.diff());
        eprintln!("{} fix(es) would change {} file(s).", result.fixed, result.files());
        return Ok(found(result.files() > 0 && !args.exit_zero));
    }
    let (violations, fixed) = if args.fix {
        let result = fixer::fix_paths(paths, &settings, mode);
        result.write().map_err(|err| err.to_string())?;
        (result.remaining, Some(result.fixed))
    } else {
        (linter::lint_paths(paths, &settings), None)
    };
    print!("{}", output::render(args.output_format, &violations));
    if args.output_format == OutputFormat::Text && !args.quiet {
        if let Some(fixed) = fixed {
            println!("Fixed {fixed} violation(s).");
        }
        if violations.is_empty() {
            println!("✨ No violations found!");
        } else {
            println!(
                "Found {} violation(s).{}",
                violations.len(),
                fixable_hint(&violations, args.fix)
            );
        }
    }
    Ok(found(!violations.is_empty() && !args.exit_zero))
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

fn upgrade_check(args: UpgradeArgs) -> Outcome {
    let target: odoo_lint::odoo_version::OdooVersion = args.target.parse().map_err(|err| format!("--target: {err}"))?;
    let overrides = CliOverrides {
        ignore: args.ignore,
        ..CliOverrides::default()
    };
    let mut settings = load(&args.paths, args.config.as_deref(), overrides)?.settings;
    settings.target_addons_path = args
        .addons_path
        .map(|paths| odoo_lint::settings::expand_paths(Path::new("."), &paths));
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
        return Ok(found(!changed.is_empty()));
    }
    for (path, _, new) in &changed {
        write(path, new)?;
    }
    if args.fix {
        eprintln!("Fixed {fixed} finding(s) in {} file(s).", changed.len());
    }
    let report = odoo_lint::upgrade::check(&settings, &args.paths, target);
    if args.output_format == ReportFormat::Json {
        println!("{}", serde_json::to_string_pretty(&report).expect("report serializes"));
    } else {
        print!("{}", odoo_lint::upgrade::render_text(&report, args.show_findings));
    }
    for (path, _, new) in &bumps {
        write(path, new)?;
    }
    if !bumps.is_empty() {
        eprintln!("Set the version of {} module(s) to {target}.1.0.0.", bumps.len());
    }
    Ok(found(report.effort.changes > 0))
}

fn format(paths: &[PathBuf], check: bool, config: Option<PathBuf>, force_exclude: bool) -> Outcome {
    let mut settings = load(paths, config.as_deref(), CliOverrides::default())?.settings;
    settings.force_exclude |= force_exclude;
    let results = odoo_lint::format::run(paths, &settings, check).map_err(|err| err.to_string())?;
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
    }
    Ok(found(pending > 0))
}

/// `odl readme` in the style the configuration found from `paths[0]` asks for.
fn readme(paths: &[PathBuf], check: bool) -> Outcome {
    let settings = load(paths, None, CliOverrides::default())?.settings;
    let target = settings.target_version.to_string();
    let style = odoo_lint::readme::style(&settings.config, &target)?;
    let changes = odoo_lint::readme::run_with(paths, check, &style).map_err(|err| err.to_string())?;
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
    }
    Ok(found(check && !changes.is_empty()))
}

fn pot(args: &PotArgs) -> Outcome {
    let overrides = CliOverrides {
        addons_path: args.addons_path.clone(),
        ..CliOverrides::default()
    };
    let settings = load(&args.paths, None, overrides)?.settings;
    let installation = odoo_lint::pot::Installation {
        addons_path: &settings.addons_path,
        installed: &args.installed,
        skip_auto_install: args.skip_auto_install,
        demo: (args.with_demo || args.without_demo).then_some(args.with_demo),
    };
    let merge = args.merge.then_some(if args.no_obsolete {
        odoo_lint::pot::Obsolete::Drop
    } else {
        odoo_lint::pot::Obsolete::Keep
    });
    let changes = odoo_lint::pot::run(
        &args.paths,
        args.check,
        args.output_dir.as_deref(),
        settings.target_version,
        &installation,
        merge,
    )
    .map_err(|err| err.to_string())?;
    let mut outdated = 0;
    for change in &changes {
        if let Some(reason) = &change.skipped {
            eprintln!("skipped {}: {reason}", change.path.display());
            continue;
        }
        outdated += 1;
        let verb = if change.written { "wrote" } else { "out of date:" };
        let note = if change.incomplete {
            " (dependencies not found on the addons path: labels may be missing)"
        } else {
            ""
        };
        eprintln!("{verb} {}{note}", change.path.display());
    }
    if args.check && outdated > 0 {
        let command = if args.merge { "odl pot --merge" } else { "odl pot" };
        eprintln!("Run `{command}` to update {outdated} file(s).");
    }
    Ok(found(args.check && outdated > 0))
}

fn requirements(dir: &Path, check: bool) -> Outcome {
    use odoo_lint::requirements::Change;
    let change = odoo_lint::requirements::run(dir, check).map_err(|err| err.to_string())?;
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
    }
    Ok(found(check && change.is_some()))
}

/// `odl rule`: every rule, or the documentation of one.
fn rule(code: Option<&str>, output_format: RuleFormat) -> Outcome {
    let json = output_format == RuleFormat::Json;
    let listed = match code {
        None => rules::ALL.iter().collect(),
        Some(code) => match rules::find(code) {
            Some(rule) => {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&rule.to_json()).expect("rule serializes")
                    );
                } else {
                    print!("{}", rule.to_markdown());
                }
                return Ok(ExitCode::SUCCESS);
            }
            None => {
                let mut matching = rules::matching(code);
                matching.sort_by_key(|rule| rule.code);
                matching
            }
        },
    };
    if listed.is_empty() {
        let code = code.unwrap_or_default();
        return Err(format!("unknown rule '{code}' (run `odl rule` to list all rules)"));
    }
    if json {
        let listed: Vec<_> = listed.iter().map(|rule| rule.to_json()).collect();
        println!("{}", serde_json::to_string_pretty(&listed).expect("rules serialize"));
    } else {
        let width = listed.iter().map(|rule| rule.name.len()).max().unwrap_or_default();
        for rule in listed {
            println!("{:<8} {:<width$} {}", rule.code, rule.name, rule.summary);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn badge(paths: &[PathBuf], upgrade: Option<String>, config: Option<PathBuf>, output: Option<PathBuf>) -> Outcome {
    let settings = load(paths, config.as_deref(), CliOverrides::default())?.settings;
    let value = match upgrade {
        Some(target) => {
            let target = target.parse().map_err(|err| format!("--upgrade: {err}"))?;
            odoo_lint::badge::upgrade_ready(&settings, paths, target)
        }
        None => odoo_lint::badge::clean(&settings, paths),
    };
    let text = serde_json::to_string_pretty(&value).expect("badge serializes") + "\n";
    match output {
        Some(path) => write(&path, &text)?,
        None => print!("{text}"),
    }
    Ok(ExitCode::SUCCESS)
}
