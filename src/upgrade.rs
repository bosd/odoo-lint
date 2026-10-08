//! `odl upgrade-check`: what a module needs to run on a newer Odoo version.
//!
//! For every module, the rules that start applying after the module's own
//! version (from its manifest) and up to the target report what has to
//! change, per version step, split into what `--fix` does automatically,
//! what an unsafe fix proposes for review, and what is left to do by hand.

use crate::diagnostics::Violation;
use crate::fix::Applicability;
use crate::linter::{collect_files, lint_files_with, modules_of};
use crate::odoo_version::OdooVersion;
use crate::rules::{self, Rule, ALL};
use crate::settings::{selector_matches, Settings, ViolationFilter};
use crate::sources::Sources;
use crate::xml::module_version;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// How much work findings are.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Effort {
    /// Findings in total.
    pub changes: usize,
    /// Fixed by `--fix`.
    pub automatic: usize,
    /// Fixed by `--fix --unsafe-fixes`, after review.
    pub review: usize,
    /// No fix: to do by hand.
    pub manual: usize,
}

impl Effort {
    fn add(&mut self, violation: &Violation) {
        self.changes += 1;
        match violation.fix.as_ref().map(|f| f.applicability) {
            Some(Applicability::Safe) => self.automatic += 1,
            Some(Applicability::Unsafe) => self.review += 1,
            None => self.manual += 1,
        }
    }

    fn merge(&mut self, other: Effort) {
        self.changes += other.changes;
        self.automatic += other.automatic;
        self.review += other.review;
        self.manual += other.manual;
    }

    /// "12 changes: 10 automatic, 1 to review, 1 by hand".
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.automatic > 0 {
            parts.push(format!("{} automatic", self.automatic));
        }
        if self.review > 0 {
            parts.push(format!("{} to review", self.review));
        }
        if self.manual > 0 {
            parts.push(format!("{} by hand", self.manual));
        }
        let noun = if self.changes == 1 { "change" } else { "changes" };
        format!("{} {noun}: {}", self.changes, parts.join(", "))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleReport {
    pub code: String,
    pub name: String,
    /// The version from which the rule applies: the upgrade step.
    pub since: String,
    #[serde(flatten)]
    pub effort: Effort,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModuleReport {
    pub name: String,
    pub path: String,
    /// The module's version from its manifest; `None` when malformed.
    pub version: Option<String>,
    #[serde(flatten)]
    pub effort: Effort,
    pub rules: Vec<RuleReport>,
    pub violations: Vec<Violation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub target: String,
    #[serde(flatten)]
    pub effort: Effort,
    pub modules: Vec<ModuleReport>,
}

/// The rules an upgrade to `target` brings: those with a first version up
/// to `target`, minus the ones the configuration ignores.
fn upgrade_rules(settings: &Settings, target: OdooVersion) -> Vec<&'static Rule> {
    ALL.iter()
        .filter(|rule| rule.min_odoo.is_some_and(|min| min <= target))
        .filter(|rule| !settings.ignore.iter().any(|s| selector_matches(s, rule)))
        .collect()
}

/// Whether `rule` is an upgrade rule (`U1801`), as opposed to a rule of the
/// other checkers that only applies from some version.
fn is_upgrade_rule(rule: &Rule) -> bool {
    rule.code.starts_with('U') && rule.code[1..].chars().all(|c| c.is_ascii_digit())
}

/// The module a file belongs to: the nearest folder in `modules`.
pub fn module_of<'a, T>(modules: &'a HashMap<PathBuf, T>, file: &Path) -> Option<(&'a PathBuf, &'a T)> {
    file.ancestors().skip(1).find_map(|dir| modules.get_key_value(dir))
}

/// Settings that lint `paths` for an upgrade to `target`: the upgrade rules,
/// each module checked as if on `target`, and only the findings of rules
/// newer than the module's own version. Also returns the modules' versions.
pub fn settings_for(
    base: &Settings,
    paths: &[PathBuf],
    target: OdooVersion,
    sources: &Sources,
) -> (Settings, HashMap<PathBuf, (String, Option<OdooVersion>)>) {
    let files = collect_files(paths, base);
    let modules: HashMap<PathBuf, (String, Option<OdooVersion>)> = modules_of(&files, sources)
        .iter()
        .map(|m| (m.path.clone(), (m.name.clone(), module_version(m, base.target_version))))
        .collect();
    let mut settings = base.clone();
    settings.target_version = target;
    settings.assume_module_version = Some(target);
    settings.select = upgrade_rules(base, target).iter().map(|r| r.code.to_string()).collect();
    let versions: Arc<HashMap<PathBuf, Option<OdooVersion>>> = Arc::new(
        modules
            .iter()
            .map(|(path, (_, version))| (path.clone(), *version))
            .collect(),
    );
    settings.violation_filter = Some(ViolationFilter(Arc::new(move |violation: &Violation| {
        let Some(rule) = rules::find(&violation.code) else {
            return false;
        };
        let Some(since) = rule.min_odoo else { return false };
        // A module of unknown version gets every step up to the target.
        match module_of(&versions, Path::new(&violation.file_path)) {
            // Upgrade rules (`U`) include the module's own version: a module
            // whose manifest already says 19.0 is not ready for 19.0 while
            // it has `<tree>` views or `_sql_constraints`. That is also where
            // a migration starts, with the version bumped first.
            Some((_, Some(current))) if is_upgrade_rule(rule) => since >= *current,
            Some((_, Some(current))) => since > *current,
            Some((_, None)) => true,
            None => false,
        }
    })));
    if settings.select.is_empty() {
        // Nothing to check; keep the linter from running every rule.
        settings.select = vec!["NONE".to_string()];
    }
    (settings, modules)
}

/// Lints `paths` for an upgrade to `target` and summarises per module.
pub fn check(base: &Settings, paths: &[PathBuf], target: OdooVersion) -> Report {
    let sources = Sources::default();
    let (settings, modules) = settings_for(base, paths, target, &sources);
    let files = collect_files(paths, &settings);
    let violations = lint_files_with(&files, &settings, &sources);
    report(target, &modules, violations)
}

/// Groups `violations` (already limited to the upgrade) per module and rule.
pub fn report(
    target: OdooVersion,
    modules: &HashMap<PathBuf, (String, Option<OdooVersion>)>,
    violations: Vec<Violation>,
) -> Report {
    let mut by_module: BTreeMap<&PathBuf, Vec<Violation>> = modules.keys().map(|p| (p, Vec::new())).collect();
    for violation in violations {
        if let Some((path, _)) = module_of(modules, Path::new(&violation.file_path)) {
            by_module.entry(path).or_default().push(violation);
        }
    }
    let mut total = Effort::default();
    let mut module_reports = Vec::new();
    for (path, violations) in by_module {
        let (name, version) = &modules[path];
        let mut effort = Effort::default();
        let mut by_rule: BTreeMap<(OdooVersion, String), (String, Effort)> = BTreeMap::new();
        for v in &violations {
            effort.add(v);
            let since = rules::find(&v.code).and_then(|r| r.min_odoo).unwrap_or(target);
            by_rule
                .entry((since, v.code.clone()))
                .or_insert_with(|| (v.name.clone(), Effort::default()))
                .1
                .add(v);
        }
        total.merge(effort);
        module_reports.push(ModuleReport {
            name: name.clone(),
            path: path.to_string_lossy().into_owned(),
            version: version.map(|v| v.to_string()),
            effort,
            rules: by_rule
                .into_iter()
                .map(|((since, code), (name, effort))| RuleReport {
                    code,
                    name,
                    since: since.to_string(),
                    effort,
                })
                .collect(),
            violations,
        });
    }
    Report {
        target: target.to_string(),
        effort: total,
        modules: module_reports,
    }
}

/// The report as text for the terminal.
pub fn render_text(report: &Report, show_violations: bool) -> String {
    let mut out = String::new();
    let mut ready = 0;
    for module in &report.modules {
        let from = module.version.as_deref().unwrap_or("unknown version");
        if module.effort.changes == 0 {
            ready += 1;
            continue;
        }
        let _ = writeln!(out, "{} ({from} → {})", module.name, report.target);
        for rule in &module.rules {
            let _ = writeln!(
                out,
                "  {:<5} {:<7} {:<40} {:>4}  {}",
                rule.since,
                rule.code,
                rule.name,
                rule.effort.changes,
                breakdown(&rule.effort)
            );
        }
        let _ = writeln!(out, "  {}", module.effort.summary());
        if show_violations {
            for v in &module.violations {
                let _ = writeln!(out, "    {}:{}: {} {}", v.file_path, v.line, v.code, v.message);
            }
        }
        out.push('\n');
    }
    let needing = report.modules.len() - ready;
    let modules = if report.modules.len() == 1 { "module" } else { "modules" };
    if needing == 0 {
        let _ = writeln!(
            out,
            "{} {modules} checked: ready for Odoo {}.",
            report.modules.len(),
            report.target
        );
    } else {
        let _ = writeln!(
            out,
            "{} {modules} checked for Odoo {}: {ready} ready, {needing} to upgrade ({}).",
            report.modules.len(),
            report.target,
            report.effort.summary()
        );
    }
    out
}

fn breakdown(effort: &Effort) -> String {
    let mut parts = Vec::new();
    if effort.automatic > 0 {
        parts.push(format!("{} automatic", effort.automatic));
    }
    if effort.review > 0 {
        parts.push(format!("{} to review", effort.review));
    }
    if effort.manual > 0 {
        parts.push(format!("{} by hand", effort.manual));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(root: &Path, path: &str, contents: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn module(root: &Path, name: &str, version: &str) {
        write(
            root,
            &format!("{name}/__manifest__.py"),
            &format!("{{'name': '{name}', 'version': '{version}', 'license': 'AGPL-3', 'data': ['views/a.xml']}}\n"),
        );
        write(root, &format!("{name}/__init__.py"), "from . import models\n");
        write(root, &format!("{name}/models/__init__.py"), "from . import partner\n");
        write(
            root,
            &format!("{name}/models/partner.py"),
            "from odoo import _, models\n\n\nclass Partner(models.Model):\n    _inherit = 'res.partner'\n\n    def action(self):\n        self._cr.execute('SELECT 1')\n        return _('Done')\n",
        );
        write(
            root,
            &format!("{name}/views/a.xml"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n<odoo>\n    <template id=\"t\">\n        <span class=\"ml-2\" t-esc=\"x\"/>\n    </template>\n</odoo>\n",
        );
    }

    #[test]
    fn each_module_gets_the_steps_after_its_version() {
        let dir = tempfile::tempdir().unwrap();
        module(dir.path(), "acme_old", "16.0.1.0.0");
        module(dir.path(), "acme_new", "19.0.1.0.0");
        let report = check(
            &Settings::default(),
            &[dir.path().to_path_buf()],
            OdooVersion::new(19, 0),
        );
        let by_name: HashMap<&str, &ModuleReport> = report.modules.iter().map(|m| (m.name.as_str(), m)).collect();

        let new = by_name["acme_new"];
        assert_eq!(new.effort.changes, 0, "{:?}", new.rules);

        let old = by_name["acme_old"];
        let steps: Vec<(&str, &str)> = old.rules.iter().map(|r| (r.since.as_str(), r.code.as_str())).collect();
        // 15.0 and older already apply to a 16.0 module.
        assert_eq!(steps, vec![("18.0", "W8161"), ("19.0", "W8165")]);
        assert_eq!(
            old.effort,
            Effort {
                changes: 2,
                automatic: 2,
                review: 0,
                manual: 0
            }
        );
        assert_eq!(report.effort.changes, 2);
        let text = render_text(&report, false);
        assert!(text.contains("acme_old (16.0 → 19.0)"), "{text}");
        assert!(
            text.contains("1 ready, 1 to upgrade (2 changes: 2 automatic)"),
            "{text}"
        );
    }

    #[test]
    fn older_modules_get_the_xml_steps_too() {
        let dir = tempfile::tempdir().unwrap();
        module(dir.path(), "acme_old", "14.0.1.0.0");
        let report = check(
            &Settings::default(),
            &[dir.path().to_path_buf()],
            OdooVersion::new(15, 0),
        );
        let codes: Vec<&str> = report.modules[0].rules.iter().map(|r| r.code.as_str()).collect();
        assert_eq!(codes, vec!["XML013", "XML101"]);
    }

    #[test]
    fn a_module_already_on_the_target_still_gets_its_upgrade_rules() {
        // The version is bumped first, the views are not migrated yet.
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "acme_bumped/__manifest__.py",
            "{'name': 'Bumped', 'version': '19.0.1.0.0', 'license': 'AGPL-3', 'data': ['views/search.xml']}\n",
        );
        write(dir.path(), "acme_bumped/__init__.py", "");
        write(
            dir.path(),
            "acme_bumped/views/search.xml",
            "<odoo>\n    <record id=\"view_search\" model=\"ir.ui.view\">\n        <field name=\"model\">res.partner</field>\n        <field name=\"arch\" type=\"xml\">\n            <search>\n                <group expand=\"0\" string=\"Group By\"><filter name=\"g\" context=\"{'group_by': 'name'}\"/></group>\n            </search>\n        </field>\n    </record>\n</odoo>\n",
        );
        let report = check(
            &Settings::default(),
            &[dir.path().to_path_buf()],
            OdooVersion::new(19, 0),
        );
        let codes: Vec<&str> = report.modules[0].rules.iter().map(|r| r.code.as_str()).collect();
        assert_eq!(codes, vec!["U1917"]);
    }
}
